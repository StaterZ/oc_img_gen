use std::{path::PathBuf, time::Duration};

use clap::Parser;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use indicatif::ProgressBar;
use itertools::Itertools;
use minifb::{Key, Scale, Window, WindowOptions};
use num::traits::ConstZero;
use palette::{Srgb, rgb::channels::Argb};
use triple_buffer::TripleBuffer;
use display_info::DisplayInfo;

use crate::{
	audio::{VoiceStateFlt, packet::Sample}, encoder::media_container::{MediaFile, Packet, PacketContent}, math::*, video::{braille, cmd::packet::Frame},
};
use super::{self as shared, DrawOptions, DrawTarget, Gpu, VoiceState};

#[derive(Debug, PartialEq, Eq, Clone, Copy, clap::ValueEnum)]
pub enum CliScale {
	X1,
	X2,
	X4,
	X8,
	X16,
	X32,
}

impl From<CliScale> for Scale {
	fn from(value: CliScale) -> Self {
		match value {
			CliScale::X1 => Scale::X1,
			CliScale::X2 => Scale::X2,
			CliScale::X4 => Scale::X4,
			CliScale::X8 => Scale::X8,
			CliScale::X16 => Scale::X16,
			CliScale::X32 => Scale::X32,
		}
	}
}

#[derive(Parser, Debug)]
pub struct Cli {
	#[arg(short = 'i', long = "in", help = "Path to image or video to encode")]
	pub in_path: PathBuf,

	#[arg(short = 'd', long = "diff", help = "visualize difference from last frame")]
	pub diff: bool,

	#[arg(short = 'r', long = "raw", help = "visualize underlying braille symbols")]
	pub show_raw: bool,

	#[arg(short = 'e', long = "export", help = "write frames to file")]
	pub export: bool,

	#[arg(
		long = "matrix-gap-size",
		help = "sub-pixels to skip between matrix cells, defaults to 0 to omitted",
		conflicts_with = "matrix_screen_size",
	)]
	pub matrix_gap_size: Option<Size<usize>>,

	#[arg(
		long = "matrix-screen-size",
		help = "screen size of matrix segments, this is used to derive the matrix gap size",
		conflicts_with = "matrix_gap_size",
	)]
	pub matrix_screen_size: Option<Size<usize>>,

	#[arg(short = 's', long = "scale", help = "scale of screens")]
	pub scale: Option<CliScale>,

	#[arg(
		short = 'S',
		long = "vst",
		help = "how many seconds of smoothing to apply to each voice to reduce popping",
		default_value_t = humantime::Duration::from(Duration::from_millis(1)),
	)]
	pub voice_smooth_time: humantime::Duration,
}

#[cfg(windows)]
fn remove_title_bar(window: &Window) {
	use windows::Win32::Foundation::HWND;
	use windows::Win32::UI::WindowsAndMessaging::{
		GetWindowLongW, SetWindowLongW, SetWindowPos,
		GWL_STYLE, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOZORDER,
		WS_CAPTION, WS_THICKFRAME, WS_MINIMIZEBOX, WS_MAXIMIZEBOX, WS_SYSMENU, HWND_TOP,
	};

	unsafe {
		let hwnd = HWND(window.get_window_handle());

		let style = GetWindowLongW(hwnd, GWL_STYLE);
		let remove = (WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU).0 as i32;
		SetWindowLongW(hwnd, GWL_STYLE, style & !remove);

		let size = window.get_size();
		SetWindowPos(
			hwnd,
			Some(HWND_TOP),
			0, 0,
			size.0 as i32,
			size.1 as i32,
			SWP_NOMOVE | SWP_NOZORDER | SWP_FRAMECHANGED,
		).unwrap();
	}
}

pub fn play(args: Cli) -> anyhow::Result<()> {
	let file = std::fs::read(&args.in_path)?;
	let file = MediaFile::from_bytes(&file)?;

	if file.header.num_tracks == 0 {
		eprintln!("No tracks");
		return Ok(());
	}

	let video_tracks = file.track_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_video())
		.map(|(index, desc)| {
			let size_cells = desc.content.as_video().unwrap().size.cast::<usize>();
			let size_pixels = size_cells * braille::SIZE;

			let pos: Option<Point<isize>> = try {
				let (x, y) = desc.name.split_once(',')?;
				Point::new(
					x.trim().parse().ok()?,
					y.trim().parse().ok()?,
				)
			};

			let scale = args.scale.map_or_else(
				|| if pos.is_none() { Scale::FitScreen } else { Scale::X1 },
				|scale| scale.into(),
			);

			let mut window = Window::new(
				&format!("Playing: {}", desc.name),
				size_pixels.w,
				size_pixels.h,
				WindowOptions {
					resize: false,
					scale,
					..Default::default()
				},
			).expect("Failed to open window");

			if let Some(pos) = pos {
				#[cfg(windows)] remove_title_bar(&window);

				let gap_size = args.matrix_gap_size
					.or_else(|| args.matrix_screen_size
						.map(|matrix_screen_size| {
							let gap = crate::encoder::cli::compute_gap_size(size_pixels, matrix_screen_size);
							eprintln!("auto-gap: {}", gap);
							gap
						}))
					.unwrap_or(Size::ZERO);

				let pos = pos * (size_pixels + gap_size).cast::<isize>();
				window.set_position(pos.x, pos.y);
			} else if let Ok(displays) = DisplayInfo::all() {
				if let Some(display) = displays.iter().filter(|d| d.is_primary).next() {
					let display_size = Size::new(display.width, display.height).cast::<isize>();
					let window_size = window.get_size();
					let window_size = Size::new(window_size.0, window_size.1).cast::<isize>();
					let pos = (display_size - window_size) / 2;
					window.set_position(pos.x, pos.y);
				}
			}

			VideoTrack {
				id: index as u8,
				window,
				target: DrawTarget::new(size_pixels, size_cells, 0xff00ff),
				gpu: Gpu::new(),
			}
		})
		.collect();

	let audio_track = file.track_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_audio())
		.next()
		.map(|(id, _)| AudioTrack {
			id: id as u8,
			next_frame_index: 0,
		});

	//a tape drive plays the pcm on its own, so the preview just runs through it next to the packets
	let mut pcm = file.track_descs
		.iter()
		.enumerate()
		.find(|(_, desc)| desc.content.is_pcm())
		.map(|(id, desc)| PcmTrack {
			samples: shared::decode_pcm(&file, id as u8),
			sample_rate: desc.content.as_pcm().unwrap().sample_rate as f32,
			pos: 0.0,
		});

	let voices = TripleBuffer::new(&vec![VoiceStateFlt::default(); audio_track.as_ref().map_or(0, |audio_track| file.track_descs[audio_track.id as usize].content.as_audio().unwrap().num_voices as usize)]);
	let (voices_in, mut voices_out) = voices.split();

	let packets = file.packets();
	let progress = ProgressBar::new(packets.len() as u64)
		.with_style(shared::progress_style());

	let mut render_state = RenderState {
		file: &file,
		packets,
		timer: std::time::Instant::now(),
		next_packet_index: 0,
		is_done: false,
		progress,

		video_tracks,
		audio_track,
		sound_card: SoundCard {
			voices: voices_in,
		},
	};

	// ── Audio ─────────────────────────────────────────────────
	let host = cpal::default_host();
	let device = host.default_output_device().expect("No audio output device");
	let config = device.default_output_config().expect("No default output config");

	let sample_rate = config.sample_rate() as f32;
	let channels = config.channels() as usize;

	let mut voice_states: Vec<VoiceState> = voices_out
		.output_buffer()
		.iter()
		.map(|v| VoiceState::new(v.frequency))
		.collect();

	let coefficient = shared::smooth_coefficient(sample_rate, args.voice_smooth_time.into());

	let stream = device.build_output_stream(
		config.into(),
		move |data: &mut [f32], _| {
			for frame in data.chunks_exact_mut(channels) {
				let voices = voices_out.read();
				let mut sample = 0.0;

				for (voice, state) in voices.iter().zip_eq(voice_states.iter_mut()) {
					sample += state.advance(voice, sample_rate, coefficient);
				}
				if let Some(pcm) = &mut pcm {
					sample += pcm.advance(sample_rate);
				}

				frame.fill(sample);
			}
		},
		|err| eprintln!("Audio error: {err}"),
		None,
	).expect("Failed to build audio stream");

	stream.play().expect("Failed to start audio stream");

	'outer: loop {
		render(&mut render_state, &args)?;
		//without windows there is nothing to close, so stop once everything played
		if render_state.video_tracks.is_empty() && render_state.is_done && render_state.timer.elapsed() >= file_length(&file) {
			break;
		}

		for video_track in render_state.video_tracks.iter_mut() {
			if !video_track.window.is_open() || video_track.window.is_key_down(Key::Escape) { break 'outer; }

			video_track.window
				.update_with_buffer(
					video_track.target.image.buffer(),
					video_track.target.image.size().w,
					video_track.target.image.size().h,
				).expect("Failed to update window");
		}
	}
	Ok(())
}

fn file_length(file: &MediaFile) -> Duration {
	file.track_descs
		.iter()
		.enumerate()
		.map(|(track_id, desc)| match desc.content.as_pcm() {
			Some(pcm) => shared::pcm_duration(pcm, file.get_pcm(track_id as u8)),
			None => (Frac::from(desc.num_packets) * desc.rate.cast::<u32>()).into(),
		})
		.max()
		.unwrap_or(Duration::ZERO)
}

struct PcmTrack {
	samples: Vec<f32>,
	sample_rate: f32,
	pos: f64,
}

impl PcmTrack {
	fn advance(&mut self, out_sample_rate: f32) -> f32 {
		let sample = self.samples.get(self.pos as usize).copied().unwrap_or(0.0);
		self.pos += (self.sample_rate / out_sample_rate) as f64;
		sample
	}
}

struct RenderState<'a> {
	file: &'a MediaFile,
	/// the packets of all streams, in play order
	packets: Vec<&'a Packet>,
	timer: std::time::Instant,
	next_packet_index: usize,
	is_done: bool,
	progress: ProgressBar,

	video_tracks: Vec<VideoTrack>,
	audio_track: Option<AudioTrack>,
	sound_card: SoundCard,
}

struct SoundCard {
	voices: triple_buffer::Input<Vec<VoiceStateFlt>>,
}

fn render(state: &mut RenderState, args: &Cli) -> anyhow::Result<()> {
	if state.is_done { return Ok(()); }

	let elapsed = state.timer.elapsed();
	let present_time = Frac::from(elapsed.as_secs()) + Frac::new(elapsed.subsec_nanos(), shared::NANOS_PER_SEC).cast();

	let length = &file_length(state.file);

	#[cfg(not(feature = "log"))]
	state.progress.set_message(format!("{}/{}",
		humantime::Duration::from(shared::clean_duration(elapsed)),
		humantime::Duration::from(shared::clean_duration(*length)),
	));

	#[cfg(feature = "log")] {
		eprintln!("t:{}/{}, p:{}/{}",
			humantime::Duration::from(shared::clean_duration(elapsed)),
			humantime::Duration::from(shared::clean_duration(*length)),
			state.next_packet_index,
			state.packets.len(),
		);
	}

	let opts = DrawOptions { diff: args.diff, show_raw: args.show_raw };

	for packet in state.packets
		.iter()
		.skip(state.next_packet_index)
	{
		match &packet.content {
			PacketContent::Video(frame) => {
				let track = state.video_tracks
					.iter_mut()
					.filter(|t| t.id == packet.track_id)
					.next()
					.unwrap();
				let desc = &state.file.track_descs[track.id as usize];

				if Frac::from(track.target.frame_index) * desc.rate.cast::<u64>() > present_time { break; } //SAFETY: packets are ordered, so break on the first one like this is ok
				state.progress.inc(1);

				if args.diff && frame.commands_len > 0 {
					for pixel in track.target.image.buffer_mut() {
						*pixel = 0xff0000;
					}
				}
				track.draw_packet(&opts, frame)?;

				if args.export {
					let path = format!("{}_{}_{}.png",
						shared::stem(&args.in_path).unwrap(),
						desc.name,
						track.target.frame_index - 1,
					);
					eprintln!("{}", path);
					shared::write_image(path, track.target.image
						.iter()
						.map(|p| Srgb::<u8>::from_u32::<Argb>(*p)))?;
				}
			},
			PacketContent::Audio(sample) if Some(packet.track_id) == state.audio_track.as_ref().map(|audio_track| audio_track.id) => {
				if let Some(track) = &mut state.audio_track {
					let desc = &state.file.track_descs[track.id as usize];

					if Frac::from(track.next_frame_index) * desc.rate.cast::<u64>() > present_time { break; } //SAFETY: packets are ordered, so break on the first one like this is ok
					track.next_frame_index += 1;
					state.progress.inc(1);

					track.play_packet(&mut state.sound_card, sample)?;
				}
			},
			_ => {},
		}
		state.next_packet_index += 1;
	}

	if state.next_packet_index + 1 >= state.packets.len() {
		state.is_done = true;
		for voice in state.sound_card.voices.input_buffer_publisher().iter_mut() {
			voice.frequency = 0.0;
			voice.volume = 0.0;
		}
	}
	Ok(())
}

struct VideoTrack {
	id: u8,
	window: Window,
	target: DrawTarget,
	gpu: Gpu,
}

impl VideoTrack {
	fn draw_packet(&mut self, opts: &DrawOptions, packet: &Frame) -> anyhow::Result<()> {
		self.target.draw_packet(&mut self.gpu, opts, packet)
	}
}

struct AudioTrack {
	id: u8,
	next_frame_index: u64,
}

impl AudioTrack {
	pub fn play_packet(&mut self, state: &mut SoundCard, packet: &Sample) -> anyhow::Result<()> {
		for (voice, target) in state.voices
			.input_buffer_publisher()
			.iter_mut()
			.zip_eq(&packet.voices)
		{
			*voice = shared::voice_target_from_sample(target);
		}
		Ok(())
	}
}

use std::{path::{Path, PathBuf}, time::Duration};
use deku::prelude::*;
use indicatif::{ProgressBar, ProgressStyle};
use num_traits::ConstZero;
use triple_buffer::TripleBuffer;
use itertools::Itertools;
use minifb::{Key, Scale, Window, WindowOptions};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use clap::Parser;

use crate::{
	FORMAT_VERSION, audio::{VoiceStateFlt, packet::Sample}, encoder::media_container::{MediaFile, PacketContent}, math::*, video::{
		Image, ImageIterator, braille::{self, Braille}, cmd::packet::{Command, CommandData, CommandKind, Frame}, oc_color::{RGB8, formatters::{Formatter, HybridFormatter}}
	}
};

#[derive(Parser, Debug)]
#[command(author, version)]
pub struct Cli {
	#[arg(
		short = 'i',
		long = "in",
		help = "Path to image or video to encode",
	)]
	pub in_path: PathBuf,

	#[arg(
		long = "diff",
		help = "visualize difference from last frame",
	)]
	pub diff: bool,

	#[arg(
		short = 'e',
		long = "export",
		help = "write frames to file",
	)]
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
}

fn progress_style() -> ProgressStyle {
	ProgressStyle::with_template("[{bar}] {msg} {pos}/{len}")
		.unwrap()
		.progress_chars("█▉▊▋▌▍▎▏ ")
}

fn remove_title_bar(window: &Window) {
	use windows::Win32::Foundation::HWND;
	use windows::Win32::UI::WindowsAndMessaging::{
		GetWindowLongW, SetWindowLongW, SetWindowPos,
		GWL_STYLE, SWP_FRAMECHANGED, SWP_NOMOVE, SWP_NOZORDER,
		WS_CAPTION, WS_THICKFRAME, WS_MINIMIZEBOX, WS_MAXIMIZEBOX, WS_SYSMENU, HWND_TOP
	};

	unsafe {
		let hwnd = HWND(window.get_window_handle());

		let style = GetWindowLongW(hwnd, GWL_STYLE);
		let remove = (WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU).0 as i32;
		SetWindowLongW(hwnd, GWL_STYLE, style & !remove);

		let size = window.get_size();
		eprintln!("{:?}", size);
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
	let file = MediaFile::from_bytes((file.as_slice(), 0))?.1;
	anyhow::ensure!(file.header.version == FORMAT_VERSION);
	
	if file.header.num_streams == 0 {
		eprintln!("No streams");
		return Ok(());
	}

	let num_video_streams = file.stream_descs
		.iter()
		.filter(|desc| desc.content.is_video())
		.count();

	let video_streams = file.stream_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_video())
		.map(|(index, desc)| {
			let size = desc.content.as_video().unwrap().size.cast::<usize>();
			let size_pixels = size * braille::SIZE;

			let pos: Option<Point<isize>> = try {
				let (x, y) = desc.name.split_once(',')?;
				Point::new(
					x.trim().parse().ok()?,
					y.trim().parse().ok()?,
				)
			};

			let scale = if pos.is_none() { Scale::FitScreen } else { Scale::X1 };

			let mut window = Window::new(
				&format!("Playing: {}", desc.name),
				size_pixels.w,
				size_pixels.h,
				WindowOptions {
					resize: false,
					//topmost: true,
					scale,
					..Default::default()
				},
			).expect("Failed to open window");

			if num_video_streams > 1 {
				remove_title_bar(&window);
			}

			if let Some(pos) = pos {
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
			}

			let init_color = 0xff00ff;
			VideoStream {
				id: index as u8,
				window,
				image: Image::new(size_pixels, init_color),
				diff_image: Image::new(size, Braille::with_index(0, RGB8::new(init_color), RGB8::new(init_color))),
				next_frame_index: 0,
			}
		})
		.collect();

	let audio_stream = file.stream_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_audio())
		.next()
		.map(|(id, _)| AudioStream {
			id: id as u8,
			next_frame_index: 0,
		});

	let voices = TripleBuffer::new(&vec![VoiceStateFlt::default(); audio_stream.as_ref().map_or(0, |audio_stream| file.stream_descs[audio_stream.id as usize].content.as_audio().unwrap().num_voices as usize)]);
	let (voices_in, mut voices_out) = voices.split();

	let progress = ProgressBar::new(file.packets.len() as u64)
		.with_style(progress_style());

	let mut render_state = RenderState {
		file: &file,
		timer: std::time::Instant::now(),
		next_packet_index: 0,
		is_done: false,
		progress,

		video_streams,
		audio_stream,
		gpu: Gpu {
			formatter: HybridFormatter::new(),
			background_color: RGB8::BLACK,
			foreground_color: RGB8::WHITE,
		},
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
	let mut voice_phases = vec![0.0; voices_out.output_buffer().len()];
	let stream = device.build_output_stream(
		config.into(),
		move |data: &mut [f32], _| {
			for frame in data.chunks_exact_mut(channels) {
				let mut sample = 0.0;
				for (voice, phase) in voices_out.read().iter().zip_eq(voice_phases.iter_mut()) {
					sample += (*phase * std::f32::consts::TAU).sin() * voice.volume;
					*phase += voice.frequency / sample_rate;
					*phase = phase.fract();
				}
				for channel_sample in frame {
					*channel_sample = sample;
				}
			}
		},
		|err| eprintln!("Audio error: {err}"),
		None,
	).expect("Failed to build audio stream");
	
	stream.play().expect("Failed to start audio stream");

	'outer: loop {
		render(&mut render_state, &args)?;

		for video_stream in render_state.video_streams.iter_mut() {
			if !video_stream.window.is_open() || video_stream.window.is_key_down(Key::Escape) { break 'outer; }

			video_stream.window
				.update_with_buffer(
					video_stream.image.buffer(),
					video_stream.image.size().w,
					video_stream.image.size().h
				).expect("Failed to update window");
		}
	}
	Ok(())
}

struct RenderState<'a> {
	file: &'a MediaFile,
	timer: std::time::Instant,
	next_packet_index: usize,
	is_done: bool,
	progress: ProgressBar,
	
	video_streams: Vec<VideoStream>,
	audio_stream: Option<AudioStream>,
	gpu: Gpu,
	sound_card: SoundCard,
}

struct Gpu {
	formatter: HybridFormatter,
	background_color: RGB8,
	foreground_color: RGB8,
}
struct SoundCard {
	voices: triple_buffer::Input<Vec<VoiceStateFlt>>
}

const MILIS_PER_SEC: u32 = 1_000;
const NANOS_PER_SEC: u32 = 1_000_000_000;
fn clean_duration(value: Duration) -> Duration {
	const NANOS_PER_MILI: u32 = NANOS_PER_SEC / MILIS_PER_SEC;
	Duration::new(value.as_secs(), value.subsec_nanos() / NANOS_PER_MILI * NANOS_PER_MILI)
}

fn render(state: &mut RenderState, args: &Cli) -> anyhow::Result<()> {
	if state.is_done { return Ok(()); }

	let elapsed = state.timer.elapsed();
	let present_time = Frac::from(elapsed.as_secs()) + Frac::new(elapsed.subsec_nanos(), NANOS_PER_SEC).cast();

	let length = &state.file.stream_descs
		.iter()
		.map(|desc| (Frac::from(desc.num_packets) * desc.rate.cast::<u32>()).into())
		.max()
		.unwrap_or(Duration::ZERO);

	#[cfg(not(feature = "log"))]
	state.progress.set_message(format!("{}/{}",
		humantime::Duration::from(clean_duration(elapsed)),
		humantime::Duration::from(clean_duration(*length))
	));

	#[cfg(feature = "log")] {
		eprintln!("t:{}/{}, p:{}/{}",
			humantime::Duration::from(clean_duration(elapsed)),
			humantime::Duration::from(clean_duration(*length)),
			state.next_packet_index,
			state.file.packets.len()
		);
	}

	for packet in state.file.packets
		.iter()
		.skip(state.next_packet_index)
	{
		match &packet.content {
			PacketContent::Video(frame) => {
				let stream = state.video_streams
					.iter_mut()
					.filter(|s| s.id == packet.stream_id)
					.next()
					.unwrap();
				let desc = &state.file.stream_descs[stream.id as usize];

				if Frac::from(stream.next_frame_index) * desc.rate.cast::<u64>() > present_time { break; } //SAFETY: packets are ordered, so break on the first one like this is ok
				stream.next_frame_index += 1;
				state.progress.inc(1);

				if args.diff && frame.commands_len > 0 {
					for pixel in stream.image.buffer_mut() {
						*pixel = 0xff0000;
					}
				}
				stream.draw_packet(&mut state.gpu, args, frame)?;

				if args.export {
					let path = format!("{}_{}_{}.png",
						stem(&args.in_path).unwrap(),
						desc.name,
						stream.next_frame_index - 1,
					);
					eprintln!("{}", path);
					write_image(path, stream.image
						.iter()
						.map(|p| RGB8::new(*p)))?;
				}
			},
			PacketContent::Audio(sample) if Some(packet.stream_id) == state.audio_stream.as_ref().map(|audio_stream| audio_stream.id) => {
				if let Some(stream) = &mut state.audio_stream {
					let desc = &state.file.stream_descs[stream.id as usize];

					if Frac::from(stream.next_frame_index) * desc.rate.cast::<u64>() > present_time { break; } //SAFETY: packets are ordered, so break on the first one like this is ok
					stream.next_frame_index += 1;
				state.progress.inc(1);

					stream.play_packet(&mut state.sound_card, sample)?;
				}
			},
			_ => {},
		}
		state.next_packet_index += 1;
	}

	if state.next_packet_index + 1 >= state.file.packets.len() {
		state.is_done = true;
		for voice in state.sound_card.voices.input_buffer_publisher().iter_mut() {
			voice.frequency = 0.0;
			voice.volume = 0.0;
		}
	}
	Ok(())
}

struct VideoStream {
	id: u8,
	window: Window,
	image: Image<u32>,
	diff_image: Image<Braille<RGB8>>,
	next_frame_index: u64,
}
impl VideoStream {
	pub fn draw_packet(&mut self, gpu: &mut Gpu, args: &Cli, packet: &Frame) -> anyhow::Result<()> {
		match packet.command_kind {
			CommandKind::Text => todo!(),
			CommandKind::Braille => {
				let mut parse_state = (packet.commands.as_slice(), 0);
				while parse_state.0.len() > 0 {
					let (next_parse_state, command) = Command::from_bytes(parse_state)?;
					parse_state = next_parse_state;

					if let Some(background) = command.background {
						gpu.background_color = gpu.formatter.inflate(background);
					}
					if let Some(foreground) = command.foreground {
						gpu.foreground_color = gpu.formatter.inflate(foreground);
					}
					
					let pos = command.pos.cast::<usize>();
					match command.data {
						CommandData::Raw(braille) => self.draw_braille_line(
							pos,
							braille
								.iter()
								.map(|i| Braille::with_index(
									*i,
									gpu.background_color,
									gpu.foreground_color,
								)),
							args,
						),
						CommandData::Rle(runs) => self.draw_braille_line(
							pos,
							runs
								.iter()
								.map(|run| run.map::<_, u8>(|i| Braille::with_index(
									i,
									gpu.background_color,
									gpu.foreground_color,
								)))
								.flatten(),
							args,
						),
					};
				}
			},
		}
		Ok(())
	}

	fn draw_braille_line(&mut self, pos: Point<usize>, braille: impl Iterator<Item = Braille<RGB8>>, args: &Cli) {
		for (char_offset, braille) in braille.enumerate() {
			let pos = pos + Point::new(char_offset, 0);
			if args.diff && self.next_frame_index > 1 && self.diff_image[pos] == braille {
				// if !(1..command.braille.len()).contains(&char_offset) {
				// 	let x = self.diff_image[pos];
				// }
				//debug_assert_range!(1..command.braille.len(), char_offset);
				self.draw_braille(pos, &Braille::with_index(0, 0x008000, 0x00ff00));
				continue;
			}
			
			self.diff_image[pos] = braille;

			self.draw_braille(pos, &braille.map(|c| c.value()));
		};
	}
	
	fn draw_braille(&mut self, pos: Point<usize>, braille: &Braille<u32>) {
		for (y, row) in braille
			.raster()
			.into_iter()
			.enumerate()
		{
			for (x, color) in row
				.into_iter()
				.enumerate()
			{
				let pos = pos * braille::SIZE + Point::new(x, y);
				self.image[pos] = color;
			}
		}
	}
}

struct AudioStream {
	id: u8,
	next_frame_index: u64,
}
impl AudioStream {
	pub fn play_packet(&mut self, state: &mut SoundCard, packet: &Sample) -> anyhow::Result<()> {
		for (voice, state) in state.voices
			.input_buffer_publisher()
			.iter_mut()
			.zip_eq(&packet.voices)
		{
			voice.frequency = state.frequency as f32 / u16::MAX as f32 * 20000.0;
			voice.volume = state.volume as f32 / u8::MAX as f32;
		}
		Ok(())
	}
}

fn write_image(path: impl AsRef<Path>, img: ImageIterator<impl Iterator<Item = RGB8>>) -> anyhow::Result<()> {
	let img = img
		.map(|p| lodepng::RGB::new(p.r, p.g, p.b))
		.collect();
	Ok(lodepng::encode24_file(
		path,
		img.buffer(),
		img.size().w,
		img.size().h,
	)?)
}

fn stem(path: &Path) -> Option<&str> {
	let path_str = path.to_str()?;
	Some(match path.extension() {
		Some(ext) => &path_str[..path_str.len() - ext.len() - 1],
		None => path_str,
	})
}

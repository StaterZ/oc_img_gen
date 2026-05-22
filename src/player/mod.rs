use std::{path::PathBuf, time::Duration};
use deku::DekuContainerRead;
use triple_buffer::TripleBuffer;
use itertools::Itertools;
use minifb::{Key, Scale, Window, WindowOptions};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use clap::Parser;

use crate::{
	FORMAT_VERSION,
	math::*,
	encoder::media_container::{MediaFile, PacketContent},
	audio::{packet::Sample, VoiceStateFlt},
	video::{
		Image,
		braille::{self, Braille},
		cmd::packet::{Command, CommandKind, Frame},
		oc_color::{RGB8, formatters::{Formatter, HybridFormatter}},
	},
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
}

pub fn play(args: Cli) -> anyhow::Result<()> {
	let file = std::fs::read(&args.in_path)?;
	let file = MediaFile::from_bytes((file.as_slice(), 0))?.1;
	anyhow::ensure!(file.header.version == FORMAT_VERSION);
	
	let video_streams = file.stream_descs
		.iter()
		.enumerate()
		.filter(|(_, desc)| desc.content.is_video())
		.map(|(index, desc)| {
			let size = desc.content.as_video().unwrap().size.cast::<usize>() * braille::SIZE;

			VideoStream {
				id: index as u8,
				window: Window::new(
					&format!("Playing: {}", desc.name),
					size.x,
					size.y,
					WindowOptions {
						//topmost: true,
						scale: Scale::FitScreen,
						..Default::default()
					},
				).expect("Failed to open window"),
				image: Image::new(size, 0xff00ff),
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
	let mut render_state = RenderState {
		file: &file,
		timer: std::time::Instant::now(),
		next_packet_index: 0,
		is_done: false,

		video_streams,
		audio_stream,
		gpu: Gpu {
			formatter: HybridFormatter::new(),
			background_color: RGB8::BLACK.value(),
			foreground_color: RGB8::BLACK.value(),
		},
		sound_card: SoundCard {
			voices: voices_in,
		},
	};

	// ── Audio ─────────────────────────────────────────────────
	let host   = cpal::default_host();
	let device = host.default_output_device().expect("No audio output device");
	let config = device.default_output_config().expect("No default output config");

	let sample_rate = config.sample_rate() as f32;
	let mut voice_phases = vec![0.0; voices_out.output_buffer().len()];
	let stream = device.build_output_stream(
		&config.into(),
		move |data: &mut [f32], _| {
			for sample in data.iter_mut() {
				*sample = 0.0;
				for (voice, phase) in voices_out.read().iter().zip_eq(voice_phases.iter_mut()) {
					*sample += (*phase * std::f32::consts::TAU).sin() * voice.volume;
					*phase += voice.frequency / 8.0 / sample_rate;
					*phase = phase.fract();
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
					&video_stream.image.buffer(),
					video_stream.image.size().x,
					video_stream.image.size().y
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
	
	video_streams: Vec<VideoStream>,
	audio_stream: Option<AudioStream>,
	gpu: Gpu,
	sound_card: SoundCard,
}

struct Gpu {
	formatter: HybridFormatter,
	background_color: u32,
	foreground_color: u32,
}
struct SoundCard {
	voices: triple_buffer::Input<Vec<VoiceStateFlt>>
}

const MILIS_PER_SEC: u32 = 1_000;
const NANOS_PER_SEC: u32 = 1_000_000_000;
fn frac_to_duration<T: GoodInt + Into<u64>>(value: Frac<T>) -> Duration where u32: From<T> {
	Duration::new(value.into_int_trunc().into(), value.cast::<u32>().into_int_frac(NANOS_PER_SEC))
}
fn clean_duration(value: Duration) -> Duration {
	const NANOS_PER_MILI: u32 = NANOS_PER_SEC / MILIS_PER_SEC;
	Duration::new(value.as_secs(), value.subsec_nanos() / NANOS_PER_MILI * NANOS_PER_MILI)
}

fn render(state: &mut RenderState, args: &Cli) -> anyhow::Result<()> {
	if state.is_done { return Ok(()); }

	let elapsed = state.timer.elapsed();
	let present_time = Frac::from(elapsed.as_secs()) + Frac::new(elapsed.subsec_nanos(), NANOS_PER_SEC).cast();
	
	let vis_desc = &state.file.stream_descs[0];
	let num_packets = state.file.stream_descs.iter().map(|desc| desc.num_packets as usize).sum();
	println!("t:{}/{}, p:{}/{}",
		humantime::Duration::from(clean_duration(elapsed)),
		humantime::Duration::from(clean_duration(frac_to_duration(Frac::from(vis_desc.num_packets) * vis_desc.rate.cast::<u32>()))),
		state.next_packet_index,
		num_packets
	);

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
				println!("{} {} {} {}", stream.next_frame_index, desc.rate, Frac::from(stream.next_frame_index) * desc.rate.cast::<u64>(), present_time);
				stream.next_frame_index += 1;

				if args.diff && frame.commands_len > 0 {
					for pixel in stream.image.buffer_mut() {
						*pixel = 0xff0000;
					}
				}
				stream.draw_packet(&mut state.gpu, frame)?
			},
			PacketContent::Audio(sample) if Some(packet.stream_id) == state.audio_stream.as_ref().map(|audio_stream| audio_stream.id) => {
				if let Some(stream) = &mut state.audio_stream {
					let desc = &state.file.stream_descs[stream.id as usize];

					if Frac::from(stream.next_frame_index) * desc.rate.cast::<u64>() > present_time { break; } //SAFETY: packets are ordered, so break on the first one like this is ok
					stream.next_frame_index += 1;

					stream.play_packet(&mut state.sound_card, sample)?;
				}
			},
			_ => {},
		}
		state.next_packet_index += 1;
	}

	if state.next_packet_index + 1 >= num_packets {
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
	next_frame_index: u64,
}
impl VideoStream {
	pub fn draw_packet(&mut self, gpu: &mut Gpu, packet: &Frame) -> anyhow::Result<()> {
		match packet.command_kind {
			CommandKind::Text => todo!(),
			CommandKind::Braille => {
				let mut parse_state = (packet.commands.as_slice(), 0);
				while parse_state.0.len() > 0 {
					let (next_parse_state, command) = Command::from_bytes(parse_state)?;
					parse_state = next_parse_state;

					if let Some(background) = command.background {
						gpu.background_color = gpu.formatter.inflate(background).value();
					}
					if let Some(foreground) = command.foreground {
						gpu.foreground_color = gpu.formatter.inflate(foreground).value();
					}
					
					for	(char_offset, braille) in command.braille.iter().enumerate() {
						let braille = Braille {
							id: {
								let bit7650 = (braille & 0b1110_0001) >> 0 << 0;
								let bit1    = (braille & 0b0000_1000) >> 3 << 1;
								let bit2    = (braille & 0b0000_0010) >> 1 << 2;
								let bit3    = (braille & 0b0001_0000) >> 4 << 3;
								let bit4    = (braille & 0b0000_0100) >> 2 << 4;
								bit4 | bit3 | bit2 | bit1 | bit7650
								//0b11101000
							},
							bg: gpu.background_color,
							fg: gpu.foreground_color,
						};
						for (y, row) in braille.raster().into_iter().enumerate() {
							for (x, color) in row.into_iter().enumerate() {
								let pos = (command.pos.cast::<usize>() + Point::new(char_offset, 0)) * braille::SIZE + Point::new(x, y);
								let index = pos.y * self.image.size().x + pos.x;
								//if index >= self.image.size().area() { continue; } //safety
								self.image.buffer_mut()[index] = color;
							}
						}
					}
				}
			},
		}
		Ok(())
	}
}

struct AudioStream {
	id: u8,
	next_frame_index: u64,
}
impl AudioStream {
	pub fn play_packet(&mut self, state: &mut SoundCard, packet: &Sample) -> anyhow::Result<()> {
		for (voice, state) in state.voices.input_buffer_publisher().iter_mut().zip_eq(&packet.voices) {
			voice.frequency = state.frequency as f32 / u16::MAX as f32 * 20000.0;
			voice.volume = state.volume as f32 / u8::MAX as f32;
		}
		Ok(())
	}
}

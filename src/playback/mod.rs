use std::path::Path;

use deku::DekuContainerRead;
use palette::{FromColor, Srgb, rgb::channels::Argb};

use crate::{
	audio::VoiceStateFlt,
	math::*,
	video::{
		Image, ImageIterator,
		braille::{self, Braille},
		cmd::packet::{Command, CommandData, CommandKind, Frame},
		oc_color::formatters::{Formatter, HybridFormatter},
		rgb,
	},
};

pub mod exporter;
pub mod player;

pub const MILIS_PER_SEC: u32 = 1_000;
pub const NANOS_PER_SEC: u32 = 1_000_000_000;

pub fn clean_duration(value: std::time::Duration) -> std::time::Duration {
	const NANOS_PER_MILI: u32 = NANOS_PER_SEC / MILIS_PER_SEC;
	std::time::Duration::new(value.as_secs(), value.subsec_nanos() / NANOS_PER_MILI * NANOS_PER_MILI)
}

pub fn progress_style() -> indicatif::ProgressStyle {
	indicatif::ProgressStyle::with_template("[{bar}] {msg} {pos}/{len}")
		.unwrap()
		.progress_chars("█▉▊▋▌▍▎▏ ")
}

pub fn stem(path: &Path) -> Option<&str> {
	let path_str = path.to_str()?;
	Some(match path.extension() {
		Some(ext) => &path_str[..path_str.len() - ext.len() - 1],
		None => path_str,
	})
}

pub fn write_image(path: impl AsRef<Path>, img: ImageIterator<impl Iterator<Item = Srgb<u8>>>) -> anyhow::Result<()> {
	let img = img
		.map(|p| lodepng::RGB::new(p.red, p.green, p.blue))
		.collect();
	Ok(lodepng::encode24_file(
		path,
		img.buffer(),
		img.size().w,
		img.size().h,
	)?)
}

pub struct Gpu {
	pub formatter: HybridFormatter,
	pub background_color: Srgb<u8>,
	pub foreground_color: Srgb<u8>,
}

impl Gpu {
	fn new() -> Self {
		Self {
			formatter: HybridFormatter::new(),
			background_color: *rgb::BLACK,
			foreground_color: *rgb::WHITE,
		}
	}
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DrawOptions {
	pub diff: bool,
	pub show_raw: bool,
}

pub struct DrawTarget {
	pub image: Image<u32>,
	pub diff_image: Image<Braille<Srgb<u8>>>,
	pub frame_index: u64,
}

impl DrawTarget {
	pub fn new(size_pixels: Size<usize>, size_cells: Size<usize>, init_color: u32) -> Self {
		let init_color_srgb = Srgb::<u8>::from_u32::<Argb>(init_color);
		Self {
			image: Image::new(size_pixels, init_color),
			diff_image: Image::new(size_cells, Braille::with_index(0, init_color_srgb, init_color_srgb)),
			frame_index: 0,
		}
	}

	pub fn draw_packet(&mut self, gpu: &mut Gpu, opts: &DrawOptions, packet: &Frame) -> anyhow::Result<()> {
		match packet.command_kind {
			CommandKind::Text => todo!(),
			CommandKind::Braille => {
				let mut parse_state = (packet.commands.as_slice(), 0);
				while parse_state.0.len() > 0 {
					let (next_parse_state, command) = Command::from_bytes(parse_state)?;
					parse_state = next_parse_state;

					if !opts.show_raw {
						if let Some(background) = command.background {
							gpu.background_color = Srgb::from_color(gpu.formatter.inflate(background)).into_format();
						}
						if let Some(foreground) = command.foreground {
							gpu.foreground_color = Srgb::from_color(gpu.formatter.inflate(foreground)).into_format();
						}
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
							opts,
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
							opts,
						),
					};
				}
			},
		}
		self.frame_index += 1;
		Ok(())
	}

	fn draw_braille_line(&mut self, pos: Point<usize>, braille: impl Iterator<Item = Braille<Srgb<u8>>>, opts: &DrawOptions) {
		for (char_offset, braille) in braille.enumerate() {
			let pos = pos + Point::new(char_offset, 0);
			if opts.diff && self.frame_index >= 1 && self.diff_image[pos] == braille {
				self.draw_braille(pos, &Braille::with_index(0, 0x008000, 0x00ff00));
				continue;
			}

			self.diff_image[pos] = braille;
			self.draw_braille(pos, &braille.map(|c| c.into_u32::<Argb>()));
		}
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

#[derive(Clone, Copy)]
pub struct VoiceState {
	pub phase: f32,
	pub cur_volume: f32,
	pub cur_freq: f32,
}

impl VoiceState {
	pub fn new(start_freq: f32) -> Self {
		Self { phase: 0.0, cur_volume: 0.0, cur_freq: start_freq }
	}

	pub fn advance(&mut self, target: &VoiceStateFlt, sample_rate: f32, smooth_coefficient: f32) -> f32 {
		self.cur_volume = target.volume + (self.cur_volume - target.volume) * smooth_coefficient;
		self.cur_freq = target.frequency + (self.cur_freq - target.frequency) * smooth_coefficient;

		let sample = (self.phase * std::f32::consts::TAU).sin() * self.cur_volume;

		self.phase += self.cur_freq / sample_rate;
		self.phase = self.phase.fract();

		sample
	}
}

pub fn smooth_coefficient(sample_rate: f32, smooth_time: std::time::Duration) -> f32 {
	(-1.0 / (sample_rate * smooth_time.as_secs_f32())).exp()
}

pub fn voice_target_from_sample(sample: &crate::audio::packet::VoiceState) -> VoiceStateFlt {
	VoiceStateFlt {
		frequency: sample.frequency as f32 / u16::MAX as f32 * 20000.0,
		volume: sample.volume as f32 / u8::MAX as f32,
	}
}

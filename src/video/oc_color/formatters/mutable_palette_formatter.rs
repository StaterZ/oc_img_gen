use palette::{Lab, Srgb, FromColor, rgb::channels::Argb};

use super::{
	super::{palette::{Palette, PaletteOr}, PackedColor},
	Formatter
};

pub struct MutablePaletteFormatter {
	dynamic_palette: Palette,
}

impl MutablePaletteFormatter {
	pub fn new() -> Self {
		Self {
			dynamic_palette: Palette::new([
				0xFFFFFF, 0xFFCC33, 0xCC66CC, 0x6699FF,
				0xFFFF33, 0x33CC33, 0xFF6699, 0x333333,
				0xCCCCCC, 0x336699, 0x9933CC, 0x333399,
				0x663300, 0x336600, 0xFF3333, 0x000000,
			].map(|c| Lab::from_color(Srgb::<u8>::from_u32::<Argb>(c).into_format::<f32>())))
		}
	}
}

impl Formatter for MutablePaletteFormatter {
	fn inflate(&self, color: PackedColor) -> Lab {
		match color.unpack() {
			PaletteOr::Palette(color) => self.dynamic_palette.inflate(color),
			PaletteOr::NonPalette(_color) => unreachable!(),
		}
	}
	
	fn deflate(&self, color: PaletteOr<Lab>) -> PackedColor {
		PackedColor::new(PaletteOr::Palette(match color {
			PaletteOr::Palette(color) => color,
			PaletteOr::NonPalette(color) => self.dynamic_palette.deflate(color),
		}))
	}
}

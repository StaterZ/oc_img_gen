
use crate::oc_color::PackedColor;

use super::{
	super::{palette::{Palette, PaletteOr}, RGB8},
	Formatter
};

pub struct MutablePaletteFormatter {
	dynamic_palette: Palette,
}

impl MutablePaletteFormatter {
	pub fn new() -> Self {
		Self {
			dynamic_palette: Palette::new([
				RGB8::new(0xFFFFFF), RGB8::new(0xFFCC33), RGB8::new(0xCC66CC), RGB8::new(0x6699FF),
				RGB8::new(0xFFFF33), RGB8::new(0x33CC33), RGB8::new(0xFF6699), RGB8::new(0x333333),
				RGB8::new(0xCCCCCC), RGB8::new(0x336699), RGB8::new(0x9933CC), RGB8::new(0x333399),
				RGB8::new(0x663300), RGB8::new(0x336600), RGB8::new(0xFF3333), RGB8::new(0x000000),
			])
		}
	}
}

impl Formatter for MutablePaletteFormatter {
	fn inflate(&self, color: PackedColor) -> RGB8 {
		match color.unpack() {
			PaletteOr::Palette(color) => self.dynamic_palette.inflate(color),
			PaletteOr::NonPalette(_color) => unreachable!(),
		}
	}
	
	fn deflate(&self, color: PaletteOr<RGB8>) -> PackedColor {
		PackedColor::new(PaletteOr::Palette(match color {
			PaletteOr::Palette(color) => color,
			PaletteOr::NonPalette(color) => self.dynamic_palette.deflate(color),
		}))
	}
}

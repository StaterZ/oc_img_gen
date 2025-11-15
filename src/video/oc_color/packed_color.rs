use std::fmt::Display;
use deku::prelude::*;

use super::formatters::hybrid_formatter::StaticColor;
use super::palette::{Palette, PaletteColor, PaletteOr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, DekuWrite)]
pub struct PackedColor(pub u8);

impl PackedColor {
	pub const BITS: u32 = u8::BITS;

	pub fn new(color: PaletteOr<StaticColor>) -> Self {
		match color {
			PaletteOr::Palette(color) => Self(color.into_inner()),
			PaletteOr::NonPalette(color) => Self(color.into_inner() + Palette::SIZE as u8),
		}
	}
	
	pub fn unpack(self) -> PaletteOr<StaticColor> {
		const SIZE_1U8: u8 = Palette::SIZE as u8 - 1; //TODO: wonky range pattern limitation
		match self.0 {
			0..=SIZE_1U8 => PaletteOr::Palette(PaletteColor::new(self.0)),
			_ => PaletteOr::NonPalette(StaticColor::new(self.0 - Palette::SIZE as u8)),
		}
	}
}

impl Display for PackedColor {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self.unpack() {
			PaletteOr::Palette(color) => write!(f, "{:02X}({})", self.0, color),
			PaletteOr::NonPalette(color) => write!(f, "{:02X}({})", self.0, color),
		}
	}
}

impl Into<u8> for PackedColor {
	fn into(self) -> u8 {
		self.0
	}
}

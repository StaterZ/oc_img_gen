use std::fmt::Display;

use more_asserts::*;

use super::RGB8;

pub struct Palette {
	pub dynamic_palette: [RGB8; Self::SIZE],
}

impl Palette {
	pub const SIZE: usize = 16;

	pub fn new(dynamic_palette: [RGB8; Self::SIZE]) -> Self {
		Self { dynamic_palette }
	}
	
	pub fn inflate(&self, color: PaletteColor) -> RGB8 {
		self.dynamic_palette[color.into_inner() as usize]
	}
	
	pub fn deflate(&self, color: RGB8) -> PaletteColor {
		PaletteColor::new(self.dynamic_palette
			.iter()
			.enumerate()
			.min_by_key(|(_i, item)| item.perceptual_delta(color))
			.unwrap().0 as u8
		) //unwarp is safe here since array size is comptime fixed as .len()>0
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PaletteColor(u8);

impl PaletteColor {
	pub fn new(value: u8) -> Self {
		debug_assert_lt!(value as usize, Palette::SIZE);
		Self(value)
	}

	pub fn into_inner(self) -> u8 {
		self.0
	}
}

impl Display for PaletteColor {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "P{}", self.0)
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PaletteOr<T> {
	Palette(PaletteColor),
	NonPalette(T),
}

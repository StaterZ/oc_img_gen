use std::fmt::Display;
use all_asserts::*;
use ordered_float::OrderedFloat;
use palette::{Lab, color_difference::ImprovedCiede2000};

#[derive(Debug, Clone)]
pub struct Palette {
	pub colors: [Lab; Self::SIZE],
}

impl Palette {
	pub const SIZE: usize = 16;

	pub const fn new(colors: [Lab; Self::SIZE]) -> Self {
		Self { colors }
	}
	
	pub fn inflate(&self, color: PaletteColor) -> Lab {
		self.colors[color.into_inner() as usize]
	}
	
	pub fn deflate(&self, color: Lab) -> PaletteColor {
		PaletteColor::new(self.colors
			.iter()
			.enumerate()
			.min_by_key(|(_i, item)| OrderedFloat(item.improved_difference(color)))
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

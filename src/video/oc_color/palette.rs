use std::fmt::Display;
use all_asserts::*;
use ordered_float::OrderedFloat;
use palette::{Lab, color_difference::Ciede2000};

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
		PaletteColor::new(nearest(&self.colors, color) as u8)
	}
}

/// index of the color closest to `color`, same pick as min_by_key over improved_difference
pub fn nearest(colors: &[Lab], color: Lab) -> usize {
	let mut best = 0;
	let mut best_score = OrderedFloat(f32::INFINITY);
	let mut best_delta = f32::INFINITY;
	for (i, item) in colors.iter().enumerate() {
		//ciede2000 >= |ΔL|/S_L, so skip colors whose lightness alone puts them past the best (with margin for float error)
		let l_bar = (item.l + color.l) / 2.0 - 50.0;
		let s_l = 1.0 + 0.015 * l_bar * l_bar / (l_bar * l_bar + 20.0).sqrt();
		let delta_l = item.l - color.l;
		let bound = best_delta * 1.001;
		if delta_l * delta_l / (s_l * s_l) - 1e-2 > bound * bound { continue; }

		//inlined improved_difference, so we keep the raw delta
		let delta = item.difference(color);
		let score = OrderedFloat(1.43f64 as f32 * delta.powf(0.7f64 as f32));
		if score < best_score {
			best = i;
			best_score = score;
			best_delta = delta;
		}
	}
	best
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

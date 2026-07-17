use std::fmt::Display;
use std::mem::MaybeUninit;
use image::imageops::ColorMap;
use lazy_static::lazy_static;
use all_asserts::*;
use ordered_float::OrderedFloat;
use palette::{Lab, Srgb, FromColor, color_difference::ImprovedCiede2000};

use super::super::PackedColor;
use super::super::palette::{Palette, PaletteOr};
use super::Formatter;

lazy_static! {
	static ref STATIC_PALETTE: [Lab; StaticColor::PALETTE_SIZE] = {
		let mut result =  [MaybeUninit::<Srgb<u8>>::uninit(); StaticColor::PALETTE_SIZE];
		let mut i = 0;
		for i_r in 0..StaticColor::NUM_REDS {
			let r = ((i_r * 0x100) / (StaticColor::NUM_REDS - 1)).min(0xff) as u8;
			for i_g in 0..StaticColor::NUM_GREENS {
				let g = ((i_g * 0x100) / (StaticColor::NUM_GREENS - 1)).min(0xff) as u8;
				for i_b in 0..StaticColor::NUM_BLUES {
					let b = ((i_b * 0x100) / (StaticColor::NUM_BLUES - 1)).min(0xff) as u8;
					result[i] = MaybeUninit::new(Srgb::<u8>::new(r, g, b));
					//eprintln!("{:02X}: {}", i, unsafe { result[i].assume_init() });
					i = i + 1;
				}
			}
		}
		
		unsafe { std::mem::transmute::<_, [Srgb<u8>; StaticColor::PALETTE_SIZE]>(result) }
	}.map(|c| Lab::from_color(c.into_format::<f32>()));

	static ref DYNAMIC_PALETTE_DEFAULT: Palette = Palette::new({
		let mut result =  [MaybeUninit::<Srgb<u8>>::uninit(); Palette::SIZE];
		for (i, item) in result.iter_mut().enumerate() {
			let shade = ((i + 1) * 0xFF / (Palette::SIZE + 1)) as u8;
			*item = MaybeUninit::new(Srgb::<u8>::new(shade, shade, shade));
			//eprintln!("{:02X}: {}", i, unsafe { item.assume_init() });
		}
		unsafe { std::mem::transmute::<_, [Srgb<u8>; Palette::SIZE]>(result) }
	}.map(|c| Lab::from_color(c.into_format::<f32>())));
}

pub struct HybridFormatter {
	palette: Palette,
}

impl HybridFormatter {
	pub const PALETTE_SIZE: usize = 16;

	pub fn new() -> Self {
		Self {
			palette: DYNAMIC_PALETTE_DEFAULT.clone(),
		}
	}

	fn inflate_impl(&self, color: PaletteOr<StaticColor>) -> Lab {
		match color {
			PaletteOr::Palette(color) => self.palette.inflate(color),
			PaletteOr::NonPalette(color) => color.inflate(),
		}
	}
}

impl Formatter for HybridFormatter {
	fn inflate(&self, color: PackedColor) -> Lab {
		self.inflate_impl(color.unpack())
	}

	fn deflate(&self, color: PaletteOr<Lab>) -> PackedColor {
		PackedColor::new(match color {
			PaletteOr::Palette(color) => PaletteOr::Palette(color),
			PaletteOr::NonPalette(color) => {
				let palette_color = self.palette.deflate(color);
				let static_color = StaticColor::deflate(color);
				
				std::cmp::min_by_key(
					PaletteOr::Palette(palette_color),
					PaletteOr::NonPalette(static_color),
					|quantized_color| OrderedFloat(color
						.improved_difference(self
							.inflate_impl(*quantized_color))))
			},
		})
	}
}

impl ColorMap for HybridFormatter {
	type Color = Lab;

	fn index_of(&self, color: &Self::Color) -> usize {
		self.deflate(PaletteOr::NonPalette(*color)).0 as usize
	}

	fn lookup(&self, index: usize) -> Option<Self::Color> {
		Some(self.inflate(PackedColor(index as u8)))
	}
	/// Determine if this implementation of `ColorMap` overrides the default `lookup`.
	fn has_lookup(&self) -> bool {
		true
	}

	fn map_color(&self, color: &mut Self::Color) {
		*color = self.inflate(self.deflate(PaletteOr::NonPalette(*color)))
	}
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StaticColor(u8);

impl StaticColor {
	const NUM_REDS: usize = 6;
	const NUM_GREENS: usize = 8;
	const NUM_BLUES: usize = 5;

	const PALETTE_SIZE: usize = (1 << u8::BITS) as usize - Palette::SIZE;

	pub fn new(value: u8) -> Self {
		debug_assert_lt!(value as usize, Self::PALETTE_SIZE);
		Self(value)
	}

	pub fn into_inner(self) -> u8 {
		self.0
	}
	
	pub fn inflate(self) -> Lab {
		STATIC_PALETTE[self.into_inner() as usize]
	}

	pub fn deflate(color: Lab) -> StaticColor {
		StaticColor::new(STATIC_PALETTE
			.iter()
			.enumerate()
			.min_by_key(|(_i, item)| OrderedFloat(item.improved_difference(color)))
			.unwrap().0 as u8
		) //unwarp is safe here since array size is comptime fixed as .len()>0
	}
}

impl Display for StaticColor {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let mut v = self.0;
		let r = v / (Self::NUM_GREENS * Self::NUM_BLUES) as u8;
		v -= r * (Self::NUM_GREENS * Self::NUM_BLUES) as u8;
		let g = v / Self::NUM_BLUES as u8;
		v -= g * Self::NUM_BLUES as u8;
		let b = v;
		
		write!(f, "R{:01}G{:01}B{:01}", r, g, b)
	}
}

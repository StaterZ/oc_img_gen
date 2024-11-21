use std::fmt::Display;
use std::mem::MaybeUninit;

use lazy_static::lazy_static;
use more_asserts::debug_assert_lt;
use szu::math::int_div_round;

use super::super::{RGB8, PackedColor};
use super::super::palette::{Palette, PaletteOr};
use super::Formatter;

lazy_static! {
	static ref STATIC_PALETTE: [RGB8; StaticColor::PALETTE_SIZE] = {
		let mut result =  [MaybeUninit::<RGB8>::uninit(); StaticColor::PALETTE_SIZE];
		let mut i = 0;
		for i_r in 0..StaticColor::NUM_REDS {
			let r = ((i_r * 0x100) / (StaticColor::NUM_REDS - 1)).min(0xff) as u8;
			for i_g in 0..StaticColor::NUM_GREENS {
				let g = ((i_g * 0x100) / (StaticColor::NUM_GREENS - 1)).min(0xff) as u8;
				for i_b in 0..StaticColor::NUM_BLUES {
					let b = ((i_b * 0x100) / (StaticColor::NUM_BLUES - 1)).min(0xff) as u8;
					result[i] = MaybeUninit::new(RGB8 { r, g, b });
					//println!("{:02X}: {}", i, unsafe { result[i].assume_init() });
					i = i + 1;
				}
			}
		}
		unsafe { std::mem::transmute(result) }
	};

	static ref DYNAMIC_PALETTE_DEFAULT: [RGB8; Palette::SIZE] = {
		let mut result =  [MaybeUninit::<RGB8>::uninit(); Palette::SIZE];
		for (i, item) in result.iter_mut().enumerate() {
			let shade = ((i + 1) * 0xFF / (Palette::SIZE + 1)) as u8;
			*item = MaybeUninit::new(RGB8 {
				r: shade,
				g: shade,
				b: shade,
			});
			//println!("{:02X}: {}", i, unsafe { item.assume_init() });
		}
		unsafe { std::mem::transmute(result) }
	};
}

pub struct HybridFormatter {
	palette: Palette,
}

impl HybridFormatter {
	pub fn new() -> Self {
		Self {
			palette: Palette::new(*DYNAMIC_PALETTE_DEFAULT),
		}
	}

	fn inflate_impl(&self, color: PaletteOr<StaticColor>) -> RGB8 {
		match color {
			PaletteOr::Palette(color) => self.palette.inflate(color),
			PaletteOr::NonPalette(color) => color.inflate(),
		}
	}
}

impl Formatter for HybridFormatter {
	fn inflate(&self, color: PackedColor) -> RGB8 {
		self.inflate_impl(color.unpack())
	}

	fn deflate(&self, color: PaletteOr<RGB8>) -> PackedColor {
		PackedColor::new(match color {
			PaletteOr::Palette(color) => PaletteOr::Palette(color),
			PaletteOr::NonPalette(color) => {
				let palette_color = self.palette.deflate(color);
				let static_color = StaticColor::deflate(color);
				
				std::cmp::min_by_key(
					PaletteOr::Palette(palette_color),
					PaletteOr::NonPalette(static_color),
					|quantized_color| color
						.perceptual_delta(self
							.inflate_impl(*quantized_color)))
			},
		})
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
	
	pub fn inflate(self) -> RGB8 {
		STATIC_PALETTE[self.into_inner() as usize]
	}

	pub fn deflate(color: RGB8) -> StaticColor {
		let i_r = int_div_round(color.r as usize * (StaticColor::NUM_REDS - 1), 0xFF) as u8;
		let i_g = int_div_round(color.g as usize * (StaticColor::NUM_GREENS - 1), 0xFF) as u8;
		let i_b = int_div_round(color.b as usize * (StaticColor::NUM_BLUES - 1), 0xFF) as u8;

		StaticColor::new(i_r * (StaticColor::NUM_GREENS * StaticColor::NUM_BLUES) as u8 + i_g * StaticColor::NUM_BLUES as u8 + i_b)
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

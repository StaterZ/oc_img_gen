use std::mem::MaybeUninit;
use num_traits::ConstZero;
use szu::{math::int_div_round, iter::MultiZipExt};

use crate::math::*;
use super::{image::Image, oc_color::{RGB, RGB8}};

#[cfg(feature = "gpu")]
mod gpu;

pub const SIZE: Size<usize> = Size::new(2, 4);
pub const WIDTH: usize = SIZE.x;
pub const HEIGHT: usize = SIZE.y;
pub const BITS: usize = WIDTH * HEIGHT;

pub struct Braille<T> {
	pub id: u8,
	pub bg: T,
	pub fg: T,
}

impl<T> Braille<T> {
	pub fn char(&self) -> char {
		std::char::from_u32('⠀' as u32 + self.char_index() as u32).unwrap() //unwrap is safe: we know the unicode range to be valid
	}

	pub fn char_index(&self) -> u8 {
		let bit7650 = (self.id & 0b1110_0001) >> 0 << 0;
		let bit1    = (self.id & 0b0000_0100) >> 2 << 1;
		let bit2    = (self.id & 0b0001_0000) >> 4 << 2;
		let bit3    = (self.id & 0b0000_0010) >> 1 << 3;
		let bit4    = (self.id & 0b0000_1000) >> 3 << 4;
		bit4 | bit3 | bit2 | bit1 | bit7650
	}

	pub fn raster(&self) -> [[&T; WIDTH]; HEIGHT] {
		std::array::from_fn::<_, { HEIGHT }, _>(move |y|
			std::array::from_fn::<_, { WIDTH }, _>(move |x| {
				let i = y * WIDTH + x;
				if (self.id >> i) & 1 == 0 { &self.bg } else { &self.fg }
			}))
	}

	pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Braille<U> {
		Braille::<U> {
			id: self.id,
			bg: f(&self.bg),
			fg: f(&self.fg),
		}
	}
}

impl Braille<RGB8> {
	#[inline(always)]
	fn compute_sharpness(&self) -> u32 {
		if self.id == 0x00 || self.id == 0xff { 0 } else { self.bg.perceptual_delta(self.fg) }
	}
	
	#[inline(always)]
	fn compute_irregularity(&self, pixels: &[RGB8; BITS]) -> u32 {
		(0..BITS).map(|i| {
			let pixel = pixels[i];
			if ((self.id as usize >> i) & 1) != 0 {
				self.fg.perceptual_delta(pixel)
			} else {
				self.bg.perceptual_delta(pixel)
			}
		}).sum()
	}

	#[inline(always)]
	fn compute_score(&self, pixels: &[RGB8; BITS]) -> i32 {
		let sharpness = self.compute_sharpness();
		let irregular = self.compute_irregularity(pixels);
		sharpness as i32 - irregular as i32
	}

	pub fn from_pixels_old(pixels: &[[RGB8; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[RGB8; BITS] = unsafe { std::mem::transmute(pixels) };

		(0u8..(1 << (BITS - 1))).map(move |group| {
				let (bg, fg) = {
					let mut bg_sum = RGB::<u32>::ZERO;
					let mut fg_sum = RGB::<u32>::ZERO;
					for i in 0..BITS {
						let bin_sum = if (group >> i) & 1 == 0 {
							&mut bg_sum
						} else {
							&mut fg_sum
						};
						*bin_sum += pixels[i].into();
					}

					fn div_rgb(lhs: RGB<u32>, rhs: u32) -> Option<RGB8> {
						(rhs != 0).then(|| RGB {
							r: int_div_round(lhs.r, rhs) as u8,
							g: int_div_round(lhs.g, rhs) as u8,
							b: int_div_round(lhs.b, rhs) as u8,
						})
					}
					
					(div_rgb(bg_sum, group.count_zeros()), div_rgb(fg_sum, group.count_ones()))
				};

				Self {
					id: group,
					bg: bg.unwrap_or_else(|| fg.unwrap()),
					fg: fg.unwrap_or_else(|| bg.unwrap()),
				}
			})
		.max_by_key(|candidate| candidate.compute_score(pixels))
		.unwrap() //unwrap is safe since iterator is Self::BITS long, that's always >0
	}

	pub fn from_pixels(pixels: &[[RGB8; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[RGB8; BITS] = unsafe { std::mem::transmute(pixels) };

		#[inline]
		fn div_rgb(lhs: &RGB<u32>, rhs: u32) -> Option<RGB8> {
			(rhs != 0).then(|| RGB {
				r: int_div_round(lhs.r, rhs) as u8,
				g: int_div_round(lhs.g, rhs) as u8,
				b: int_div_round(lhs.b, rhs) as u8,
			})
		}

		// Precompute per-bit sums as RGB<u32> so we can add/subtract quickly.
		let pixels32: [RGB<u32>; BITS] = std::array::from_fn(|i| pixels[i].into());

		const NUM_GROUPS: usize = 1 << (BITS - 1);

		// Start with group == 0 (all bits 0 => all pixels in bg)
		// bg_sum = sum of all pixel_sums, fg_sum = ZERO
		let mut bg_sum = pixels32.iter().copied().sum();
		let mut fg_sum = RGB::<u32>::ZERO;
		let mut bg_count: u32 = BITS as u32;
		let mut fg_count: u32 = 0;

		// Gray-code generation helpers:
		// Gray(i) = i ^ (i >> 1). We iterate i from 0..GROUPS and compute gray.
		// flipped bit index = trailing_zeros(prev_gray ^ gray).
		let mut best = MaybeUninit::<Braille<RGB8>>::uninit();
		let mut best_score = i32::MIN;

		let mut prev_gray: usize = 0;
		// For group 0 we already have bg_sum, fg_sum, counts set.
		for seq in 0..NUM_GROUPS {
			let gray = seq ^ (seq >> 1);
			// find flipped bit compared to previous gray (except seq==0)
			if seq != 0 {
				let diff = prev_gray ^ gray;
				let flipped_bit = diff.trailing_zeros() as usize; // 0..BITS-1
				// if bit in gray is 1, we moved that pixel from bg -> fg, else fg -> bg
				if ((gray >> flipped_bit) & 1) != 0 {
					// 0->1: move pixel_sums[flipped_bit] from bg_sum to fg_sum
					bg_sum -= pixels32[flipped_bit];
					fg_sum += pixels32[flipped_bit];
					bg_count -= 1;
					fg_count += 1;
				} else {
					// 1->0: move pixel_sums[flipped_bit] from fg_sum to bg_sum
					fg_sum -= pixels32[flipped_bit];
					bg_sum += pixels32[flipped_bit];
					fg_count -= 1;
					bg_count += 1;
				}
			}

			// Candidate id is the lower (BITS-1) bits of gray; keep type compatibility with original
			let id = (gray as u8) & ((1u8 << (BITS - 1)) - 1); // matches original group->id mapping

			// compute bg/fg averages (O(1)).
			// If either count==0, we fall back to other color per original logic.
			let bg_opt = div_rgb(&bg_sum, bg_count);
			let fg_opt = div_rgb(&fg_sum, fg_count);

			let (bg, fg) = match (bg_opt, fg_opt) {
				(Some(b), Some(f)) => (b, f),
				(Some(b), None) => (b, b),
				(None, Some(f)) => (f, f),
				(None, None) => unreachable!(),
			};

			let candidate = Braille { id, bg, fg };
			let score = candidate.compute_score(pixels);

			if score > best_score {
				best_score = score;
				best = MaybeUninit::new(candidate);
			}

			prev_gray = gray;
		}

		unsafe { best.assume_init() }
	}
}

#[cfg(feature = "gpu")]
pub use gpu::as_braille;

#[cfg(not(feature = "gpu"))]
pub fn as_braille(input: &Image<RGB8>) -> Image<Braille<RGB8>> {
	use rayon::prelude::*;

	let row_len = input.size().x as usize;
	let buffer: Vec<Braille<RGB8>> = input
		.buffer()
		.par_chunks_exact(HEIGHT * row_len)
		.flat_map(|row_block| {
			let rows = row_block.chunks_exact(row_len).collect_array::<HEIGHT>().unwrap(); //SAFETY: safe due to multiplying by HEIGHT in par_chunks_exact above
			(0..(row_len / WIDTH)).into_par_iter().map(move |col| {
				let start = col * WIDTH;
				let end = start + WIDTH;
				let cluster: [[RGB8; WIDTH]; HEIGHT] = std::array::from_fn(|y| *rows[y][start..end].as_array::<WIDTH>().unwrap());
				Braille::from_pixels(&cluster)
			})
		})
		.collect();

	Image::new(*input.size() / SIZE, buffer)
}

pub fn raster<T: Copy>(input: &Image<Braille<T>>) -> Image<T> {
	let buffer = input.buffer()
		.chunks_exact(input.size().x)
		.flat_map(|row| row
			.into_iter()
			.map(|c| c
				.raster()
				.into_iter())
			.multi_zip()
			.flatten())
		.flatten()
		.copied()
		.collect();

	Image::new(
		*input.size() * SIZE,
		buffer,
	)
}

use std::mem::MaybeUninit;
use num_traits::ConstZero;
use itertools::Itertools;
#[cfg(feature = "rayon")]
use rayon::prelude::*;
use szu::{math::int_div_round, iter::MultiZipExt};

use crate::math::*;
use crate::encoder::cli::BrailleStrategy;
#[cfg(not(feature = "rayon"))]
use super::image_iter::ImageIterator;
#[cfg(feature = "rayon")]
use super::image_iter::ParallelImageIterator;
use super::{
	image::Image,
	oc_color::{RGB, RGB8},
};

pub const SIZE: Size<usize> = Size::new(2, 4);
pub const WIDTH: usize = SIZE.w;
pub const HEIGHT: usize = SIZE.h;
pub const BITS: usize = WIDTH * HEIGHT;

#[derive(Debug, Clone, Copy, Eq)]
pub struct Braille<T> {
	pub id: u8,
	pub bg: T,
	pub fg: T,
}

impl<T> Braille<T> {
	pub fn new(c: char, bg: T, fg: T) -> Self {
		debug_assert!(('⠀'..='⣿').contains(&c));
		let i = (c as u32 - '⠀' as u32) as u8;
		Self::with_index(i, bg, fg)
	}

	pub fn with_index(i: u8, bg: T, fg: T) -> Self {
		let bit7650 = (i & 0b1110_0001) >> 0 << 0;
		let bit1    = (i & 0b0000_1000) >> 3 << 1;
		let bit2    = (i & 0b0000_0010) >> 1 << 2;
		let bit3    = (i & 0b0001_0000) >> 4 << 3;
		let bit4    = (i & 0b0000_0100) >> 2 << 4;
		let id = bit4 | bit3 | bit2 | bit1 | bit7650;

		Self {
			id,
			bg,
			fg,
		}
	}

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

	pub fn map<U>(&self, f: impl Fn(&T) -> U) -> Braille<U> {
		Braille::<U> {
			id: self.id,
			bg: f(&self.bg),
			fg: f(&self.fg),
		}
	}
}

impl<T: Copy> Braille<T> {
	pub fn raster(&self) -> [[T; WIDTH]; HEIGHT] {
		std::array::from_fn::<_, { HEIGHT }, _>(move |y|
			std::array::from_fn::<_, { WIDTH }, _>(move |x| {
				let i = y * WIDTH + x;
				if (self.id >> i) & 1 == 0 { self.bg } else { self.fg }
			}))
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
			let palette_color = if ((self.id as usize >> i) & 1) != 0 { self.fg } else { self.bg };
			pixel.perceptual_delta(palette_color)
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

		(0u8..(1 << (BITS - 1)))
			.map(move |group| {
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

	pub fn from_pixels_centroid_cohesion(pixels: &[[RGB8; WIDTH]; HEIGHT]) -> Self {
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

	pub fn from_pixels_polar_pair(pixels: &[[RGB8; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[RGB8; BITS] = unsafe { std::mem::transmute(pixels) };

		let [bg, fg] = pixels
			.iter()
			.array_combinations::<2>()
			.max_by_key(|[bg, fg]| bg.perceptual_delta(**fg))
			.unwrap(); //unwrap is safe since iterator is Self::BITS long, that's always >0
		let (bg, fg) = (*bg, *fg);
		
		let id = (0..BITS).map(|i| {
			let pixel = pixels[i];
			(((pixel.perceptual_delta(fg) < pixel.perceptual_delta(bg)) as usize) << i) as u8
		}).fold(0u8, |acc, item| acc | item);

		Self {
			id,
			bg,
			fg,
		}
	}

	pub fn from_pixels_axis_split(pixels: &[[RGB8; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[RGB8; BITS] = unsafe { std::mem::transmute(pixels) };

		// Find min/max per channel across all 8 pixels.
		let (mut min_r, mut max_r) = (u8::MAX, u8::MIN);
		let (mut min_g, mut max_g) = (u8::MAX, u8::MIN);
		let (mut min_b, mut max_b) = (u8::MAX, u8::MIN);
		for p in pixels {
			min_r = min_r.min(p.r); max_r = max_r.max(p.r);
			min_g = min_g.min(p.g); max_g = max_g.max(p.g);
			min_b = min_b.min(p.b); max_b = max_b.max(p.b);
		}

		let range_r = max_r as i32 - min_r as i32;
		let range_g = max_g as i32 - min_g as i32;
		let range_b = max_b as i32 - min_b as i32;

		// Pick the channel with the greatest spread — that's our split axis.
		let channel = |p: &RGB8| -> u8 {
			if range_r >= range_g && range_r >= range_b {
				p.r
			} else if range_g >= range_b {
				p.g
			} else {
				p.b
			}
		};

		// Sort pixel indices along that axis, split at the median.
		let mut idx: [usize; BITS] = std::array::from_fn(|i| i);
		idx.sort_by_key(|&i| channel(&pixels[i]));
		let (low, high) = idx.split_at(BITS / 2);

		// Average each half in its own bucket to seed bg/fg.
		let avg = |group: &[usize]| -> RGB8 {
			let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
			for &i in group {
				r += pixels[i].r as u32;
				g += pixels[i].g as u32;
				b += pixels[i].b as u32;
			}
			let n = group.len() as u32;
			RGB8 { r: (r / n) as u8, g: (g / n) as u8, b: (b / n) as u8 }
		};

		let (bg, fg) = (avg(low), avg(high));

		// Same final assignment rule as polar_pair: nearest perceptual match wins the bit.
		let id = (0..BITS).map(|i| {
			let pixel = pixels[i];
			(((pixel.perceptual_delta(fg) < pixel.perceptual_delta(bg)) as usize) << i) as u8
		}).fold(0u8, |acc, item| acc | item);

		Self { id, bg, fg }
	}
}

impl<T: PartialEq> PartialEq for Braille<T> {
	fn eq(&self, other: &Self) -> bool {
		for i in 0..BITS {
			let self_color = if (self.id >> i) & 1 == 0 { &self.bg } else { &self.fg };
			let other_color = if (other.id >> i) & 1 == 0 { &other.bg } else { &other.fg };
			if self_color != other_color { return false; }
		}
		return true;
	}
}

#[cfg(feature = "rayon")]
pub fn as_braille(input: &Image<RGB8>, strategy: BrailleStrategy) -> ParallelImageIterator<impl ParallelIterator<Item = Braille<RGB8>>> {
	let row_len = input.size().w as usize;
	ParallelImageIterator {
		size: input.size() / SIZE,
		iter: input
			.buffer()
			.par_chunks_exact(HEIGHT * row_len)
			.flat_map(move |row_block| {
				let rows = row_block.chunks_exact(row_len).collect_array::<HEIGHT>().unwrap(); //SAFETY: safe due to multiplying by HEIGHT in par_chunks_exact above
				(0..(row_len / WIDTH)).into_par_iter().map(move |col| {
					let start = col * WIDTH;
					let end = start + WIDTH;
					std::array::from_fn(|y| *rows[y][start..end].as_array::<WIDTH>().unwrap())
				})
			})
			.map(match strategy {
				BrailleStrategy::CentroidCohesion => |cluster| Braille::from_pixels_centroid_cohesion(&cluster),
				BrailleStrategy::PolarPair => |cluster| Braille::from_pixels_polar_pair(&cluster),
				BrailleStrategy::AxisSplit => |cluster| Braille::from_pixels_axis_split(&cluster),
			}),
	}
}
#[cfg(not(feature = "rayon"))]
pub fn as_braille(input: &Image<RGB8>, strategy: BrailleStrategy) -> ImageIterator<impl Iterator<Item = Braille<RGB8>>> {
	let row_len = input.size().w as usize;
	ImageIterator {
		size: input.size() / SIZE,
		iter: input
			.buffer()
			.chunks_exact(HEIGHT * row_len)
			.flat_map(move |row_block| {
				let rows = row_block.chunks_exact(row_len).collect_array::<HEIGHT>().unwrap(); //SAFETY: safe due to multiplying by HEIGHT in par_chunks_exact above
				(0..(row_len / WIDTH)).into_iter().map(move |col| {
					let start = col * WIDTH;
					let end = start + WIDTH;
					std::array::from_fn(|y| *rows[y][start..end].as_array::<WIDTH>().unwrap())
				})
			})
			.map(match strategy {
				BrailleStrategy::CentroidCohesion => |cluster| Braille::from_pixels_centroid_cohesion(&cluster),
				BrailleStrategy::PolarPair => |cluster| Braille::from_pixels_polar_pair(&cluster),
				BrailleStrategy::AxisSplit => |cluster| Braille::from_pixels_axis_split(&cluster),
			}),
	}
}

pub fn raster<T: Copy>(input: &Image<Braille<T>>) -> Image<T> {
	let buffer = input.buffer()
		.chunks_exact(input.size().w)
		.flat_map(|row| row
			.into_iter()
			.map(|c| c
				.raster()
				.into_iter())
			.multi_zip()
			.flatten())
		.flatten()
		.collect();

	Image::with_buffer(
		input.size() * SIZE,
		buffer,
	)
}

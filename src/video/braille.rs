use std::mem::MaybeUninit;
use itertools::Itertools;
use palette::{Lab, color_difference::ImprovedCiede2000};
use ordered_float::OrderedFloat;
#[cfg(feature = "rayon")]
use rayon::prelude::*;
use szu::iter::MultiZipExt;

use crate::math::*;
use crate::encoder::cli::BrailleStrategy;
use crate::video::rgb;
#[cfg(not(feature = "rayon"))]
use super::image_iter::ImageIterator;
#[cfg(feature = "rayon")]
use super::image_iter::ParallelImageIterator;
use super::image::Image;

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

impl Braille<Lab> {
	#[inline(always)]
	fn compute_sharpness(&self) -> f32 {
		if self.id == 0x00 || self.id == 0xff { 0.0 } else { self.bg.improved_difference(self.fg) }
	}
	
	#[inline(always)]
	fn compute_irregularity(&self, pixels: &[Lab; BITS]) -> f32 {
		(0..BITS).map(|i| {
			let pixel = pixels[i];
			let palette_color = if ((self.id as usize >> i) & 1) != 0 { self.fg } else { self.bg };
			pixel.improved_difference(palette_color)
		}).sum()
	}

	#[inline(always)]
	fn compute_score(&self, pixels: &[Lab; BITS]) -> i32 {
		let sharpness = self.compute_sharpness();
		let irregular = self.compute_irregularity(pixels);
		sharpness as i32 - irregular as i32
	}

	pub fn from_pixels_centroid_cohesion(pixels: &[[Lab; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[Lab; BITS] = unsafe { std::mem::transmute(pixels) };

		const NUM_GROUPS: usize = 1 << (BITS - 1);

		fn average_lab<I>(colors: I) -> Option<Lab>
		where
			I: IntoIterator<Item = Lab>,
		{
			let mut sum_l = 0.0;
			let mut sum_a = 0.0;
			let mut sum_b = 0.0;
			let mut count = 0u32;

			for c in colors {
				sum_l += c.l;
				sum_a += c.a;
				sum_b += c.b;
				count += 1;
			}

			if count == 0 {
				return None;
			}

			let n = count as f32;
			Some(Lab::new(sum_l / n, sum_a / n, sum_b / n))
		}

		// Start with group == 0 (all bits 0 => all pixels in bg)
		// bg_sum = sum of all pixel_sums, fg_sum = ZERO
		let mut bg_sum = pixels.iter().copied().fold(*rgb::ZERO_LAB, |acc, p| acc + p);
		let mut fg_sum = *rgb::ZERO_LAB;
		let mut bg_count: usize = BITS as usize;
		let mut fg_count: usize = 0;

		// Gray-code generation helpers:
		// Gray(i) = i ^ (i >> 1). We iterate i from 0..GROUPS and compute gray.
		// flipped bit index = trailing_zeros(prev_gray ^ gray).
		let mut best = MaybeUninit::<Braille<Lab>>::uninit();
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
					bg_sum -= pixels[flipped_bit];
					fg_sum += pixels[flipped_bit];
					bg_count -= 1;
					fg_count += 1;
				} else {
					// 1->0: move pixel_sums[flipped_bit] from fg_sum to bg_sum
					fg_sum -= pixels[flipped_bit];
					bg_sum += pixels[flipped_bit];
					fg_count -= 1;
					bg_count += 1;
				}
			}

			// Candidate id is the lower (BITS-1) bits of gray; keep type compatibility with original
			let id = (gray as u8) & ((1u8 << (BITS - 1)) - 1); // matches original group->id mapping

			fn safe_div(lhs: &Lab, rhs: usize) -> Option<Lab> {
				(rhs != 0).then_some(*lhs / rhs as f32)
			}
			let bg_opt = safe_div(&bg_sum, bg_count);
			let fg_opt = safe_div(&fg_sum, fg_count);

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

	pub fn from_pixels_polar_pair(pixels: &[[Lab; WIDTH]; HEIGHT]) -> Self {
		let pixels: &[Lab; BITS] = unsafe { std::mem::transmute(pixels) };

		let [bg, fg] = pixels
			.iter()
			.array_combinations::<2>()
			.max_by_key(|[bg, fg]| OrderedFloat(bg.improved_difference(**fg)))
			.unwrap(); //unwrap is safe since iterator is Self::BITS long, that's always >0
		let (bg, fg) = (*bg, *fg);
		
		let id = (0..BITS).map(|i| {
			let pixel = pixels[i];
			(((pixel.improved_difference(fg) < pixel.improved_difference(bg)) as usize) << i) as u8
		}).fold(0u8, |acc, item| acc | item);

		Self {
			id,
			bg,
			fg,
		}
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
pub fn as_braille(input: &Image<Lab>, strategy: BrailleStrategy) -> ParallelImageIterator<impl ParallelIterator<Item = Braille<Lab>>> {
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
			}),
	}
}
#[cfg(not(feature = "rayon"))]
pub fn as_braille(input: &Image<Lab>, strategy: BrailleStrategy) -> ImageIterator<impl Iterator<Item = Braille<Lab>>> {
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

use std::mem::MaybeUninit;
use itertools::Itertools;
use palette::{Lab, LinSrgb, FromColor, color_difference::ImprovedCiede2000};
use ordered_float::OrderedFloat;
#[cfg(feature = "rayon")]
use rayon::prelude::*;
use szu::iter::MultiZipExt;

use crate::math::*;
use crate::video::rgb;
use crate::video::oc_color::{PackedColor, formatters::{Formatter, HybridFormatter}};
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

	/// same as compute_score, but gives up once the score can't beat `best_score`
	#[inline(always)]
	fn compute_score_above(&self, pixels: &[Lab; BITS], best_score: i32) -> Option<i32> {
		let sharpness = self.compute_sharpness() as i32;
		if sharpness <= best_score { return None; }

		let mut irregular = 0.0;
		for i in 0..BITS {
			let palette_color = if ((self.id as usize >> i) & 1) != 0 { self.fg } else { self.bg };
			irregular += pixels[i].improved_difference(palette_color);
			if sharpness - irregular as i32 <= best_score { return None; } //irregular only grows
		}
		Some(sharpness - irregular as i32)
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

		// Start with every pixel in the background.
		let mut bg_sum = pixels.iter().copied().fold(*rgb::ZERO_LAB, |acc, p| acc + p);
		let mut fg_sum = *rgb::ZERO_LAB;
		let mut bg_count: usize = BITS as usize;
		let mut fg_count: usize = 0;

		// Walk the ids in Gray-code order so each step moves exactly one pixel
		// between bg and fg, letting the sums update incrementally.
		// Only half the ids are tried: an id and its inverse give the same split.
		let mut best = MaybeUninit::<Braille<Lab>>::uninit();
		let mut best_score = i32::MIN;

		let mut prev_gray: usize = 0;
		for seq in 0..NUM_GROUPS {
			let gray = seq ^ (seq >> 1);
			if seq != 0 {
				let flipped_bit = (prev_gray ^ gray).trailing_zeros() as usize;
				if ((gray >> flipped_bit) & 1) != 0 {
					// bg -> fg
					bg_sum -= pixels[flipped_bit];
					fg_sum += pixels[flipped_bit];
					bg_count -= 1;
					fg_count += 1;
				} else {
					// fg -> bg
					fg_sum -= pixels[flipped_bit];
					bg_sum += pixels[flipped_bit];
					fg_count -= 1;
					bg_count += 1;
				}
			}

			let id = (gray as u8) & ((1u8 << (BITS - 1)) - 1);

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
			if let Some(score) = candidate.compute_score_above(pixels, best_score) {
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

/// the palette colors near a cell's pixels, with each one's error against every pixel
struct PaletteCandidates {
	colors: [PackedColor; Self::MAX],
	labs: [Lab; Self::MAX],
	errors: [[f32; BITS]; Self::MAX],
	len: usize,
}

impl PaletteCandidates {
	const MAX: usize = BITS * 9;

	fn new(pixels: &[Lab; BITS], formatter: &HybridFormatter) -> Self {
		let mut result = Self {
			colors: [PackedColor(0); Self::MAX],
			labs: [*rgb::ZERO_LAB; Self::MAX],
			errors: [[0.0; BITS]; Self::MAX],
			len: 0,
		};
		let mut seen = [false; 1 << PackedColor::BITS];
		for pixel in pixels {
			for color in formatter.neighbors(*pixel) {
				if std::mem::replace(&mut seen[color.0 as usize], true) { continue; }
				let color_lab = formatter.inflate(color);
				result.colors[result.len] = color;
				result.labs[result.len] = color_lab;
				result.errors[result.len] = pixels.map(|p| p.improved_difference(color_lab));
				result.len += 1;
			}
		}
		result
	}
}

impl Braille<PackedColor> {
	/// picks bg and fg straight from the screen palette, so the error is measured after quantization instead of before it.
	/// every pair of palette colors near the pixels is tried, and each pixel takes the closer color of the pair
	pub fn from_pixels_palette_pair(pixels: &[[Lab; WIDTH]; HEIGHT], formatter: &HybridFormatter) -> Self {
		let pixels: &[Lab; BITS] = unsafe { std::mem::transmute(pixels) };
		let candidates = PaletteCandidates::new(pixels, formatter);
		let errors = &candidates.errors;

		let mut best = (0, 0);
		let mut best_error = f32::INFINITY;
		for a in 0..candidates.len {
			'pairs: for b in a..candidates.len {
				let mut error = 0.0;
				for i in 0..BITS {
					error += errors[a][i].min(errors[b][i]);
					if error >= best_error { continue 'pairs; } //error only grows
				}
				best_error = error;
				best = (a, b);
			}
		}

		let (bg, fg) = best;
		let id = (0..BITS)
			.filter(|&i| errors[fg][i] < errors[bg][i])
			.fold(0u8, |acc, i| acc | (1 << i));

		Self {
			id,
			bg: candidates.colors[bg],
			fg: candidates.colors[fg],
		}
	}

	/// same palette pairs as from_pixels_palette_pair, but also tries every count of fg dots,
	/// so the average color of the cell, mixed in linear light, gets close to the average of the pixels.
	/// in flat areas this dithers between two palette colors to show shades the palette lacks
	pub fn from_pixels_palette_dither(pixels: &[[Lab; WIDTH]; HEIGHT], formatter: &HybridFormatter) -> Self {
		/// scales the linear rgb distance of the cell average to about the size of a ciede2000 difference
		const MIX_WEIGHT: f32 = 100.0;
		/// pulls dots that are close to a tie into a dispersed order, so flat areas get an even pattern instead of streaks
		const ORDER_BIAS: f32 = 1.0;
		const DISPERSED_RANK: [f32; BITS] = [0.0, 4.0, 6.0, 2.0, 1.0, 5.0, 7.0, 3.0];

		let pixels: &[Lab; BITS] = unsafe { std::mem::transmute(pixels) };
		let candidates = PaletteCandidates::new(pixels, formatter);
		let errors = &candidates.errors;

		let to_linear = |color: Lab| {
			let color = LinSrgb::from_color(color);
			[color.red, color.green, color.blue]
		};
		let linears: [[f32; 3]; PaletteCandidates::MAX] = std::array::from_fn(|i| to_linear(candidates.labs[i]));
		let target = pixels
			.iter()
			.map(|p| to_linear(*p))
			.fold([0.0; 3], |acc, p| std::array::from_fn(|j| acc[j] + p[j] / BITS as f32));
		let mix_error = |a: usize, b: usize, k: usize| {
			let t = k as f32 / BITS as f32;
			let distance_squared: f32 = (0..3)
				.map(|j| linears[a][j] + (linears[b][j] - linears[a][j]) * t - target[j])
				.map(|d| d * d)
				.sum();
			MIX_WEIGHT * BITS as f32 * distance_squared.sqrt()
		};

		let mut best = (0, 0, 0u8);
		let mut best_score = f32::INFINITY;
		for a in 0..candidates.len {
			let error_a: f32 = errors[a].iter().sum();
			let score = error_a + mix_error(a, a, 0);
			if score < best_score {
				best_score = score;
				best = (a, a, 0);
			}

			for b in (a + 1)..candidates.len {
				//the k dots that lose the least by moving from a to b become fg
				let preference = |i: usize| errors[b][i] - errors[a][i];
				let mut order: [usize; BITS] = std::array::from_fn(|i| i);
				order.sort_unstable_by_key(|&i| OrderedFloat(preference(i) + ORDER_BIAS * (DISPERSED_RANK[i] - 3.5)));

				let mut error = error_a;
				let mut id = 0u8;
				for (k, &i) in order.iter().enumerate() {
					error += preference(i);
					id |= 1 << i;
					let score = error + mix_error(a, b, k + 1);
					if score < best_score {
						best_score = score;
						best = (a, b, id);
					}
				}
			}
		}

		let (bg, fg, id) = best;
		Self {
			id,
			bg: candidates.colors[bg],
			fg: candidates.colors[fg],
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
pub fn as_braille<T: Copy + Sync + Send, U: Send>(input: &Image<T>, from_pixels: impl Fn([[T; WIDTH]; HEIGHT]) -> U + Sync + Send) -> ParallelImageIterator<impl ParallelIterator<Item = U>> {
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
			.map(from_pixels),
	}
}
#[cfg(not(feature = "rayon"))]
pub fn as_braille<T: Copy + Sync + Send, U: Send>(input: &Image<T>, from_pixels: impl Fn([[T; WIDTH]; HEIGHT]) -> U + Sync + Send) -> ImageIterator<impl Iterator<Item = U>> {
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
			.map(from_pixels),
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

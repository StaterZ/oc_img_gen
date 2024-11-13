use std::borrow::Borrow;

use lodepng::Bitmap;
use num_traits::Zero;
use szu::{math::int_div_round, iter::{MultiZipArrayExt, MultiZipExt}};

use crate::oc_color::{RGB, RGB8};

pub const WIDTH: usize = 2;
pub const HEIGHT: usize = 4;
pub const BITS: usize = WIDTH * HEIGHT;

pub struct Braille<T> {
	pub id: u8,
	pub bg: T,
	pub fg: T,
}

impl<T> Braille<T> {
	pub fn char(&self) -> char {
		std::char::from_u32('⠀' as u32 + self.id as u32).unwrap() //unwrap is safe: we know the unicode range to be valid
	}

	pub fn raster(&self) -> [[&T; WIDTH]; HEIGHT] {
		std::array::from_fn::<_, { HEIGHT }, _>(move |y|
			std::array::from_fn::<_, { WIDTH }, _>(move |x| {
				let i = y * WIDTH + x;
				if (self.id >> i) & 1 == 0 { &self.bg } else { &self.fg }
			}))
	}
}

impl Braille<RGB8> {
	fn compute_bin_bleed(&self, pixels: &[impl Borrow<[RGB8; WIDTH]>; HEIGHT]) -> u32 {
		let mut output = 0;
		for i in 0..BITS {
			if (self.id >> i) & 1 == 0 {
				if self.id != 0xff {
					output += self.bg.perceptual_delta(pixels[i / WIDTH].borrow()[i % WIDTH])
				}
			} else {
				if self.id != 0x00 {
					output += self.fg.perceptual_delta(pixels[i / WIDTH].borrow()[i % WIDTH])
				}
			}
		}
		output
	}
	
	fn compute_cross_bin_sharpness(&self) -> u32 {
		if matches!(self.id, 0x00 | 0xff) { 0 } else { self.bg.perceptual_delta(self.fg) }
	}

	pub fn from_pixels(pixels: &[impl Borrow<[RGB8; WIDTH]>; HEIGHT]) -> Self {
		(0..(1 << (BITS - 1))).map(move |group| {
				let group = group as u8;

				let (bg, fg) = {
					let mut bg_sum = RGB::<u32>::zero();
					let mut fg_sum = RGB::<u32>::zero();
					for i in 0..BITS {
						let bin_sum = if (group >> i) & 1 == 0 {
							&mut bg_sum
						} else {
							&mut fg_sum
						};
						*bin_sum += pixels[i / WIDTH].borrow()[i % WIDTH].into();
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
		.max_by_key(|char| {
			let bin_bleed = Self::compute_bin_bleed(char, pixels);
			let cross_bin_sharpness = Self::compute_cross_bin_sharpness(char);
			cross_bin_sharpness as i32 - bin_bleed as i32
		}).unwrap() //unwrap is safe since iterator is Self::BITS long, that's always >0
	}
}

pub fn as_braille(input: &Bitmap<RGB8>) -> Bitmap<Braille<RGB8>> {
	let braille_pixel_clusters = input.buffer
		.chunks_exact(input.width) //make grid
		.array_chunks::<{ HEIGHT }>() //group rows by 4
		.map(|char_row| char_row
			.map(|row| row
				.array_chunks::<{ WIDTH }>()) //split rows in chunks of 2
			.multi_zip_array()); //convert the 4 chunked rows into a cluster row
	
	let output = braille_pixel_clusters
		.flat_map(|rows| rows
			.map(|cluster| Braille::from_pixels(&cluster)))
		.collect();

	Bitmap {
		buffer: output,
		width: input.width / WIDTH,
		height: input.height / HEIGHT,
	}
}

pub fn raster<T: Copy>(input: &Bitmap<Braille<T>>) -> Bitmap<T> {
	let output = input.buffer
		.chunks_exact(input.width)
		.flat_map(|row| row
			.into_iter()
			.map(|char| char
				.raster()
				.into_iter())
			.multi_zip()
			.flatten())
		.flatten()
		.copied()
		.collect();

	Bitmap {
		buffer: output,
		width: input.width * WIDTH,
		height: input.height * HEIGHT,
	}
}

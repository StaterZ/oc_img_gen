use itertools::Itertools;
use more_asserts::*;

use crate::math::{Rect, Size};
use super::oc_color::RGB8;

#[derive(Clone)]
pub struct Image<T> {
	size: Size<usize>,
	buffer: Vec<T>,
}

impl<T> Image<T> {
	pub fn new(size: Size<usize>, buffer: Vec<T>) -> Self {
 		debug_assert_eq!(size.x * size.y, buffer.len());
		Self {
			size,
			buffer,
		}
	}

	#[inline]
	pub fn size(&self) -> &Size<usize> {
		&self.size
	}

	#[inline]
	pub fn buffer(&self) -> &[T] {
		&self.buffer
	}

	pub fn map<B>(&self, f: impl FnMut(&T) -> B) -> Image<B> {
		let buffer = self.buffer
			.iter()
			.map(f)
			.collect();

		Image {
			size: self.size,
			buffer,
		}
	}
}

impl<T: Copy> Image<T> {
	pub fn crop(&self, rect: &Rect<usize>) -> Self {
		debug_assert_le!(rect.pos.x + rect.size.x, self.size.x);
		debug_assert_le!(rect.pos.y + rect.size.y, self.size.y);

		let buffer = self.buffer
			.chunks_exact(self.size.x)
			.skip(rect.pos.y)
			.take(rect.size.y)
			.flat_map(|row| &row[rect.pos.x..rect.pos.x+rect.size.x])
			.copied()
			.collect_vec();

		debug_assert_eq!(buffer.len(), rect.size.area());

		Self {
			size: rect.size,
			buffer,
		}
	}

	pub fn resize(&self, size: Size<usize>, fill: T) -> Self {
		let mut buffer = vec![fill; size.area()];
		for y in 0..self.size.y.min(size.y) {
			for x in 0..self.size.x.min(size.x) {
				let old_index = y * self.size.x + x;
				let new_index = y * size.x + x;
				buffer[new_index] = self.buffer[old_index];
			}
		}

		Self {
			size,
			buffer,
		}
	}
}

impl<'a> From<&'a ffmpeg_next::frame::Video> for Image<RGB8> {
	fn from(value: &'a ffmpeg_next::frame::Video) -> Self {
		let width = value.width() as usize;
		let height = value.height() as usize;
		
		Self {
			size: Size::new(width, height),
			buffer: value.data(0)
				.chunks_exact(value.stride(0))
				.flat_map(|row| row[..width * 3]
					.array_chunks::<3>()
					.map(|p| RGB8 { r: p[0], g: p[1], b: p[2] }))
				.collect(),
		}
	}
}

#[cfg(feature = "debug-mode")]
impl From<lodepng::Bitmap<lodepng::RGB<u8>>> for Image<RGB8> {
	fn from(value: lodepng::Bitmap<lodepng::RGB<u8>>) -> Self {
		debug_assert_eq!(value.width * value.height, value.buffer.len()); //Just to be sure
		Self {
			size: Size::new(value.width, value.height),
			buffer: value.buffer
				.into_iter()
				.map(|p| RGB8 { r: p.r, g: p.g, b: p.b })
				.collect(),
		}
	}
}
#[cfg(feature = "debug-mode")]
impl Into<lodepng::Bitmap<lodepng::RGB<u8>>> for Image<RGB8> {
	fn into(self) -> lodepng::Bitmap<lodepng::RGB<u8>> {
		lodepng::Bitmap {
			buffer: self.buffer
				.iter()
				.map(|p| lodepng::RGB { r: p.r, g: p.g, b: p.b })
				.collect(),
			width: self.size.x,
			height: self.size.y,
		}
	}
}

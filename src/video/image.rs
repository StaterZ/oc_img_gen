use std::ops::{Index, IndexMut};

use itertools::Itertools;
use all_asserts::*;
use num_traits::ConstZero;

use crate::math::*;
use super::{
	oc_color::RGB8,
	ImageIterator,
};

#[derive(Clone)]
pub struct Image<T> {
	size: Size<usize>,
	buffer: Vec<T>,
}

impl<T> Image<T> {
	pub fn with_buffer(size: Size<usize>, buffer: Vec<T>) -> Self {
		debug_assert_eq!(size.area(), buffer.len());
		Self {
			size,
			buffer,
		}
	}

	#[inline]
	pub fn size(&self) -> Size<usize> {
		self.size
	}

	#[inline]
	pub fn buffer(&self) -> &[T] {
		&self.buffer
	}
	
	#[inline]
	pub fn buffer_mut(&mut self) -> &mut [T] {
		&mut self.buffer
	}

	pub fn iter(&self) -> ImageIterator<impl Iterator<Item = &T>> {
		ImageIterator {
			size: self.size,
			iter: self.buffer.iter(),
		}
	}

	pub fn into_iter(self) -> ImageIterator<impl Iterator<Item = T>> {
		ImageIterator {
			size: self.size,
			iter: self.buffer.into_iter(),
		}
	}
}

impl<T: Copy> Image<T> {
	pub fn new(size: Size<usize>, value: T) -> Self {
		Self {
			size,
			buffer: vec![value; size.area()],
		}
	}

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

	pub fn resize(&self, size: Size<usize>, fill: T, anchor: Point<Frac<isize>>) -> Self {
		let delta = (size.cast::<isize>() - self.size.cast::<isize>()).cast::<Frac<isize>>();
		let offset = Point::new( //TODO: blegh!
			(anchor.x * delta.x).into_int_round(),
			(anchor.y * delta.y).into_int_round(),
		);
		
		let mut new = Self::new(size, fill);
		let rect = Rect::new(Point::ZERO, new.size.cast());
		for y in 0..self.size.y {
			for x in 0..self.size.x {
				let old_pos = Point::new(x, y);
				let new_pos = old_pos.cast::<isize>() + offset;
				if !rect.contains(new_pos) { continue; }

				new[new_pos.cast()] = self[old_pos];
			}
		}
		new
	}
}

impl<T> Index<Point<usize>> for Image<T> {
	type Output = T;

	fn index(&self, index: Point<usize>) -> &Self::Output {
		&self.buffer[index.y * self.size.x + index.x]
	}
}
impl<T> IndexMut<Point<usize>> for Image<T> {
	fn index_mut(&mut self, index: Point<usize>) -> &mut Self::Output {
		&mut self.buffer[index.y * self.size.x + index.x]
	}
}

impl<I: Iterator> From<ImageIterator<I>> for Image<I::Item> {
	fn from(value: ImageIterator<I>) -> Self {
		Self {
			size: value.size,
			buffer: value.iter.collect(),
		}
	}
}

impl<'a> From<&'a ffmpeg_next::frame::Video> for Image<RGB8> {
	fn from(value: &'a ffmpeg_next::frame::Video) -> Self {
		let width = value.width() as usize;
		let height = value.height() as usize;
		const POD_SIZE: usize = 3;
		
		Self {
			size: Size::new(width, height),
			buffer: value.data(0)
				.chunks_exact(value.stride(0))
				.flat_map(|row| row[..width * POD_SIZE]
					.into_iter()
					.array_chunks::<POD_SIZE>()
					.map(|p| RGB8 { r: *p[0], g: *p[1], b: *p[2] }))
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
impl From<Image<RGB8>> for lodepng::Bitmap<lodepng::RGB<u8>> {
	fn from(value: Image<RGB8>) -> lodepng::Bitmap<lodepng::RGB<u8>> {
		Self {
			buffer: value.buffer
				.iter()
				.map(|p| lodepng::RGB { r: p.r, g: p.g, b: p.b })
				.collect(),
			width: value.size.x,
			height: value.size.y,
		}
	}
}

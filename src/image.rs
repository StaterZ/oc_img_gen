use crate::{math::Size, oc_color::RGB8};

#[derive(Clone)]
pub struct Image<T> {
	size: Size<usize>,
	buffer: Vec<T>,
}

impl<T> Image<T> {
	pub fn new(size: Size<usize>, buffer: Vec<T>) -> Self {
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

impl From<video_rs::Frame> for Image<RGB8> {
	fn from(value: video_rs::Frame) -> Self {
		let buffer = value.view()
			.iter()
			.copied()
			.collect::<Vec<u8>>()
			.chunks_exact(3)
			.map(|chunk| RGB8 {
				r: chunk[0],
				g: chunk[1],
				b: chunk[2],
			})
			.collect();
		
		Self {
			size: Size::new(value.dim().0, value.dim().1),
			buffer,
		}
	}
}

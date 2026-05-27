use crate::math::*;
use super::Image;

pub struct ImageIterator<I: Iterator> {
	pub size: Size<usize>,
	pub iter: I,
}

impl<I: Iterator> ImageIterator<I> {
	#[inline]
	pub fn map<B>(self, f: impl FnMut(I::Item) -> B) -> ImageIterator<impl Iterator<Item = B>> {
		ImageIterator {
			size: self.size,
			iter: self.iter.map(f)
		}
	}

	#[inline]
    pub fn collect(self) -> Image<I::Item> {
        Image::with_buffer(self.size, self.iter.collect())
    }
}

#[cfg(feature = "rayon")]
pub use parallel::*;
#[cfg(feature = "rayon")]
mod parallel {
	use rayon::prelude::*;

	use crate::math::*;
	use super::Image;

	pub struct ParallelImageIterator<I: ParallelIterator> {
		pub size: Size<usize>,
		pub iter: I,
	}

	impl<I: ParallelIterator> ParallelImageIterator<I> {
		#[inline]
		pub fn map<B: Send>(self, f: impl Fn(I::Item) -> B + Sync + Send) -> ParallelImageIterator<impl ParallelIterator<Item = B>> {
			ParallelImageIterator {
				size: self.size,
				iter: self.iter.map(f)
			}
		}

		#[inline]
		pub fn collect(self) -> Image<I::Item> {
			Image::with_buffer(self.size, self.iter.collect())
		}
	}
}

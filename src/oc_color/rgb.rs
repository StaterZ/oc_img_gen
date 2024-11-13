use std::fmt::Display;
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};
use more_asserts::*;
use num_traits::{Signed, Zero};
use szu::math::AbsDiff;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RGB<T> {
	pub r: T,
	pub g: T,
	pub b: T,
}

impl<T: Signed> RGB<T> {
	pub fn abs(self) -> Self {
		Self {
			r: self.r.abs(),
			g: self.g.abs(),
			b: self.b.abs(),
		}
	}
}

impl<T: Zero> Zero for RGB<T> {
	fn zero() -> Self {
		Self {
			r: T::zero(),
			g: T::zero(),
			b: T::zero(),
		}
	}
	
	fn is_zero(&self) -> bool {
		self.r.is_zero() && self.g.is_zero() && self.b.is_zero()
	}
}

impl<T: AbsDiff> AbsDiff for RGB<T> {
	type Abs = RGB<T::Abs>;

	fn max_abs_diff() -> Self::Abs {
		Self::Abs {
			r: T::max_abs_diff(),
			g: T::max_abs_diff(),
			b: T::max_abs_diff(),
		}
	}

	fn abs_diff(self, other: Self) -> Self::Abs {
		Self::Abs {
			r: self.r.abs_diff(other.r),
			g: self.g.abs_diff(other.g),
			b: self.b.abs_diff(other.b),
		}
	}
}

pub type RGB8 = RGB<u8>;

impl RGB8 {
	const R_SHIFT: u32 = 8 * 2;
	const G_SHIFT: u32 = 8 * 1;
	const B_SHIFT: u32 = 8 * 0;

	pub fn new(value: u32) -> Self {
		debug_assert_lt!(value, 0xffffff);

		Self {
			r: (value >> Self::R_SHIFT) as u8,
			g: (value >> Self::G_SHIFT) as u8,
			b: (value >> Self::B_SHIFT) as u8,
		}
	}

	pub fn perceptual_delta(self, other: Self) -> u32 { //u32/i32 is big enough for entire range: log2((255*255) * 9999 * 3)
		let d = <RGB8 as Into<RGB<i32>>>::into(self) - other.into();
		//let d: RGB<u32> = self.abs_diff(other).into();
		let d2 = d * d;
		return (2126 * d2.r + 7152 * d2.g + 0722 * d2.b) as u32;
		//return (d2.r + d2.g + d2.b) as u32;
	}
}

impl Display for RGB8 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

macro_rules! rgb_into {
	($from:ty, $to:ty) => {
		impl Into<RGB<$to>> for RGB<$from> {
			fn into(self) -> RGB<$to> {
				RGB {
					r: self.r as $to,
					g: self.g as $to,
					b: self.b as $to,
				}
			}
		}
	};
}
rgb_into!(u8, u32);
rgb_into!(u8, i32);
rgb_into!(u32, u8);

impl<T: Add<Output = T>> Add for RGB<T> {
	type Output = Self;
	
	fn add(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r + rhs.r,
			g: self.g + rhs.g,
			b: self.b + rhs.b,
		}
	}
}

impl<T: AddAssign> AddAssign for RGB<T> {
	fn add_assign(&mut self, rhs: Self) {
		self.r += rhs.r;
		self.g += rhs.g;
		self.b += rhs.b;
	}
}

impl<T: Sub<Output = T>> Sub for RGB<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r - rhs.r,
			g: self.g - rhs.g,
			b: self.b - rhs.b,
		}
	}
}

impl<T: SubAssign> SubAssign for RGB<T> {
	fn sub_assign(&mut self, rhs: Self) {
		self.r -= rhs.r;
		self.g -= rhs.g;
		self.b -= rhs.b;
	}
}

impl<T: Mul<Output = T>> Mul for RGB<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r * rhs.r,
			g: self.g * rhs.g,
			b: self.b * rhs.b,
		}
	}
}

impl<T: MulAssign> MulAssign for RGB<T> {
	fn mul_assign(&mut self, rhs: Self) {
		self.r *= rhs.r;
		self.g *= rhs.g;
		self.b *= rhs.b;
	}
}

impl<T: Div<Output = T>> Div for RGB<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r / rhs.r,
			g: self.g / rhs.g,
			b: self.b / rhs.b,
		}
	}
}

impl<T: DivAssign> DivAssign for RGB<T> {
	fn div_assign(&mut self, rhs: Self) {
		self.r /= rhs.r;
		self.g /= rhs.g;
		self.b /= rhs.b;
	}
}

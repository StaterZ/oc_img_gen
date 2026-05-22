use std::{
	fmt::Display,
	iter::Sum,
	num::ParseIntError,
	ops::*,
	str::FromStr,
};
//use all_asserts::*;
use num_traits::{ConstZero, Signed, Zero};
use szu::math::AbsDiff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl<T: ConstZero> ConstZero for RGB<T> {
	const ZERO: Self = Self {
		r: T::ZERO,
		g: T::ZERO,
		b: T::ZERO,
	};
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
	pub const BLACK: RGB8 = RGB8::new(0x000000);
	pub const WHITE: RGB8 = RGB8::new(0xffffff);
	
	const R_SHIFT: u32 = 8 * 2;
	const G_SHIFT: u32 = 8 * 1;
	const B_SHIFT: u32 = 8 * 0;

	pub const fn new(value: u32) -> Self {
		//debug_assert_le!(value, 0xffffff);

		Self {
			r: (value >> Self::R_SHIFT) as u8,
			g: (value >> Self::G_SHIFT) as u8,
			b: (value >> Self::B_SHIFT) as u8,
		}
	}

	pub const fn value(&self) -> u32 {
		(self.r as u32) << Self::R_SHIFT | (self.g as u32) << Self::G_SHIFT | (self.b as u32) << Self::B_SHIFT
	}

	#[inline]
	pub fn perceptual_delta(self, other: Self) -> u32 { //u32/i32 is big enough for entire range: log2((255*255) * 9999 * 3)
		Into::<RGB<i32>>::into(self).perceptual_delta(other.into())
	}
}

impl RGB<i32> {
	#[inline]
	pub fn perceptual_delta(self, other: Self) -> u32 { //u32/i32 is big enough for entire range: log2((255*255) * 9999 * 3)
		let d = self - other;
		//let d: RGB<u32> = self.abs_diff(other).into();
		let d2 = d * d;
		(2126 * d2.r + 7152 * d2.g + 0722 * d2.b) as u32
		//(d2.r + d2.g + d2.b) as u32
	}
}

impl Display for RGB8 {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:02X}{:02X}{:02X}", self.r, self.g, self.b)
	}
}

#[derive(thiserror::Error, Debug)]
pub enum ParseColorError {
	#[error("Color must be 6 hex digits, {0} supplied")]
	BadCharacterCount(usize),
	#[error("{0}")]
	ParseIntError(#[from] ParseIntError),
}

impl FromStr for RGB8 {
	type Err = ParseColorError;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let hex = s.trim();
		if hex.len() != 6 {
			return Err(ParseColorError::BadCharacterCount(hex.len()));
		}
		let r = u8::from_str_radix(&hex[0..2], 16)?;
		let g = u8::from_str_radix(&hex[2..4], 16)?;
		let b = u8::from_str_radix(&hex[4..6], 16)?;
		Ok(RGB8 { r, g, b })
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

impl<T: Zero> Sum for RGB<T> {
	fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
		iter.fold(Self::zero(), |lhs, rhs| lhs + rhs)
	}
}

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

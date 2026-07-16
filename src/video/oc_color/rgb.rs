use std::{
	fmt::Display,
	iter::Sum,
	num::ParseIntError,
	ops::*,
	str::FromStr,
};
//use all_asserts::*;
use num::{Num, One};
use num_traits::{ConstOne, ConstZero, Signed, Zero};
use palette::{IntoColor, Lab, Srgb, color_difference::ImprovedCiede2000};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RGB<T: Num> {
	pub r: T,
	pub g: T,
	pub b: T,
}

#[derive(thiserror::Error)]
pub enum FromStrRadixErr<T: Num> {
	#[error("too few parts")]
	MissingPart,
	#[error("too many parts")]
	TooManyParts,
	#[error(transparent)]
	Inner(T::FromStrRadixErr),
}

impl<T: Num> Num for RGB<T> {
	type FromStrRadixErr = FromStrRadixErr<T>;

	fn from_str_radix(str: &str, radix: u32) -> Result<Self, Self::FromStrRadixErr> {
		let mut parts = str.split(',');
		let value = Self {
			r: T::from_str_radix(parts.next().ok_or(Self::FromStrRadixErr::MissingPart)?, radix).map_err(FromStrRadixErr::Inner)?,
			g: T::from_str_radix(parts.next().ok_or(Self::FromStrRadixErr::MissingPart)?, radix).map_err(FromStrRadixErr::Inner)?,
			b: T::from_str_radix(parts.next().ok_or(Self::FromStrRadixErr::MissingPart)?, radix).map_err(FromStrRadixErr::Inner)?,
		};
		if parts.next().is_some() {
			return Err(Self::FromStrRadixErr::TooManyParts);
		}
		Ok(value)
	}
}

impl<T: Num + Neg<Output: Num>> Neg for RGB<T> {
	type Output = RGB<<T as Neg>::Output>;

	fn neg(self) -> Self::Output {
		Self::Output {
			r: self.r.neg(),
			g: self.g.neg(),
			b: self.b.neg(),
		}
	}
}

impl<T: Num + Signed> Signed for RGB<T> {
	fn abs(&self) -> Self {
		Self {
			r: self.r.abs(),
			g: self.g.abs(),
			b: self.b.abs(),
		}
	}
	
	fn abs_sub(&self, other: &Self) -> Self {
		Self {
			r: self.r.abs_sub(&other.r),
			g: self.g.abs_sub(&other.g),
			b: self.b.abs_sub(&other.b),
		}
	}
	
	fn signum(&self) -> Self {
		Self {
			r: self.r.signum(),
			g: self.g.signum(),
			b: self.b.signum(),
		}
	}

	fn is_positive(&self) -> bool {
		self.r.is_positive() &&
		self.g.is_positive() &&
		self.b.is_positive()
	}
	
	fn is_negative(&self) -> bool {
		self.r.is_negative() &&
		self.g.is_negative() &&
		self.b.is_negative()
	}
}

impl<T: Num + Zero> Zero for RGB<T> {
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

impl<T: Num + ConstZero> ConstZero for RGB<T> {
	const ZERO: Self = Self {
		r: T::ZERO,
		g: T::ZERO,
		b: T::ZERO,
	};
}

impl<T: Num + One> One for RGB<T> {
	fn one() -> Self {
		Self {
			r: T::one(),
			g: T::one(),
			b: T::one(),
		}
	}
	
	fn is_one(&self) -> bool {
		self.r.is_one() && self.g.is_one() && self.b.is_one()
	}
}

impl<T: Num + ConstOne> ConstOne for RGB<T> {
	const ONE: Self = Self {
		r: T::ONE,
		g: T::ONE,
		b: T::ONE,
	};
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
		(self.r as u32) << Self::R_SHIFT |
		(self.g as u32) << Self::G_SHIFT |
		(self.b as u32) << Self::B_SHIFT
	}

	pub const PERCEPTUAL_DELTA_MAX: u32 = (40.686831874610654f32 * 1000.0).round() as u32;
	pub fn perceptual_delta(self, other: Self) -> u32 {
		let a: Lab = Srgb::from(self).into_format::<f32>().into_color();
		let b: Lab = Srgb::from(other).into_format::<f32>().into_color();

		let delta_e = a.improved_difference(b);

		(delta_e * 1000.0).round() as u32
	}
}

impl From<palette::rgb::Rgb<palette::encoding::Srgb, u8>> for RGB8 {
	fn from(value: palette::rgb::Rgb<palette::encoding::Srgb, u8>) -> Self {
		Self {
			r: value.red,
			g: value.green,
			b: value.blue,
		}
	}
}
impl From<RGB8> for palette::rgb::Rgb<palette::encoding::Srgb, u8> {
	fn from(value: RGB8) -> Self {
		Self::new(
			value.r,
			value.g,
			value.b,
		)
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

macro_rules! rgb_cast {
	($from:ty, $to:ty) => {
		impl From<RGB<$from>> for RGB<$to> {
			fn from(value: RGB<$from>) -> Self {
				Self {
					r: value.r as $to,
					g: value.g as $to,
					b: value.b as $to,
				}
			}
		}
	};
}
rgb_cast!(u8, u32);
rgb_cast!(u8, i32);
rgb_cast!(u32, u8);

impl<T: Num + Zero> Sum for RGB<T> {
	fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
		iter.fold(Self::zero(), |lhs, rhs| lhs + rhs)
	}
}

impl<T: Num + Add<Output = T>> Add for RGB<T> {
	type Output = Self;
	
	fn add(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r + rhs.r,
			g: self.g + rhs.g,
			b: self.b + rhs.b,
		}
	}
}

impl<T: Num + AddAssign> AddAssign for RGB<T> {
	fn add_assign(&mut self, rhs: Self) {
		self.r += rhs.r;
		self.g += rhs.g;
		self.b += rhs.b;
	}
}

impl<T: Num + Sub<Output = T>> Sub for RGB<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r - rhs.r,
			g: self.g - rhs.g,
			b: self.b - rhs.b,
		}
	}
}

impl<T: Num + SubAssign> SubAssign for RGB<T> {
	fn sub_assign(&mut self, rhs: Self) {
		self.r -= rhs.r;
		self.g -= rhs.g;
		self.b -= rhs.b;
	}
}

impl<T: Num + Mul<Output = T>> Mul for RGB<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r * rhs.r,
			g: self.g * rhs.g,
			b: self.b * rhs.b,
		}
	}
}

impl<T: Num + MulAssign> MulAssign for RGB<T> {
	fn mul_assign(&mut self, rhs: Self) {
		self.r *= rhs.r;
		self.g *= rhs.g;
		self.b *= rhs.b;
	}
}

impl<T: Num + Div<Output = T>> Div for RGB<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r / rhs.r,
			g: self.g / rhs.g,
			b: self.b / rhs.b,
		}
	}
}

impl<T: Num + DivAssign> DivAssign for RGB<T> {
	fn div_assign(&mut self, rhs: Self) {
		self.r /= rhs.r;
		self.g /= rhs.g;
		self.b /= rhs.b;
	}
}

impl<T: Num + Rem<Output = T>> Rem for RGB<T> {
	type Output = Self;

	fn rem(self, rhs: Self) -> Self::Output {
		Self {
			r: self.r % rhs.r,
			g: self.g % rhs.g,
			b: self.b % rhs.b,
		}
	}
}

impl<T: Num + RemAssign> RemAssign for RGB<T> {
	fn rem_assign(&mut self, rhs: Self) {
		self.r %= rhs.r;
		self.g %= rhs.g;
		self.b %= rhs.b;
	}
}

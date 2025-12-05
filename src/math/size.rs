use std::{fmt::Display, ops::{Add, Div, Mul, Sub}, str::FromStr};
use deku::{no_std_io, prelude::*};
use num::NumCast;
use num_traits::{ConstZero, PrimInt, Zero};

use super::{Frac, GCD, Point, Rect};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Size<T: PrimInt + GCD> {
	pub x: T,
	pub y: T,
}

impl<T: PrimInt + GCD> Size<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
	pub const fn one(v: T) -> Self {
		Self {
			x: v,
			y: v,
		}
	}
	
	pub fn ratio(&self) -> Frac<T> {
		Frac::new(self.x, self.y)
	}

	pub fn area(&self) -> T {
		self.x * self.y
	}

	pub fn cast<U: PrimInt + GCD>(self) -> Size::<U> {
		Size {
			x: NumCast::from(self.x).unwrap(),
			y: NumCast::from(self.y).unwrap(),
		}
	}
	pub fn try_cast<U: PrimInt + GCD>(self) -> Option<Size<U>> {
		Some(Size {
			x: NumCast::from(self.x)?,
			y: NumCast::from(self.y)?,
		})
	}
	
	pub fn contain(&self, content: Self) -> Self {
		if content.ratio() > self.ratio() {
			Self {
				x: self.x,
				y: (<T as Into<Frac<T>>>::into(self.x) / content.ratio()).into_int(),
			}
		} else {
			Self {
				x: (<T as Into<Frac<T>>>::into(self.y) * content.ratio()).into_int(),
				y: self.y,
			}
		}
	}

	pub fn center(self, content: Self) -> Rect<T> {
		Rect {
			pos: (self - content) / T::from(2).unwrap(),
			size: content,
		}
	}
}

impl<T: PrimInt + GCD + Display> Display for Size<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}x{}", self.x, self.y)
	}
}

impl<T: PrimInt + GCD + DekuWriter> DekuWriter for Size<T> {
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.x.to_writer(writer, ctx)?;
		self.y.to_writer(writer, ctx)?;
		Ok(())
	}
}

#[derive(thiserror::Error, Debug)]
pub enum SizeParseErr<E> {
	#[error("Size must be in WIDTHxHEIGHT format")] BadFormat,
	#[error("parse failed")] ParseError(#[from] E),
}

impl<T: PrimInt + GCD + FromStr> FromStr for Size<T> {
	type Err = SizeParseErr<<T as FromStr>::Err>;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let parts: Vec<&str> = s.split('x').collect();
		if parts.len() != 2 {
			return Err(SizeParseErr::BadFormat);
		}

		let x = parts[0].parse::<T>().map_err(SizeParseErr::ParseError)?;
		let y = parts[1].parse::<T>().map_err(SizeParseErr::ParseError)?;

		Ok(Size { x, y })
	}
}

impl<T: PrimInt + GCD + Zero> Zero for Size<T> {
	fn zero() -> Self {
		Self::new(T::zero(), T::zero())
	}
	
	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: PrimInt + GCD + ConstZero> ConstZero for Size<T> {
	const ZERO: Self = Self::new(T::ZERO, T::ZERO);
}

impl<T: PrimInt + GCD> Add for Size<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x + rhs.x,
			y: self.y + rhs.y,
		}
	}
}
impl<T: PrimInt + GCD> Add<T> for Size<T> {
	type Output = Self;

	fn add(self, rhs: T) -> Self::Output {
		Self {
			x: self.x + rhs,
			y: self.y + rhs,
		}
	}
}

impl<T: PrimInt + GCD> Sub for Size<T> {
	type Output = Point<T>;

	fn sub(self, rhs: Self) -> Self::Output {
		Self::Output {
			x: self.x - rhs.x,
			y: self.y - rhs.y,
		}
	}
}
impl<T: PrimInt + GCD> Sub<T> for Size<T> {
	type Output = Self;

	fn sub(self, rhs: T) -> Self::Output {
		Self {
			x: self.x - rhs,
			y: self.y - rhs,
		}
	}
}

impl<T: PrimInt + GCD> Mul for Size<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
		}
	}
}
impl<T: PrimInt + GCD> Mul<T> for Size<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		Self {
			x: self.x * rhs,
			y: self.y * rhs,
		}
	}
}

impl<T: PrimInt + GCD> Div for Size<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x / rhs.x,
			y: self.y / rhs.y,
		}
	}
}
impl<T: PrimInt + GCD> Div<T> for Size<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			x: self.x / rhs,
			y: self.y / rhs,
		}
	}
}

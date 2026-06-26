use std::{fmt::Display, ops::{Add, Div, Mul, Sub}, str::FromStr};
use deku::prelude::*;
use num::NumCast;
use num_traits::{ConstZero, ConstOne, Zero, One};
use szu::math::{GoodInt, GoodNum};

use crate::math::IntoIntRound;

use super::{Frac, Point, Rect};

pub trait SizeTrait<T: GoodNum> {
	type RatioOutput;

	fn ratio(&self) -> Self::RatioOutput;
	fn contain(&self, content: Self) -> Self;
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Size<T: GoodNum> {
	pub w: T,
	pub h: T,
}

impl<T: GoodNum> Size<T> {
	pub const fn new(w: T, h: T) -> Self {
		Self { w, h }
	}
	pub const fn one(v: T) -> Self {
		Self {
			w: v,
			h: v,
		}
	}

	pub fn area(&self) -> T {
		self.w * self.h
	}

	pub fn cast<U: GoodNum>(self) -> Size::<U> {
		Size {
			w: NumCast::from(self.w).unwrap(),
			h: NumCast::from(self.h).unwrap(),
		}
	}
	pub fn try_cast<U: GoodNum>(self) -> Option<Size<U>> {
		Some(Size {
			w: NumCast::from(self.w)?,
			h: NumCast::from(self.h)?,
		})
	}

	pub fn center(self, content: Self) -> Rect<T> {
		Rect {
			pos: (self - content) / T::from(2).unwrap(),
			size: content,
		}
	}
}

impl<T: GoodInt> SizeTrait<T> for Size<T> {
	type RatioOutput = Frac<T>;

	fn ratio(&self) -> Self::RatioOutput {
		Frac::new(self.w, self.h)
	}
	
	fn contain(&self, content: Self) -> Self {
		if content.ratio() > self.ratio() {
			Self {
				w: self.w,
				h: (<Frac::<T> as From<T>>::from(self.w) / content.ratio()).into_int_round(),
			}
		} else {
			Self {
				w: (<Frac::<T> as From<T>>::from(self.h) * content.ratio()).into_int_round(),
				h: self.h,
			}
		}
	}
}

// impl<T: GoodFloat> SizeTrait<T> for Size<T> {
// 	type RatioOutput = T;

// 	fn ratio(&self) -> Self::RatioOutput {
// 		self.x / self.y
// 	}
	
// 	fn contain(&self, content: Self) -> Self {
// 		if content.ratio() > self.ratio() {
// 			Self {
// 				x: self.x,
// 				y: self.x / content.ratio(),
// 			}
// 		} else {
// 			Self {
// 				x: self.y * content.ratio(),
// 				y: self.y,
// 			}
// 		}
// 	}
// }

// impl<T: GoodNum + Num> Num for Size<T> {
// 	type FromStrRadixErr = SizeParseErr<<T as Num>::FromStrRadixErr>;

// 	fn from_str_radix(str: &str, radix: u32) -> Result<Self, Self::FromStrRadixErr> {
// 		let parts: Vec<&str> = str.split('x').collect();
// 		if parts.len() != 2 {
// 			return Err(SizeParseErr::BadFormat);
// 		}

// 		Ok(Self {
// 			x: T::from_str_radix(parts[0], radix).map_err(SizeParseErr::ParseError)?,
// 			y: T::from_str_radix(parts[1], radix).map_err(SizeParseErr::ParseError)?
// 		})
// 	}
// }

// impl<T: GoodNum + Unsigned> Unsigned for Size<T> { }

impl<T: GoodNum + Display> Display for Size<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{}x{}", self.w, self.h)
	}
}

impl<Ctx: Copy, T: GoodNum + DekuWriter<Ctx>> DekuWriter<Ctx> for Size<T> {
	fn to_writer<W: deku::no_std_io::Write + deku::no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError> {
		self.w.to_writer(writer, ctx)?;
		self.h.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<'a, Ctx: Copy, T: GoodNum + DekuReader<'a, Ctx>> DekuReader<'a, Ctx> for Size<T> {
	fn from_reader_with_ctx<R: deku::no_std_io::Read + deku::no_std_io::Seek>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Self, DekuError> {
		Ok(Self {
			w: T::from_reader_with_ctx(reader, ctx)?,
			h: T::from_reader_with_ctx(reader, ctx)?,
		})
	}
}

#[derive(thiserror::Error, Debug)]
pub enum SizeParseErr<E> {
	#[error("Size must be in WIDTHxHEIGHT format")] BadFormat,
	#[error("parse failed")] ParseError(#[from] E),
}

impl<T: GoodNum + FromStr> FromStr for Size<T> {
	type Err = SizeParseErr<<T as FromStr>::Err>;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let parts: Vec<&str> = s.split('x').collect();
		if parts.len() != 2 {
			return Err(SizeParseErr::BadFormat);
		}

		Ok(Self {
			w: parts[0].parse::<T>().map_err(SizeParseErr::ParseError)?,
			h: parts[1].parse::<T>().map_err(SizeParseErr::ParseError)?
		})
	}
}

impl<T: GoodNum + Zero> Zero for Size<T> {
	fn zero() -> Self {
		Self::one(T::zero())
	}
	
	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: GoodNum + ConstZero> ConstZero for Size<T> {
	const ZERO: Self = Self::one(T::ZERO);
}

impl<T: GoodNum + One> One for Size<T> {
	fn one() -> Self {
		Self::one(T::one())
	}
}

impl<T: GoodNum + ConstOne> ConstOne for Size<T> {
	const ONE: Self = Self::one(T::ONE);
}

impl<T: GoodNum> Add for Size<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			w: self.w + rhs.w,
			h: self.h + rhs.h,
		}
	}
}
impl<T: GoodNum> Add<T> for Size<T> {
	type Output = Self;

	fn add(self, rhs: T) -> Self::Output {
		Self {
			w: self.w + rhs,
			h: self.h + rhs,
		}
	}
}

impl<T: GoodNum> Sub for Size<T> {
	type Output = Point<T>;

	fn sub(self, rhs: Self) -> Self::Output {
		Self::Output {
			x: self.w - rhs.w,
			y: self.h - rhs.h,
		}
	}
}
impl<T: GoodNum> Sub<T> for Size<T> {
	type Output = Self;

	fn sub(self, rhs: T) -> Self::Output {
		Self {
			w: self.w - rhs,
			h: self.h - rhs,
		}
	}
}

impl<T: GoodNum> Mul for Size<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			w: self.w * rhs.w,
			h: self.h * rhs.h,
		}
	}
}
impl<T: GoodNum> Mul<T> for Size<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		Self {
			w: self.w * rhs,
			h: self.h * rhs,
		}
	}
}

impl<T: GoodNum> Div for Size<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			w: self.w / rhs.w,
			h: self.h / rhs.h,
		}
	}
}
impl<T: GoodNum> Div<T> for Size<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			w: self.w / rhs,
			h: self.h / rhs,
		}
	}
}

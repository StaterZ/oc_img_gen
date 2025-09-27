use std::{fmt::Display, ops::{Add, Div, Mul, Sub}};

use deku::{no_std_io, prelude::*};
use num_traits::PrimInt;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Frac<T: PrimInt> {
	pub numerator: T,
	pub denominator: T,
}

impl<T: PrimInt> Frac<T> {
	pub const fn new(numerator: T, denominator: T) -> Self {
		Self { numerator, denominator }
	}

	pub fn into_int(self) -> T {
		let two = T::one() + T::one();
		(self.numerator + (self.denominator / two)) / self.denominator
	}

	pub fn cast<U: PrimInt + From<T>>(self) -> Frac::<U> {
		Frac {
			numerator: self.numerator.into(),
			denominator: self.denominator.into(),
		}
	}
	pub fn try_cast<U: PrimInt + TryFrom<T>>(self) -> Result<Frac::<U>, U::Error> {
		Ok(Frac {
			numerator: self.numerator.try_into()?,
			denominator: self.denominator.try_into()?,
		})
	}
}

impl<T: PrimInt + Display> Display for Frac<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.numerator, self.denominator)
	}
}

impl<T: PrimInt + DekuWriter> DekuWriter for Frac<T> {
	#[doc = " Write type to bytes"]
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.numerator.to_writer(writer, ctx)?;
		self.denominator.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<T: PrimInt> From<T> for Frac<T> {
	fn from(value: T) -> Self {
		Self {
			numerator: value,
			denominator: T::one(),
		}
	}
}

impl From<ffmpeg_next::Rational> for Frac<i32> {
	fn from(value: ffmpeg_next::Rational) -> Self {
		Self {
			numerator: value.numerator(),
			denominator: value.denominator(),
		}
	}
}

impl<T: PrimInt> PartialOrd for Frac<T> {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		(self.numerator * other.denominator).partial_cmp(&(other.numerator * self.denominator))
	}
}

impl<T: PrimInt> Add for Frac<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator + rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}
	}
}

impl<T: PrimInt> Sub for Frac<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator - rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}
	}
}

impl<T: PrimInt> Mul for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.numerator,
			denominator: self.denominator * rhs.denominator,
		}
	}
}

impl<T: PrimInt> Mul<T> for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		Self {
			numerator: self.numerator * rhs,
			denominator: self.denominator,
		}
	}
}

impl<T: PrimInt> Div for Frac<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator,
			denominator: self.denominator * rhs.numerator,
		}
	}
}

impl<T: PrimInt> Div<T> for Frac<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			numerator: self.numerator,
			denominator: self.denominator * rhs,
		}
	}
}

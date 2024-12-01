use std::{fmt::Display, ops::{Mul, Div}};

use deku::{no_std_io, prelude::*};
use num_traits::PrimInt;

use super::Size;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Point<T: PrimInt> {
	pub x: T,
	pub y: T,
}

impl<T: PrimInt> Point<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
	
	pub fn map<U: PrimInt>(&self, f: impl Fn(T) -> U) -> Point<U> {
		Point::<U> {
			x: f(self.x),
			y: f(self.y),
		}
	}
}

impl<T: PrimInt + Display> Display for Point<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

impl<T: PrimInt + DekuWriter> DekuWriter for Point<T> {
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.x.to_writer(writer, ctx)?;
		self.y.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<T: PrimInt> Mul<Size<T>> for Point<T> {
	type Output = Self;

	fn mul(self, rhs: Size<T>) -> Self::Output {
		Self {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
		}
	}
}

impl<T: PrimInt> Div<T> for Point<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			x: self.x / rhs,
			y: self.y / rhs,
		}
	}
}

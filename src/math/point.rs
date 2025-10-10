use std::{fmt::Display, ops::{Div, Mul}, str::FromStr};

use deku::{no_std_io, prelude::*};
use num_traits::PrimInt;

use super::{Size, GCD};

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
		Point {
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

impl<T: PrimInt + FromStr> FromStr for Point<T>
where
	<T as FromStr>::Err: std::fmt::Debug,
{
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let parts: Vec<&str> = s.split(',').collect();
		if parts.len() != 2 {
			return Err("Point must be in X,Y format".into());
		}
		
		let x = parts[0].parse::<T>().map_err(|e| format!("{:?}", e))?;
		let y = parts[1].parse::<T>().map_err(|e| format!("{:?}", e))?;
		Ok(Point { x, y })
	}
}

impl<T: PrimInt + GCD> Mul<Size<T>> for Point<T> {
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

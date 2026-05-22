use std::{fmt::Display, ops::*, str::FromStr};
use deku::{no_std_io, prelude::*};
use num::NumCast;
use num_traits::{ConstZero, Zero};

use super::{Size, GoodInt};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Point<T: GoodInt> {
	pub x: T,
	pub y: T,
}

impl<T: GoodInt> Point<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
	pub const fn one(v: T) -> Self {
		Self {
			x: v,
			y: v,
		}
	}
	
	pub fn cast<U: GoodInt>(self) -> Point::<U> {
		Point {
			x: NumCast::from(self.x).unwrap(),
			y: NumCast::from(self.y).unwrap(),
		}
	}
	pub fn try_cast<U: GoodInt>(self) -> Option<Point<U>> {
		Some(Point {
			x: NumCast::from(self.x)?,
			y: NumCast::from(self.y)?,
		})
	}
	
	pub fn map<U: GoodInt>(&self, f: impl Fn(T) -> U) -> Point<U> {
		Point {
			x: f(self.x),
			y: f(self.y),
		}
	}
}

impl<T: GoodInt + Display> Display for Point<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

impl<Ctx: Copy, T: GoodInt + DekuWriter<Ctx>> DekuWriter<Ctx> for Point<T> {
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError> {
		self.x.to_writer(writer, ctx)?;
		self.y.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<'a, Ctx: Copy, T: GoodInt + DekuReader<'a, Ctx>> DekuReader<'a, Ctx> for Point<T> {
	fn from_reader_with_ctx<R: no_std_io::Read + no_std_io::Seek>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Self, DekuError> {
		Ok(Self {
			x: T::from_reader_with_ctx(reader, ctx)?,
			y: T::from_reader_with_ctx(reader, ctx)?,
		})
	}
}

impl<T: GoodInt + FromStr> FromStr for Point<T>
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

impl<T: GoodInt + Zero> Zero for Point<T> {
	fn zero() -> Self {
		Self::one(T::zero())
	}
	
	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: GoodInt + ConstZero> ConstZero for Point<T> {
	const ZERO: Self = Self::one(T::ZERO);
}

impl<T: GoodInt> Add for Point<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x + rhs.x,
			y: self.y + rhs.y,
		}
	}
}

impl<T: GoodInt> Sub for Point<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x - rhs.x,
			y: self.y - rhs.y,
		}
	}
}

impl<T: GoodInt> Mul<Size<T>> for Point<T> {
	type Output = Self;

	fn mul(self, rhs: Size<T>) -> Self::Output {
		Self {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
		}
	}
}

impl<T: GoodInt> Div<T> for Point<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			x: self.x / rhs,
			y: self.y / rhs,
		}
	}
}

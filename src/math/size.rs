use std::{fmt::Display, ops::{Add, Div, Mul, Sub}, str::FromStr};

use deku::{no_std_io, prelude::*};
use num_traits::{ConstZero, PrimInt, Zero};

use super::{Frac, Point, Rect};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Size<T: PrimInt> {
	pub x: T,
	pub y: T,
}

impl<T: PrimInt> Size<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
	
	pub fn ratio(&self) -> Frac<T> {
		Frac::new(self.x, self.y)
	}

	pub fn area(&self) -> T {
		self.x * self.y
	}

	pub fn cast<U: PrimInt + From<T>>(self) -> Size::<U> {
		Size {
			x: self.x.into(),
			y: self.y.into(),
		}
	}
	pub fn try_cast<U: PrimInt + TryFrom<T>>(self) -> Result<Size::<U>, U::Error> {
		Ok(Size {
			x: self.x.try_into()?,
			y: self.y.try_into()?,
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

impl<T: PrimInt + Display> Display for Size<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

impl<T: PrimInt + DekuWriter> DekuWriter for Size<T> {
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.x.to_writer(writer, ctx)?;
		self.y.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<T: PrimInt + FromStr> FromStr for Size<T>
where
	<T as FromStr>::Err: std::fmt::Debug,
{
	type Err = String;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		let parts: Vec<&str> = s.split('x').collect();
		if parts.len() != 2 {
			return Err("Size must be in WIDTHxHEIGHT format".into());
		}
		
		let x = parts[0].parse::<T>().map_err(|e| format!("{:?}", e))?;
		let y = parts[1].parse::<T>().map_err(|e| format!("{:?}", e))?;
		Ok(Size { x, y })
	}
}

impl<T: PrimInt + Zero> Zero for Size<T> {
	fn zero() -> Self {
		Self::new(T::zero(), T::zero())
	}
	
	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: PrimInt + ConstZero> ConstZero for Size<T> {
	const ZERO: Self = Self::new(T::ZERO, T::ZERO);
}

impl<T: PrimInt> Add for Size<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x + rhs.x,
			y: self.y + rhs.y,
		}
	}
}
impl<T: PrimInt> Add<T> for Size<T> {
	type Output = Self;

	fn add(self, rhs: T) -> Self::Output {
		Self {
			x: self.x + rhs,
			y: self.y + rhs,
		}
	}
}

impl<T: PrimInt> Sub for Size<T> {
	type Output = Point<T>;

	fn sub(self, rhs: Self) -> Self::Output {
		Self::Output {
			x: self.x - rhs.x,
			y: self.y - rhs.y,
		}
	}
}
impl<T: PrimInt> Sub<T> for Size<T> {
	type Output = Self;

	fn sub(self, rhs: T) -> Self::Output {
		Self {
			x: self.x - rhs,
			y: self.y - rhs,
		}
	}
}

impl<T: PrimInt> Mul for Size<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
		}
	}
}
impl<T: PrimInt> Mul<T> for Size<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		Self {
			x: self.x * rhs,
			y: self.y * rhs,
		}
	}
}

impl<T: PrimInt> Div for Size<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x / rhs.x,
			y: self.y / rhs.y,
		}
	}
}
impl<T: PrimInt> Div<T> for Size<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		Self {
			x: self.x / rhs,
			y: self.y / rhs,
		}
	}
}

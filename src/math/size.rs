use std::{fmt::Display, ops::{Div, Mul}};

use deku::{no_std_io, prelude::*};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Size<T> {
	pub x: T,
	pub y: T,
}

impl<T> Size<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}

	pub fn cast<U: From<T>>(self) -> Size::<U> {
		Size::<U> {
			x: self.x.into(),
			y: self.y.into(),
		}
	}
	pub fn try_cast<U: TryFrom<T>>(self) -> Result<Size::<U>, U::Error> {
		Ok(Size::<U> {
			x: self.x.try_into()?,
			y: self.y.try_into()?,
		})
	}
}

impl<T: Display> Display for Size<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

impl<T: DekuWriter> DekuWriter for Size<T> {
	#[doc = " Write type to bytes"]
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.x.to_writer(writer, ctx)?;
		self.y.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<T: Mul<Output = T>> Mul for Size<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x * rhs.x,
			y: self.y * rhs.y,
		}
	}
}

impl<T: Div<Output = T>> Div for Size<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			x: self.x / rhs.x,
			y: self.y / rhs.y,
		}
	}
}

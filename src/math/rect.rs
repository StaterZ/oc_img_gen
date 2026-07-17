use std::ops::{Div, DivAssign, Mul, MulAssign};

use szu::math::GoodNum;

use super::{Point, Size};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Rect<T: GoodNum> {
	pub pos: Point<T>,
	pub size: Size<T>,
}

impl<T: GoodNum> Rect<T> {
	pub fn new(pos: Point<T>, size: Size<T>) -> Self {
		Self {
			pos,
			size,
		}
	}

	pub fn contains(&self, point: Point<T>) -> bool {
		(self.pos.x..self.pos.x + self.size.w).contains(&point.x) &&
		(self.pos.y..self.pos.y + self.size.h).contains(&point.y)
	}
}

impl<T: GoodNum> Mul<Size<T>> for Rect<T> {
	type Output = Self;

	fn mul(self, rhs: Size<T>) -> Self::Output {
		Rect {
			pos: self.pos * rhs,
			size: self.size * rhs,
		}
	}
}

impl<T: GoodNum> MulAssign<Size<T>> for Rect<T> {
	fn mul_assign(&mut self, rhs: Size<T>) {
		*self = *self * rhs;
	}
}

impl<T: GoodNum> Div<Size<T>> for Rect<T> {
	type Output = Self;

	fn div(self, rhs: Size<T>) -> Self::Output {
		Rect {
			pos: self.pos / rhs,
			size: self.size / rhs,
		}
	}
}

impl<T: GoodNum> DivAssign<Size<T>> for Rect<T> {
	fn div_assign(&mut self, rhs: Size<T>) {
		*self = *self / rhs;
	}
}

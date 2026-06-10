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

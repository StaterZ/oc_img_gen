use szu::math::GoodNum;

use super::{Point, Size};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Rect<T: GoodNum> {
	pub pos: Point<T>,
	pub size: Size<T>,
}

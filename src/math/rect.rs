use super::{Point, Size, GoodInt};

pub struct Rect<T: GoodInt> {
	pub pos: Point<T>,
	pub size: Size<T>,
}

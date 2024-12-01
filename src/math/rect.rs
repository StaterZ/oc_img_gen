use num_traits::PrimInt;

use super::{Point, Size};

pub struct Rect<T: PrimInt> {
	pub pos: Point<T>,
	pub size: Size<T>,
}

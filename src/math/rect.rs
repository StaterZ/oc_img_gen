use num_traits::PrimInt;

use super::{Point, Size, GCD};

pub struct Rect<T: PrimInt + GCD> {
	pub pos: Point<T>,
	pub size: Size<T>,
}

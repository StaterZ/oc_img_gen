use std::fmt::Display;

use deku::prelude::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy, DekuWrite)]
pub struct Size<T: DekuWriter> {
	pub x: T,
	pub y: T,
}

impl<T: DekuWriter> Size<T> {
	pub fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
}

impl<T: DekuWriter + Display> Display for Size<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

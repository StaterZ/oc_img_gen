use std::fmt::Display;

use deku::prelude::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy, DekuWrite)]
pub struct Point<T: DekuWriter> {
	pub x: T,
	pub y: T,
}

impl<T: DekuWriter + Copy> Point<T> {
	pub const fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
	
	pub fn map<U: DekuWriter>(&self, f: impl Fn(T) -> U) -> Point<U> {
		Point::<U> {
			x: f(self.x),
			y: f(self.y),
		}
	}
}

impl<T: DekuWriter + Display> Display for Point<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

use std::fmt::Display;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Point {
	pub x: usize,
	pub y: usize,
}

impl Display for Point {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{})", self.x, self.y)
	}
}

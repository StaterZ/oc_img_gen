use serde::Serialize;

#[derive(Debug, PartialEq, Eq, Clone, Copy, Serialize)]
pub struct Size<T> {
	pub x: T,
	pub y: T,
}

impl<T> Size<T> {
	pub fn new(x: T, y: T) -> Self {
		Self { x, y }
	}
}

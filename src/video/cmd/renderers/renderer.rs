use crate::math::{Point, Size};
use super::super::super::oc_color::PackedColor;

pub trait Renderer {
	fn get_resolution(&self) -> Size<usize>;
	fn set_resolution(&mut self, value: Size<usize>);

	fn get_background(&self) -> PackedColor;
	fn set_background(&mut self, value: PackedColor);
	
	fn get_foreground(&self) -> PackedColor;
	fn set_foreground(&mut self, value: PackedColor);

	fn set(&mut self, pos: &Point<usize>, value: &str);
}

use crate::{oc_color::PackedColor, math::Point};

pub trait Renderer {
	fn get_background(&self) -> PackedColor;
	fn set_background(&mut self, value: PackedColor);
	
	fn get_foreground(&self) -> PackedColor;
	fn set_foreground(&mut self, value: PackedColor);

	fn set(&mut self, pos: &Point, value: &str);
}

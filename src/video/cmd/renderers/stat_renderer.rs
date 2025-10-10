use crate::{math::{Frac, Point, Size}, video::cmd::{renderers::{BasicRenderer, RenderState}, Machine}};
use super::{
	Renderer,
	super::super::oc_color::PackedColor,
};

pub struct StatRenderer<T> {
	renderer: T,
	num_resolution_commands: usize,
	num_color_commands: usize,
	num_set_commands: usize,
}

impl<T> StatRenderer<T> {
	pub fn new(renderer: T) -> Self {
		Self {
			renderer,
			num_resolution_commands: 0,
			num_color_commands: 0,
			num_set_commands: 0,
		}
	}

	pub fn into_inner(self) -> T {
		self.renderer
	}

	pub fn get_cost(&self, machine: &Machine) -> Frac<usize> {
		machine.color_cost * self.num_color_commands + machine.set_cost * self.num_set_commands
	}
}

impl<T: Renderer> Renderer for StatRenderer<T> {
	fn get_resolution(&self) -> Size<usize> {
		self.renderer.get_resolution()
	}

	fn set_resolution(&mut self, value: Size<usize>) {
		self.renderer.set_resolution(value);
		self.num_resolution_commands += 1;
	}

	fn get_background(&self) -> PackedColor {
		self.renderer.get_background()
	}

	fn set_background(&mut self, value: PackedColor) {
		self.renderer.set_background(value);
		self.num_color_commands += 1;
	}

	fn get_foreground(&self) -> PackedColor {
		self.renderer.get_foreground()
	}
	
	fn set_foreground(&mut self, value: PackedColor) {
		self.renderer.set_foreground(value);
		self.num_color_commands += 1;
	}
	
	fn set(&mut self, pos: &Point<usize>, value: &str) {
		self.renderer.set(pos, value);
		self.num_set_commands += 1;
	}
}

impl<T: BasicRenderer> BasicRenderer for StatRenderer<T> {
	fn set_resolution(&mut self, value: Size<usize>) {
		self.renderer.set_resolution(value);
		self.num_resolution_commands += 1;
	}

	fn set_background(&mut self, value: PackedColor) {
		self.renderer.set_background(value);
		self.num_color_commands += 1;
	}

	fn set_foreground(&mut self, value: PackedColor) {
		self.renderer.set_foreground(value);
		self.num_color_commands += 1;
	}
	
	fn set(&mut self, state: &RenderState, pos: &Point<usize>, value: &str) {
		self.renderer.set(state, pos, value);
		self.num_set_commands += 1;
	}
}

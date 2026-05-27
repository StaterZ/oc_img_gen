use crate::{
	math::{Frac, Point, Size},
	video::cmd::{machine::Machine, renderers::{BasicRenderer, RenderState}}
};
use super::{
	Renderer,
	super::super::oc_color::PackedColor,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
	pub num_resolution_commands: usize,
	pub num_color_commands: usize,
	pub num_set_commands: usize,
	pub num_set_pixels: usize,
	pub num_bitblt_pixels: usize,
}

impl Stats {
	pub fn new() -> Self {
		Self {
			num_resolution_commands: 0,
			num_color_commands: 0,
			num_set_commands: 0,
			num_set_pixels: 0,
			num_bitblt_pixels: 0,
		}
	}

	pub fn get_set_resolution_cost(&self, machine: &Machine) -> Frac<usize> {
		machine.set_resolution_cost * self.num_resolution_commands
	}

	pub fn get_set_color_cost(&self, machine: &Machine) -> Frac<usize> {
		machine.set_color_cost * self.num_color_commands
	}

	pub fn get_set_cost(&self, machine: &Machine) -> Frac<usize> {
		machine.set_cost * self.num_set_commands
	}

	pub fn get_bitblt_cost(&self, machine: &Machine) -> Frac<usize> {
		machine.bitblt_cost / machine.max_screen_size.area() * self.num_bitblt_pixels
	}

	pub fn get_cost(&self, machine: &Machine) -> Frac<usize> {
		self.get_set_resolution_cost(&machine) +
		self.get_set_color_cost(&machine) +
		self.get_set_cost(&machine) +
		self.get_bitblt_cost(&machine)
	}
}

pub struct StatRenderer<T> {
	renderer: T,
	stats: Stats,
}

impl<T> StatRenderer<T> {
	pub fn new(renderer: T) -> Self {
		Self {
			renderer,
			stats: Stats::new(),
		}
	}
	
	pub fn into_inner(self) -> T {
		self.renderer
	}
	
	pub fn get_stats(&self) -> Stats {
		self.stats
	}
}

impl<T: Renderer> Renderer for StatRenderer<T> {
	fn get_resolution(&self) -> Size<usize> {
		self.renderer.get_resolution()
	}

	fn set_resolution(&mut self, value: Size<usize>) {
		self.renderer.set_resolution(value);
		self.stats.num_resolution_commands += 1;
	}

	fn get_background(&self) -> PackedColor {
		self.renderer.get_background()
	}

	fn set_background(&mut self, value: PackedColor) {
		self.renderer.set_background(value);
		self.stats.num_color_commands += 1;
	}

	fn get_foreground(&self) -> PackedColor {
		self.renderer.get_foreground()
	}
	
	fn set_foreground(&mut self, value: PackedColor) {
		self.renderer.set_foreground(value);
		self.stats.num_color_commands += 1;
	}
	
	fn set(&mut self, pos: &Point<usize>, value: &str) {
		self.renderer.set(pos, value);
		self.stats.num_set_commands += 1;
		self.stats.num_set_pixels += value.chars().count();
	}
}

impl<T: BasicRenderer> BasicRenderer for StatRenderer<T> {
	fn set_resolution(&mut self, value: Size<usize>) {
		self.renderer.set_resolution(value);
		self.stats.num_resolution_commands += 1;
	}

	fn set_background(&mut self, value: PackedColor) {
		self.renderer.set_background(value);
		self.stats.num_color_commands += 1;
	}

	fn set_foreground(&mut self, value: PackedColor) {
		self.renderer.set_foreground(value);
		self.stats.num_color_commands += 1;
	}
	
	fn set(&mut self, state: &RenderState, pos: &Point<usize>, value: &str) {
		self.renderer.set(state, pos, value);
		self.stats.num_set_commands += 1;
		self.stats.num_set_pixels += value.chars().count();
	}
}

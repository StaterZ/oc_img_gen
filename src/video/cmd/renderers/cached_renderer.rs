use crate::math::*;
use super::{
	Renderer,
	super::super::oc_color::PackedColor,
};

pub struct RenderState {
	pub resolution: Option<Size<usize>>,
	pub background: Option<PackedColor>,
	pub foreground: Option<PackedColor>,
}

pub struct CachedRenderer<T: BasicRenderer> {
	renderer: T,
	state: RenderState,
}

pub trait BasicRenderer {
	fn set_resolution(&mut self, value: Size<usize>);
	fn set_background(&mut self, value: PackedColor);
	fn set_foreground(&mut self, value: PackedColor);
	fn set(&mut self, state: &RenderState, pos: &Point<usize>, value: &str);
}

impl<T: BasicRenderer> CachedRenderer<T> {
	pub fn new(renderer: T) -> Self {
		Self {
			renderer,
			state: RenderState {
				resolution: None,
				background: None,
				foreground: None,
			},
		}
	}

	pub fn into_inner(self) -> T {
		self.renderer
	}
}

impl<T: BasicRenderer> Renderer for CachedRenderer<T> {
	fn get_resolution(&self) -> Size<usize> {
		debug_assert_ne!(self.state.resolution, None);
		self.state.resolution.unwrap()
	}
	
	fn set_resolution(&mut self, value: Size<usize>) {
		if self.state.resolution == Some(value) { return; }

		self.renderer.set_resolution(value);
		self.state.resolution = Some(value);
	}

	fn get_background(&self) -> PackedColor {
		debug_assert_ne!(self.state.background, None);
		self.state.background.unwrap()
	}

	fn set_background(&mut self, value: PackedColor) {
		if self.state.background == Some(value) { return; }

		self.renderer.set_background(value);
		self.state.background = Some(value);
	}

	fn get_foreground(&self) -> PackedColor { 
		debug_assert_ne!(self.state.foreground, None);
		self.state.foreground.unwrap()
	}

	fn set_foreground(&mut self, value: PackedColor) {
		if self.state.foreground == Some(value) { return; }

		self.renderer.set_foreground(value);
		self.state.foreground = Some(value);
	}
	
	fn set(&mut self, pos: &Point<usize>, value: &str) {
		self.renderer.set(&self.state, pos, value);
	}
}

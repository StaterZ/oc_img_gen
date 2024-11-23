use crate::{math::{Point, Size}, oc_color::PackedColor};

use super::Renderer;

pub struct RenderState {
	pub resolution: Option<Size<usize>>,
	pub background: Option<PackedColor>,
	pub foreground: Option<PackedColor>,
}

pub struct CachedRenderer<T: BasicRenderer> {
	renderer: T,
	render_state: RenderState,
}

pub trait BasicRenderer {
	fn set_resolution(&mut self, _state: &RenderState, value: Size<usize>);
	fn set_background(&mut self, state: &RenderState, value: PackedColor);
	fn set_foreground(&mut self, state: &RenderState, value: PackedColor);
	fn set(&mut self, state: &RenderState, pos: &Point, value: &str);
}

impl<T: BasicRenderer> CachedRenderer<T> {
	pub fn new(renderer: T) -> Self {
		Self {
			renderer,
			render_state: RenderState {
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
		debug_assert_ne!(self.render_state.resolution, None);
		self.render_state.resolution.unwrap()
	}

	fn set_resolution(&mut self, value: Size<usize>) {
		if self.render_state.resolution == Some(value) { return; }

		self.renderer.set_resolution(&self.render_state, value);
		self.render_state.resolution = Some(value);
	}

	fn get_background(&self) -> PackedColor {
		debug_assert_ne!(self.render_state.background, None);
		self.render_state.background.unwrap()
	}

	fn set_background(&mut self, value: PackedColor) {
		if self.render_state.background == Some(value) { return; }

		self.renderer.set_background(&self.render_state, value);
		self.render_state.background = Some(value);
	}

	fn get_foreground(&self) -> PackedColor { 
		debug_assert_ne!(self.render_state.foreground, None);
		self.render_state.foreground.unwrap()
	}

	fn set_foreground(&mut self, value: PackedColor) {
		if self.render_state.foreground == Some(value) { return; }

		self.renderer.set_foreground(&self.render_state, value);
		self.render_state.foreground = Some(value);
	}
	
	fn set(&mut self, pos: &Point, value: &str) {
		self.renderer.set(&self.render_state, pos, value);
	}
}

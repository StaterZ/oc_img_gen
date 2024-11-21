use crate::{hybrid_formatter::StaticColor, oc_color::{PackedColor, PaletteOr, RGB8}, math::Point};

use super::Renderer;

pub struct RenderState {
	pub background: PackedColor,
	pub foreground: PackedColor,
}

pub struct CachedRenderer<T: BasicRenderer> {
	renderer: T,
	render_state: RenderState,
}

pub trait BasicRenderer {
	fn set_background(&mut self, state: &RenderState, prev_value: PackedColor);
	fn set_foreground(&mut self, state: &RenderState, prev_value: PackedColor);
	fn set(&mut self, state: &RenderState, pos: &Point, value: &str);
}

impl<T: BasicRenderer> CachedRenderer<T> {
	pub fn new(renderer: T) -> Self {
		Self {
			renderer,
			render_state: RenderState {
				background: PackedColor::new(PaletteOr::NonPalette(StaticColor::deflate(RGB8::new(0x000000)))),
				foreground: PackedColor::new(PaletteOr::NonPalette(StaticColor::deflate(RGB8::new(0xffffff)))),
			},
		}
	}

	pub fn into_inner(self) -> T {
		self.renderer
	}
}

impl<T: BasicRenderer> Renderer for CachedRenderer<T> {
	fn get_background(&self) -> PackedColor {
		self.render_state.background
	}

	fn set_background(&mut self, value: PackedColor) {
		if self.render_state.background == value { return; }

		let prev_value = self.render_state.background;
		self.render_state.background = value;
		self.renderer.set_background(&self.render_state, prev_value);
	}

	fn get_foreground(&self) -> PackedColor { 
		self.render_state.foreground
	}

	fn set_foreground(&mut self, value: PackedColor) {
		if self.render_state.foreground == value { return; }

		let prev_value = self.render_state.foreground;
		self.render_state.foreground = value;
		self.renderer.set_foreground(&self.render_state, prev_value);
	}
	
	fn set(&mut self, pos: &Point, value: &str) {
		self.renderer.set(&self.render_state, pos, value);
	}
}

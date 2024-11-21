use crate::{oc_color::PackedColor, Formatter, math::Point};

use super::basic_renderer::{BasicRenderer, RenderState};

pub struct CodeRenderer<'a, T: Formatter> {
	code: String,
	gpu_ident: String,
	formatter: &'a T,
	dbg: Vec<Point>,
}

impl<'a, T: Formatter> CodeRenderer<'a, T> {
	pub fn new(gpu_ident: String, prelude: String, formatter: &'a T) -> Self {
		Self {
			code: prelude,
			gpu_ident,
			formatter,
			dbg: Vec::new(),
		}
	}

	pub fn build(self) -> String {
		self.code
	}
}

impl<'a, T: Formatter> BasicRenderer for CodeRenderer<'a, T> {
	fn set_background(&mut self, state: &RenderState, _prev_value: PackedColor) {
		self.code += &format!("{}.setBackground(0x{})\n", self.gpu_ident, self.formatter.inflate(state.background));
	}

	fn set_foreground(&mut self, state: &RenderState, _prev_value: PackedColor) {
		self.code += &format!("{}.setForeground(0x{})\n", self.gpu_ident, self.formatter.inflate(state.foreground));
	}

	fn set(&mut self, _state: &RenderState, pos: &Point, value: &str) {
		if self.dbg.contains(pos) {
			//panic!("ouf!");
		}
		self.dbg.push(*pos);

		self.code += &format!("{}.set({}, {}, \"{}\")\n", self.gpu_ident, pos.x + 1, pos.y + 1, value);
	}
}

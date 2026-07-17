use palette::{Srgb, FromColor};

use crate::math::*;
use super::{
	BasicRenderer,
	RenderState,
	super::super::oc_color::{PackedColor, formatters::Formatter},
};

pub struct CodeRenderer<'a, T: Formatter> {
	code: String,
	gpu_ident: String,
	formatter: &'a T,
	//dbg: Vec<Point>,
}

impl<'a, T: Formatter> CodeRenderer<'a, T> {
	pub fn new(gpu_ident: String, prelude: String, formatter: &'a T) -> Self {
		Self {
			code: prelude,
			gpu_ident,
			formatter,
			//dbg: Vec::new(),
		}
	}

	pub fn build(self) -> String {
		self.code
	}
}

impl<'a, T: Formatter> BasicRenderer for CodeRenderer<'a, T> {
	fn set_resolution(&mut self, value: Size<usize>) {
		self.code += &format!("{}.setResolution({},{})\n", self.gpu_ident, value.w, value.h);
	}

	fn set_background(&mut self, value: PackedColor) {
		let (r, g, b) = Srgb::from_color(self.formatter.inflate(value)).into_format::<u8>().into_components();
		self.code += &format!("{}.setBackground(0x{:02x}{:02x}{:02x})\n", self.gpu_ident, r, g, b);
	}

	fn set_foreground(&mut self, value: PackedColor) {
		let (r, g, b) = Srgb::from_color(self.formatter.inflate(value)).into_format::<u8>().into_components();
		self.code += &format!("{}.setForeground(0x{:02x}{:02x}{:02x})\n", self.gpu_ident, r, g, b);
	}

	fn set(&mut self, _state: &RenderState, pos: &Point<usize>, value: &str) {
		// if self.dbg.contains(pos) {
		// 	panic!("ouf!");
		// }
		// self.dbg.push(*pos);

		self.code += &format!("{}.set({}, {}, \"{}\")\n", self.gpu_ident, pos.x + 1, pos.y + 1, value);
	}
}

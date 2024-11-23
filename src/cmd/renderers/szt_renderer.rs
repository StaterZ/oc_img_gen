use szu::iter::SplitByBytes;

use crate::{cmd::szt_file::SztFrame, math::{Point, Size}, oc_color::PackedColor};

use super::basic_renderer::{BasicRenderer, RenderState};

enum CommandKind {
	SetBackground,
	SetForeground,
	Set,
}

pub struct SztRenderer {
	pub blob: Vec<u8>,
}

impl SztRenderer {
	pub fn new() -> Self {
		Self {
			blob: Vec::new(),
		}
	}
	
	pub fn build(self) -> SztFrame {
		SztFrame {
			commands: self.blob,
		}
	}
}

impl BasicRenderer for SztRenderer {
	fn set_resolution(&mut self, _state: &RenderState, value: Size<usize>) {
		self.blob.push(0b1000_0010);
		self.blob.push(value.x as u8);
		self.blob.push(value.y as u8);
	}

	fn set_background(&mut self, _state: &RenderState, value: PackedColor) {
		self.blob.push(0b1000_0000);
		self.blob.push(value.into());
	}

	fn set_foreground(&mut self, _state: &RenderState, value: PackedColor) {
		self.blob.push(0b1000_0001);
		self.blob.push(value.into());
	}

	fn set(&mut self, _state: &RenderState, pos: &Point, value: &str) {
		for chunk in SplitByBytes::new(value, 0x7f) {
			self.blob.push(chunk.as_bytes().len() as u8);
			self.blob.push(pos.x as u8);
			self.blob.push(pos.y as u8);
			self.blob.extend_from_slice(chunk.as_bytes());
		}
	}
}

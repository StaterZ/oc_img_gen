use more_asserts::*;
use szu::iter::SplitByBytes;

use crate::math::{Point, Size};
use super::super::{packet, super::oc_color::PackedColor};

use super::basic_renderer::{BasicRenderer, RenderState};

pub struct SztRenderer<const KIND: packet::CommandKind> {
	blob: Vec<u8>,
	bg_needs_emit: bool,
	fg_needs_emit: bool,
}

impl<const KIND: packet::CommandKind> SztRenderer<KIND> {
	pub fn new() -> Self {
		Self {
			blob: Vec::new(),
			bg_needs_emit: false,
			fg_needs_emit: false,
		}
	}
	
	pub fn build(self) -> packet::Frame {
		packet::Frame {
			command_kind: KIND,
			commands_len: 0,
			commands: self.blob,
		}
	}
}

impl<const KIND: packet::CommandKind> BasicRenderer for SztRenderer<KIND> {
	fn set_resolution(&mut self, _state: &RenderState, _value: Size<usize>) {
		panic!("tried to set resoultion on SZT renderer");
	}

	fn set_background(&mut self, _state: &RenderState, _value: PackedColor) {
		std::debug_assert_eq!(self.bg_needs_emit, false, "background set twice without set call inbetween");
		self.bg_needs_emit = true;
	}

	fn set_foreground(&mut self, _state: &RenderState, _value: PackedColor) {
		std::debug_assert_eq!(self.fg_needs_emit, false, "foreground set twice without set call inbetween");
		self.fg_needs_emit = true;
	}

	fn set(&mut self, state: &RenderState, pos: &Point<usize>, value: &str) {
		fn to_braille(c: char) -> u8 {
			match c {
				' ' => 0x00,
				'█' => 0xff,
				'⠀'..'⣿' => (c as u32 - '⠀' as u32) as u8,
				_ => unreachable!(),
			}
		}

		const MAX_BYTES: usize = 1 << (8 - 2);
		let mut pos = *pos;
		let mut encode_chunk = |chunk: &[u8], chunk_uni_len: usize| {
			debug_assert_gt!(chunk.len(), 0);
			debug_assert_le!(chunk.len(), MAX_BYTES);
			{
				let bg_flag = (self.bg_needs_emit as u8) << 7;
				let fg_flag = (self.fg_needs_emit as u8) << 6;
				let len = (chunk.len() - 1) as u8;
				self.blob.push(bg_flag | fg_flag | len);
			}

			if self.bg_needs_emit {
				if let Some(bg) = state.background {
					self.blob.push(bg.into());
				}
				self.bg_needs_emit = false;
			}
			if self.fg_needs_emit {
				if let Some(fg) = state.foreground {
					self.blob.push(fg.into());
				}
				self.fg_needs_emit = false;
			}

			self.blob.push(pos.x as u8);
			self.blob.push(pos.y as u8);
			pos.x += chunk_uni_len;
			self.blob.extend_from_slice(chunk);
		};

		match KIND {
			packet::CommandKind::Text => SplitByBytes::new(value, MAX_BYTES).for_each(|chunk| encode_chunk(chunk.as_bytes(), chunk.len())),
			packet::CommandKind::Braille => {
				let mut iter = value.chars().map(to_braille).array_chunks::<MAX_BYTES>();
				while let Some(chunk) = iter.next() {
					encode_chunk(&chunk, 64);
				}
				if let Some(rem) = iter.into_remainder() {
					let rem = rem.as_slice();
					if !rem.is_empty() {
						encode_chunk(rem, rem.len());
					}
				}
			},
		}
	}
}

use more_asserts::*;
use szu::iter::SplitByBytes;

use crate::{cmd::szt, math::{Point, Size}, oc_color::PackedColor};

use super::basic_renderer::{BasicRenderer, RenderState};

pub struct SztRenderer<const KIND: szt::CommandKind> {
	commands: Vec<szt::Command>,
	bg_needs_emit: bool,
	fg_needs_emit: bool,
}

impl<const KIND: szt::CommandKind> SztRenderer<KIND> {
	pub fn new() -> Self {
		Self {
			commands: Vec::new(),
			bg_needs_emit: false,
			fg_needs_emit: false,
		}
	}
	
	pub fn build(self) -> szt::Frame {
		szt::Frame {
			command_kind: KIND,
			commands: self.commands,
		}
	}
}

impl<const KIND: szt::CommandKind> BasicRenderer for SztRenderer<KIND> {
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
		
		let mut encode_chunk = |chunk: &[u8]| {
			debug_assert_gt!(chunk.len(), 0);
			debug_assert_le!(chunk.len(), MAX_BYTES);

			self.commands.push(szt::Command {
				flags: szt::CommandFlags {
					len: (chunk.len() - 1) as u8,
					has_background: self.bg_needs_emit,
					has_foreground: self.fg_needs_emit,
				},
				background: state.background,
				foreground: state.foreground,
				pos: pos.map(|x| x as u8),
				braille: chunk.to_owned(),
			});

			self.bg_needs_emit = false;
			self.fg_needs_emit = false;
		};

		match KIND {
			szt::CommandKind::Text => SplitByBytes::new(value, MAX_BYTES).for_each(|chunk| encode_chunk(chunk.as_bytes())),
			szt::CommandKind::Braille => {
				let mut iter = value.chars().map(to_braille).array_chunks::<MAX_BYTES>();
				while let Some(chunk) = iter.next() {
					encode_chunk(&chunk);
				}
				if let Some(rem) = iter.into_remainder() {
					encode_chunk(rem.as_slice());
				}
			},
		}
	}
}

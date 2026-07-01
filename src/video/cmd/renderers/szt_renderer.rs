use all_asserts::*;
use deku::prelude::*;
use itertools::Itertools;
use szu::iter::{RleExt, RleRun, SplitByBytes};

use crate::math::{Point, Size};
use super::{
	BasicRenderer,
	super::{packet::{Command, CommandData, CommandFlags, CommandKind, Frame}, super::oc_color::PackedColor, renderers::cached_renderer::RenderState},
};

pub struct SztRenderer<const KIND: CommandKind> {
	blob: Vec<u8>,
	bg_needs_emit: bool,
	fg_needs_emit: bool,
}

impl<const KIND: CommandKind> SztRenderer<KIND> {
	pub fn new() -> Self {
		Self {
			blob: Vec::new(),
			bg_needs_emit: false,
			fg_needs_emit: false,
		}
	}
	
	pub fn build(self) -> Frame {
		let mut frame = Frame {
			command_kind: KIND,
			commands_len: 0,
			commands: self.blob,
		};
		frame.update().unwrap();
		frame
	}
}

impl<const KIND: CommandKind> BasicRenderer for SztRenderer<KIND> {
	fn set_resolution(&mut self, _value: Size<usize>) {
		//panic!("tried to set resoultion on SZT renderer");
	}

	fn set_background(&mut self, _value: PackedColor) {
		std::debug_assert_eq!(self.bg_needs_emit, false, "background set twice without set call inbetween");
		self.bg_needs_emit = true;
	}

	fn set_foreground(&mut self, _value: PackedColor) {
		std::debug_assert_eq!(self.fg_needs_emit, false, "foreground set twice without set call inbetween");
		self.fg_needs_emit = true;
	}

	fn set(&mut self, state: &RenderState, pos: &Point<usize>, value: &str) {
		fn to_braille(c: char) -> u8 {
			match c {
				' ' => 0x00,
				'█' => 0xff,
				'⠀'..='⣿' => (c as u32 - '⠀' as u32) as u8,
				_ => unreachable!(),
			}
		}

		let mut pos = *pos;

		let mut encode_chunk = |data: CommandData, chunk_uni_len: usize| {
			let data_len = match &data {
				CommandData::Raw(v) => v.len(),
				CommandData::Rle(v) => v.len(),
			};
			debug_assert_range!(1..=Command::MAX_BRAILLE_COUNT, data_len);

			self.blob.extend_from_slice(Command {
				flags: CommandFlags {
					has_background: self.bg_needs_emit,
					has_foreground: self.fg_needs_emit,
					is_rle: matches!(data, CommandData::Rle(_)),
					len: (data_len - 1) as u8,
				},
				background: self.bg_needs_emit.then_some(state.background).flatten(),
				foreground: self.fg_needs_emit.then_some(state.foreground).flatten(),
				pos: pos.cast::<u8>(),
				data,
			}.to_bytes().unwrap().as_slice());
			pos.x += chunk_uni_len;
			self.fg_needs_emit = false;
			self.bg_needs_emit = false;
		};

		match KIND {
			CommandKind::Text => {
				for chunk in SplitByBytes::new(value, Command::MAX_BRAILLE_COUNT) {
					encode_chunk(CommandData::Raw(chunk.as_bytes().to_vec()), chunk.len());
				}
			},
			CommandKind::Braille => {
				let raw = value.chars().map(to_braille).collect_vec();

				let rle_runs = raw
					.iter()
					.copied()
					.peekable()
					.rle::<u8>()
					.collect_vec();

				// RLE wins if total runs < total raw bytes (assuming equal wire size per element).
				if rle_runs.len() * size_of::<RleRun<u8, u8>>() < raw.len() * size_of::<u8>() {
					for rle_chunk in rle_runs.chunks(Command::MAX_BRAILLE_COUNT) {
						let chunk_uni_len = rle_chunk.iter().map(|r| r.len as usize).sum();
						encode_chunk(CommandData::Rle(rle_chunk.to_vec()), chunk_uni_len);
					}
				} else {
					for raw_chunk in raw.chunks(Command::MAX_BRAILLE_COUNT) {
						encode_chunk(CommandData::Raw(raw_chunk.to_vec()), raw_chunk.len());
					}
				}
			},
		}
	}
}

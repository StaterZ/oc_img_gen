use std::marker::ConstParamTy;

use deku::prelude::*;

use crate::{map_bitmap, math::{Point, Size}, oc_color::PackedColor};

use super::{batchers, renderers::{CachedRenderer, SztRenderer}, BrailleFrame, TermFrame};

#[derive(DekuWrite)]
#[deku(magic = b"sztb")]
pub struct Header {
	#[deku(endian = "big")] pub version: u16,
	pub size: Size<u8>,
	#[deku(endian = "big")] pub frame_rate: u16,
	#[deku(endian = "big")] pub num_frames: u32,
}

#[derive(DekuWrite, ConstParamTy, PartialEq, Eq)]
#[deku(id_type = "u8")]
pub enum CommandKind {
	#[deku(id = 0x00)] Text,
	#[deku(id = 0x01)] Braille,
}

#[derive(DekuWrite)]
pub struct CommandFlags {
	#[deku(bits = 1)] pub has_background: bool,
	#[deku(bits = 1)] pub has_foreground: bool,
	#[deku(bits = 6)] pub len: u8,
}

#[derive(DekuWrite)]
pub struct Command {
	pub flags: CommandFlags,
	#[deku(cond = "flags.has_background")] pub background: Option<PackedColor>,
	#[deku(cond = "flags.has_foreground")] pub foreground: Option<PackedColor>,
	pub pos: Point<u8>,
	#[deku(count = "flags.len")]
	pub braille: Vec<u8>,
}

#[derive(DekuWrite)]
pub struct Frame {
	pub command_kind: CommandKind,
	pub commands: Vec<u8>,
	//pub commands: Vec<Command>,
}

impl Frame {
	pub fn size(&self) -> usize {
		self.to_bytes().unwrap().len()
	}
}

#[derive(DekuWrite)]
pub struct File {
	pub header: Header,

	#[deku(count = "header.num_frames", endian = "big")]
	pub frame_sizes: Vec<u32>,
	
	#[deku(count = "header.num_frames")]
	pub frames: Vec<Frame>,
}

impl File {
	pub fn new(size: Size<u8>, frame_rate: u16) -> Self {
		Self {
			header: Header {
				version: 2,
				size,
				frame_rate,
				num_frames: 0,
			},
			frame_sizes: Vec::new(),
			frames: Vec::new(),
		}
	}
	
	pub fn push_frame(&mut self, frame: Frame) {
		self.frame_sizes.push(frame.size() as u32);
		self.frames.push(frame);
		self.header.num_frames += 1;
	}
}

pub struct Writer {
	pub file: File,
	prev_frame: Option<TermFrame>,
}

impl Writer {
	pub fn new(size: Size<u8>, frame_rate: u16) -> Self {
		Self {
			file: File::new(size, frame_rate),
			prev_frame: None,
		}
	}

	pub fn push_frame_text(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(SztRenderer::<{ CommandKind::Text }>::new());
		batchers::batcher2::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.file.push_frame(renderer.into_inner().build());
		
		self.prev_frame = Some(frame);
	}

	pub fn push_frame_braille(&mut self, frame: BrailleFrame) {
		let frame = map_bitmap(&frame, |braille| braille.into());

		let mut renderer = CachedRenderer::new(SztRenderer::<{ CommandKind::Braille }>::new());
		batchers::batcher2::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.file.push_frame(renderer.into_inner().build());

		self.prev_frame = Some(frame);
	}

	pub fn serialize(&self) -> Result<Vec<u8>, DekuError> {
		self.file.to_bytes()
	}
}

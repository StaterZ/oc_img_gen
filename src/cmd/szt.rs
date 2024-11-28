use std::marker::ConstParamTy;

use deku::prelude::*;
use itertools::Itertools;

use crate::{math::{Point, Size}, oc_color::PackedColor};

use super::{batchers, renderers::{CachedRenderer, SztRenderer}, BrailleFrame, TermFrame};

#[derive(DekuWrite, ConstParamTy, PartialEq, Eq)]
#[deku(endian = "big", id_type = "u8")]
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
pub struct SizedString<TLen: DekuWriter> {
	len: TLen,

	#[deku(count = "len")]
	data: Vec<u8>,
}

impl<TLen: DekuWriter + TryFrom<usize>> SizedString<TLen> {
	pub fn new(text: &str) -> Option<Self> {
		Some(Self {
			len: text.as_bytes().len().try_into().ok()?,
			data: text.as_bytes().to_owned(),
		})
	}
}

#[derive(DekuWrite)]
pub struct StreamDesc {
	pub size: Size<u8>,
	pub name: SizedString<u8>,
}

impl StreamDesc {
	pub fn new(name: &str, size: Size<u8>) -> Option<Self> {
		Some(Self {
			size,
			name: SizedString::new(name)?,
		})
	}
}

#[derive(DekuWrite)]
#[deku(endian = "big", magic = b"sztb")]
pub struct Header {
	pub version: u16,
	pub frame_rate: u16,
	pub num_frames: u32,
	pub num_streams: u8,
}

#[derive(DekuWrite)]
pub struct File {
	pub header: Header,

	#[deku(count = "header.num_streams")]
	pub stream_descs: Vec<StreamDesc>,

	#[deku(count = "header.num_frames")]
	#[deku(endian = "big")] pub frame_sizes: Vec<Vec<u32>>,

	#[deku(count = "header.num_frames")]
	pub frames: Vec<Vec<Frame>>,
}

impl File {
	pub fn new(frame_rate: u16) -> Self {
		Self {
			header: Header {
				version: 3,
				frame_rate,
				num_frames: 0,
				num_streams: 0,
			},
			stream_descs: Vec::new(),
			frame_sizes: Vec::new(),
			frames: Vec::new(),
		}
	}
}

pub struct StreamWriter {
	desc: StreamDesc,
	frame_sizes: Vec<u32>,
	frames: Vec<Frame>,
	prev_frame: Option<TermFrame>,
}

impl StreamWriter {
	pub fn new(desc: StreamDesc) -> Self {
		Self {
			desc,
			frame_sizes: Vec::new(),
			frames: Vec::new(),
			prev_frame: None,
		}
	}
	
	pub fn push_frame_text(&mut self, frame: TermFrame) {
		self.push_frame::<{ CommandKind::Text }>(frame);
	}

	pub fn push_frame_braille(&mut self, frame: &BrailleFrame) {
		self.push_frame::<{ CommandKind::Braille }>(frame.map(|braille| braille.into()));
	}
	
	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(SztRenderer::<CMD_KIND>::new());
		batchers::batcher2::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.prev_frame = Some(frame);

		let frame = renderer.into_inner().build();
		self.frame_sizes.push(frame.size() as u32);
		self.frames.push(frame);
	}
}

pub struct FileWriter {
	pub file: File,
}

impl FileWriter {
	pub fn new(frame_rate: u16) -> Self {
		Self {
			file: File::new(frame_rate),
		}
	}

	pub fn push_stream(&mut self, stream: StreamWriter) {
		if self.file.frames.is_empty() {
			self.file.frames.resize_with(stream.frame_sizes.len(), || Vec::new());
		} else {
			debug_assert_eq!(self.file.frames.len(), stream.frames.len());
		}

		self.file.stream_descs.push(stream.desc);
		self.file.frame_sizes.push(stream.frame_sizes);
		for (frame_streams, stream_frame) in self.file.frames
			.iter_mut()
			.zip_eq(stream.frames)
		{
			frame_streams.push(stream_frame);
		}
		self.file.header.num_streams += 1;
	}

	pub fn serialize(&self) -> Result<Vec<u8>, DekuError> {
		self.file.to_bytes()
	}
}

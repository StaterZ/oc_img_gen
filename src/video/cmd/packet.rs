use std::marker::ConstParamTy;

use deku::prelude::*;

use crate::math::{Point, Size};
use crate::encoder::media_container::{DescriptorHeader, MediaFile, Packet, PacketData, SeekTable, StreamDescriptor};
use super::super::oc_color::PackedColor;

use super::{batchers, renderers::{CachedRenderer, SztRenderer}, BrailleFrame, TermFrame};

#[derive(DekuWrite, ConstParamTy, PartialEq, Eq)]
#[deku(endian = "little", id_type = "u8")]
pub enum CommandKind {
	#[deku(id = 0x00)] Text,
	#[deku(id = 0x01)] Braille,
}

#[derive(DekuWrite)]
#[deku(endian = "little")]
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
pub struct Descriptor {
	pub header: DescriptorHeader,
	#[deku(endian = "little")] pub frame_rate: u16,
	pub size: Size<u8>,
}

pub struct VideoEncoder {
	desc: Descriptor,
	frame_sizes: Vec<u32>,
	frames: Vec<Frame>,
	prev_frame: Option<TermFrame>,
}

impl VideoEncoder {
	pub fn new(desc: Descriptor) -> Self {
		Self {
			desc,
			frame_sizes: Vec::new(),
			frames: Vec::new(),
			prev_frame: None,
		}
	}
	
	pub fn get_desc(&self) -> &Descriptor {
		&self.desc
	}

	pub fn push_frame_text(&mut self, frame: TermFrame) {
		self.push_frame::<{ CommandKind::Text }>(frame);
	}

	pub fn push_frame_braille(&mut self, frame: &BrailleFrame) {
		self.push_frame::<{ CommandKind::Braille }>(frame.map(|braille| braille.into()));
	}
	
	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(SztRenderer::<CMD_KIND>::new());
		batchers::batcher_v2::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.prev_frame = Some(frame);

		let frame = renderer.into_inner().build();
		self.frame_sizes.push(frame.size() as u32);
		self.frames.push(frame);
	}

	pub fn attach(self, file: &mut MediaFile) {
		let stream_id = file.header.num_streams;
		file.header.num_streams += 1;
		file.stream_descs.push(StreamDescriptor::Video(self.desc));

		for frame in self.frames {
			file.packets.push(Packet {
				stream_id,
				data: PacketData::Video(frame),
			});

			todo!("insert at right times");
		}
		
		file.seek_tables.push(SeekTable {
			start_offsets: Vec::new(),
		});
		todo!("update seek tables");
	}
}

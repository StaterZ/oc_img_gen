use std::marker::ConstParamTy;

use deku::prelude::*;

use crate::math::{Frac, Point, Rect, Size};
use crate::encoder::{
	media_container::{
		Descriptor as StreamDescriptor,
		DescriptorContent,
		Packet,
		PacketContent,
		SizedString
	},
	muxer::PacketWriter,
};

use super::{
	super::{
		braille,
		oc_color::{formatters::{HybridFormatter, Formatter}, PackedColor, RGB8, PaletteOr},
		Image,
	},
	batcher,
	renderers::{CachedRenderer, StatRenderer, SztRenderer},
	BrailleFrame,
	TermFrame,
	Machine,
};

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
#[deku(ctx = "desc: &Descriptor")]
pub struct Frame {
	#[deku(endian = "little", update = "self.commands.len()")] pub commands_len: u16,
	pub command_kind: CommandKind,
	#[deku(count = "commands_len")] pub commands: Vec<u8>,
	//pub commands: Vec<Command>,
}

#[derive(Clone, DekuWrite)]
pub struct Descriptor {
	#[deku(endian = "little")] pub frame_rate: u16,
	pub size: Size<u8>,
}

pub struct VideoEncoder {
	name: SizedString<u8>,
	pub desc: Descriptor,
	frames: Vec<Frame>,
	prev_frame: Option<TermFrame>,
	num_frames_since_emit: usize,
	source_area: Option<Rect<usize>>,
	num_written_frames: u64,
	stream_id: u8,
}

impl VideoEncoder {
	pub fn new(name: SizedString<u8>, source_area: Option<Rect<usize>>, desc: Descriptor, stream_id: u8) -> Self {
		Self {
			name,
			source_area,
			desc,
			frames: Vec::new(),
			prev_frame: None,
			num_frames_since_emit: 0,
			num_written_frames: 0,
			stream_id,
		}
	}
	
	pub fn process(&mut self, img: &Image<RGB8>, formatter: &HybridFormatter) {
		let img = match &self.source_area {
			Some(source_area) => crate::stage("Stream | Preamble  | Crop", || img.crop(source_area)),
			None => img.clone(), //TODO: PERF
		};

		let img = crate::stage("Stream | Process   | Black&White", || img.map(|p| {
			const BLACK: RGB8 = RGB8::new(0x000000);
			const WHITE: RGB8 = RGB8::new(0xffffff);
			if p.perceptual_delta(WHITE) < p.perceptual_delta(BLACK) { WHITE } else { BLACK }
		}));

		// let img = crate::stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
		// let img = crate::stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));

		let img = crate::stage("Stream | Process   | Braille", || braille::as_braille(&img));
		let img = crate::stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
		
		crate::stage("Stream | Postamble | Cmd Gen", || self.push_frame_braille(&img));
		//println!("{}", cmd::code_gen(&img.map(|braille| braille.into()), None, &formatter));
	}

	fn push_frame_text(&mut self, frame: TermFrame) {
		self.push_frame::<{ CommandKind::Text }>(frame);
	}

	fn push_frame_braille(&mut self, frame: &BrailleFrame) {
		self.push_frame::<{ CommandKind::Braille }>(frame.map(|braille| braille.into()));
	}
	
	fn push_frame<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(SztRenderer::<CMD_KIND>::new());
		batcher::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.prev_frame = Some(frame);

		let mut frame = renderer.into_inner().build();
		frame.update().unwrap();
		self.frames.push(frame);
	}

	fn push_frame_fancy<const CMD_KIND: CommandKind>(&mut self, frame: TermFrame) {
		let mut renderer = CachedRenderer::new(StatRenderer::new(SztRenderer::<CMD_KIND>::new()));
		batcher::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		let renderer = renderer.into_inner();

		let machine = Machine::new_t3();
		let cost = renderer.get_cost(&machine);
		self.num_frames_since_emit += 1;
		let budget = (machine.call_budget * 20) / self.desc.frame_rate as usize * self.num_frames_since_emit;
		//println!("cost:{}%", (cost / budget * 100).into_int());
		let mut frame = if cost <= budget {
			self.num_frames_since_emit = 0;
			self.prev_frame = Some(frame);
			renderer.into_inner().build()
		} else {
			//println!("experimental budget jump!");
			Frame { //TODO: remove this terrible by moving from frame-rate-based format to timestamped frames
				commands_len: 0,
				command_kind: CMD_KIND,
				commands: Vec::new(),
			}
		};
		frame.update().unwrap();
		self.frames.push(frame);
	}
}

impl PacketWriter for VideoEncoder {
	fn get_descriptor(self) -> StreamDescriptor {
		StreamDescriptor {
			num_packets: 0,
			name: self.name,
			content: DescriptorContent::Video(self.desc),
		}
	}

	fn get_next_packet_time(&self) -> Option<Frac<u64>> {
		(!self.frames.is_empty()).then_some(Frac::new(self.num_written_frames, self.desc.frame_rate as u64))
	}

	fn get_next_packet(&mut self) -> Option<Packet> {
		self.frames.pop().map(|frame| {
			self.num_written_frames += 1;
			Packet {
				stream_id: self.stream_id,
				content: PacketContent::Video(frame),
			}
		})
	}
}

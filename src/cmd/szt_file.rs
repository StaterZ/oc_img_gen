use bincode::Options;
use serde::Serialize;

use crate::math::Size;

use super::{batchers, renderers::{CachedRenderer, SztRenderer}, Frame};


#[derive(Serialize)]
#[repr(packed)]
pub struct SztHeader {
	magic: [u8; 4],
	version: u16,
	pub size: Size<u8>,
	pub frame_rate: u16,
}

#[derive(Serialize)]
pub struct SztFrame {
	pub commands: Vec<u8>,
}

#[derive(Serialize)]
pub struct SztFile {
	pub header: SztHeader,
	pub frames: Vec<SztFrame>,
}

impl SztFile {
	pub fn new(size: Size<u8>, frame_rate: u16) -> Self {
		Self {
			header: SztHeader {
				magic: *b"sztb",
				version: 1,
				size,
				frame_rate,
			},
			frames: Vec::new(),
		}
	}
}

pub struct SztWriter {
	pub file: SztFile,
	prev_frame: Option<Frame>,
}

impl SztWriter {
	pub fn new(size: Size<u8>, frame_rate: u16) -> Self {
		Self {
			file: SztFile::new(size, frame_rate),
			prev_frame: None,
		}
	}

	pub fn push_frame(&mut self, frame: Frame) {
		let mut renderer = CachedRenderer::new(SztRenderer::new());
		batchers::batcher2::draw(&mut renderer, &frame, self.prev_frame.as_ref());
		self.file.frames.push(renderer.into_inner().build());
		
		self.prev_frame = Some(frame);
	}

	pub fn serialize(&self) -> bincode::Result<Vec<u8>> {
		let options = bincode::DefaultOptions::new()
			.with_big_endian()
			.with_fixint_encoding();
		
		options.serialize(&self.file)
	}
}

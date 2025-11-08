use szu::iter::IteratorExt;

use crate::{encoder::media_container::{Descriptor, MediaFile, Packet}, math::Frac};

pub trait PacketWriter {
	fn get_descriptor(self) -> Descriptor;
	fn get_next_packet_time(&self) -> Option<Frac<u64>>;
	fn get_next_packet(&mut self) -> Option<Packet>;
}

pub struct Muxer {
	output: MediaFile,
}

impl Muxer {
	pub fn new() -> Self {
		Self {
			output: MediaFile::new(),
		}
	}

	pub fn create_stream(&mut self, desc: Descriptor) -> u8 {
		let stream_id = self.output.stream_descs.len() as u8;
		self.output.stream_descs.push(desc);
		stream_id
	}

	pub fn process(&mut self, writers: &mut [&mut dyn PacketWriter]) {
		while let Some(writer) = writers
			.iter_mut()
			.min_by_key_while(|writer| writer.get_next_packet_time())
		{
			let packet = writer.get_next_packet().unwrap(); //SAFETY: unwrap is safe due to while condition
			self.output.stream_descs[packet.stream_id as usize].num_packets += 1;
			self.output.packets.push(packet);
		}
	}

	pub fn process_eof(mut self, writers: &mut [&mut dyn PacketWriter]) -> MediaFile {
		while let Some(writer) = writers
			.iter_mut()
			.filter_map(|writer| writer.get_next_packet_time().map(|t| (t, writer)))
			.min_by_key(|(t, _)| *t)
			.map(|(_, writer)| writer)
		{
			let packet = writer.get_next_packet().unwrap(); //SAFETY: unwrap is safe due to while condition
			self.output.stream_descs[packet.stream_id as usize].num_packets += 1;
			self.output.packets.push(packet);
		}
		self.output.header.num_streams = self.output.stream_descs.len() as u8;
		self.output
	}
}

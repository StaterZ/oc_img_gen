use szu::iter::IteratorExt;

use crate::{encoder::media_container::{Descriptor, DescriptorContent, MediaFile, Packet, StreamContent}, math::Frac};

pub trait PacketWriter {
	fn get_next_packet_end_time(&self) -> Option<Frac<u64>>;
	fn get_next_packet(&mut self) -> Option<Packet>;
}

pub struct Muxer {
	output: MediaFile,
}

impl Muxer {
	pub fn new(settings: String) -> Self {
		Self {
			output: MediaFile::new(settings),
		}
	}

	/// the stream the packet tracks share, made when the first one asks for it
	pub fn packet_stream(&mut self) -> u8 {
		match self.output.streams.iter().position(|stream| stream.content.is_packets()) {
			Some(stream_id) => stream_id as u8,
			None => self.output.create_stream(StreamContent::Packets(Vec::new()), 1),
		}
	}

	/// a stream for one pcm track, starting on a multiple of `align` bytes
	pub fn create_pcm_stream(&mut self, align: u32) -> u8 {
		self.output.create_stream(StreamContent::Pcm(Vec::new()), align)
	}

	pub fn create_track(&mut self, mut desc: Descriptor<DescriptorContent>, stream_id: u8) -> u8 {
		desc.stream_id = stream_id;
		let track_id = self.output.track_descs.len() as u8;
		self.output.track_descs.push(desc);
		track_id
	}

	fn push_packet(&mut self, packet: Packet) {
		let desc = &mut self.output.track_descs[packet.track_id as usize];
		desc.num_packets += 1;
		self.output.streams[desc.stream_id as usize].content
			.as_packets_mut()
			.unwrap()
			.push(packet);
	}

	pub fn process(&mut self, writers: &mut [&mut dyn PacketWriter]) {
		while let Some(writer) = writers
			.iter_mut()
			.min_by_key_while(|writer| writer.get_next_packet_end_time())
		{
			let packet = writer.get_next_packet().unwrap(); //SAFETY: unwrap is safe due to while condition
			self.push_packet(packet);
		}
	}

	pub fn process_eof(mut self, writers: &mut [&mut dyn PacketWriter]) -> MediaFile {
		while let Some(writer) = writers
			.iter_mut()
			.filter_map(|writer| writer
				.get_next_packet_end_time()
				.map(|t| (t, writer)))
			.min_by_key(|(t, _)| *t)
			.map(|(_, writer)| writer)
		{
			let packet = writer.get_next_packet().unwrap(); //SAFETY: unwrap is safe due to while condition
			self.push_packet(packet);
		}
		self.output
	}
}

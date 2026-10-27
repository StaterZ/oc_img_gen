use szu::iter::IteratorExt;

use crate::{encoder::media_container::{Descriptor, DescriptorContent, MediaFile, Packet}, math::Frac};

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

	pub fn create_track(&mut self, desc: Descriptor<DescriptorContent>) -> u8 {
		let track_id = self.output.track_descs.len() as u8;
		self.output.track_descs.push(desc);
		track_id
	}

	pub fn process(&mut self, writers: &mut [&mut dyn PacketWriter]) {
		while let Some(writer) = writers
			.iter_mut()
			.min_by_key_while(|writer| writer.get_next_packet_end_time())
		{
			let packet = writer.get_next_packet().unwrap(); //SAFETY: unwrap is safe due to while condition
			self.output.track_descs[packet.track_id as usize].num_packets += 1;
			self.output.packets.push(packet);
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
			self.output.track_descs[packet.track_id as usize].num_packets += 1;
			self.output.packets.push(packet);
		}
		self.output.header.num_tracks = self.output.track_descs.len() as u8;
		self.output
	}
}

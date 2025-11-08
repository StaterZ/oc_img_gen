use deku::prelude::*;

use crate::{
	math::Frac,
	encoder::{
		media_container::{
			Descriptor as StreamDescriptor,
			DescriptorContent,
			Packet,
			PacketContent,
			SizedString,
		},
		muxer::PacketWriter,
	},
};

#[derive(DekuWrite)]
#[deku(endian = "little")]
pub struct VoiceState {
	pub volume: u8,
	pub frequency: u16,
}

#[derive(DekuWrite)]
#[deku(ctx = "desc: &Descriptor")]
pub struct Sample {
	#[deku(count = "desc.num_voices")]
	pub voices: Vec<VoiceState>,
	pub duration: u8,
}

#[derive(Clone, DekuWrite)]
pub struct Descriptor {
	pub num_voices: u8,
}

pub struct AudioEncoder {
	pub name: SizedString<u8>,
	pub desc: Descriptor,
	pub samples: Vec<Sample>,
	pub milis_written: u64,
	pub stream_id: u8,
}

impl AudioEncoder {
	pub fn new(name: SizedString<u8>, desc: Descriptor) -> Self {
		Self {
			name,
			desc,
			samples: Vec::new(),
			milis_written: 0,
			stream_id: 0,
		}
	}
}

impl PacketWriter for AudioEncoder {
	fn get_descriptor(self) -> StreamDescriptor {
		StreamDescriptor {
			num_packets: self.samples.len() as u32,
			name: self.name,
			content: DescriptorContent::Audio(self.desc),
		}
	}

	fn get_next_packet_time(&self) -> Option<Frac<u64>> {
		(!self.samples.is_empty()).then_some(Frac::new(self.milis_written, 1000))
	}

	fn get_next_packet(&mut self) -> Option<Packet> {
		self.samples.pop().map(|sample| {
			self.milis_written += sample.duration as u64;
			Packet {
				stream_id: self.stream_id,
				content: PacketContent::Audio(sample),
			}
		})
	}
}

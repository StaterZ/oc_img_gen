use std::collections::VecDeque;
use deku::prelude::*;

use crate::{
	math::Frac,
	encoder::{
		media_container::{
			Descriptor as StreamDescriptor,
			Packet,
			PacketContent,
		},
		muxer::PacketWriter,
	},
};

#[derive(DekuWrite, DekuRead)]
#[deku(endian = "little")]
pub struct VoiceState {
	pub volume: u8,
	pub frequency: u16,
}

#[derive(DekuWrite, DekuRead)]
#[deku(ctx = "desc: &Descriptor")]
pub struct Sample {
	#[deku(count = "desc.num_voices")]
	pub voices: Vec<VoiceState>,
}

#[derive(Debug, Clone, DekuWrite, DekuRead)]
pub struct Descriptor {
	pub num_voices: u8,
}

pub struct AudioEncoder {
	pub desc: StreamDescriptor<Descriptor>,
	stream_id: u16,
	pub samples: VecDeque<Sample>,
}

impl AudioEncoder {
	pub fn new(desc: StreamDescriptor<Descriptor>, stream_id: u16) -> Self {
		Self {
			desc,
			stream_id,
			samples: VecDeque::new(),
		}
	}
}

impl PacketWriter for AudioEncoder {
	fn get_next_packet_time(&self) -> Option<Frac<u64>> {
		(!self.samples.is_empty()).then_some(Frac::from(self.desc.num_packets as u64) * self.desc.rate.cast::<u64>())
	}

	fn get_next_packet(&mut self) -> Option<Packet> {
		self.samples.pop_front().map(|sample| {
			self.desc.num_packets += 1;
			Packet {
				stream_id: self.stream_id,
				content: PacketContent::Audio(sample),
			}
		})
	}
}

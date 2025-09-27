use deku::prelude::*;

use crate::encoder::media_container::{DescriptorHeader, MediaFile, Packet, PacketData, StreamDescriptor};

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

#[derive(DekuWrite)]
pub struct Descriptor {
	pub header: DescriptorHeader,
	pub num_voices: u8,
}

pub struct AudioEncoder {
	desc: Descriptor,
	samples: Vec<Sample>,
}

impl AudioEncoder {
	pub fn new(desc: Descriptor) -> Self {
		Self {
			desc,
			samples: Vec::new(),
		}
	}
	
	pub fn get_desc(&self) -> &Descriptor {
		&self.desc
	}

	fn push_sample(&mut self, sample: Sample) {
		self.samples.push(sample);
		self.desc.header.num_packets += 1;
	}

	pub fn attach(self, file: &mut MediaFile) {
		let stream_id = file.header.num_streams;
		file.header.num_streams += 1;
		file.stream_descs.push(StreamDescriptor::Audio(self.desc));

		for sample in self.samples {
			file.packets.push(Packet {
				stream_id,
				data: PacketData::Audio(sample),
			});
		}
	}
}

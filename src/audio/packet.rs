use deku::prelude::*;

use crate::encoder::media_container::DescriptorHeader;

#[derive(DekuWrite)]
#[deku(endian = "little")]
pub struct VoiceState {
	volume: u8,
	frequency: u16,
}

#[derive(DekuWrite)]
pub struct Sample<const N: usize> {
	voices: [VoiceState; N],
	duration: u8,
}

#[derive(DekuWrite)]
pub struct Packet {
	samples: Vec<Sample<8>>,
}

#[derive(DekuWrite)]
pub struct Descriptor {
	pub header: DescriptorHeader,
	pub num_voices: u8,
}

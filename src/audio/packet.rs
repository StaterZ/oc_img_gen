use deku::prelude::*;

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
pub struct Packet<const N: usize> {
	samples: Vec<Sample<N>>,
}

#[derive(DekuWrite)]
#[deku(endian = "little")]
pub struct Header {
	pub num_voices: u8,
}

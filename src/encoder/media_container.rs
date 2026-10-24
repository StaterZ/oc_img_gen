use std::fmt::Debug;
use deku::prelude::*;
use deku_string::StringDeku;
use enum_as_inner::EnumAsInner;

use crate::math::*;
use crate::FORMAT_VERSION;

#[derive(Clone, DekuWrite, DekuRead)]
pub struct Descriptor<TContent: Debug + DekuWriter + for<'a> DekuReader<'a>> {
	#[deku(endian = "little")] pub num_packets: u32,
	#[deku(endian = "little")] pub rate: Frac<u16>,
	#[deku(endian = "little", ctx = "deku_string::Encoding::Utf8, deku_string::StringLayout::LengthPrefix(deku_string::Size::U8)")] pub name: StringDeku,
	pub content: TContent,
}
impl From<Descriptor<crate::video::cmd::packet::Descriptor>> for Descriptor<DescriptorContent> {
	fn from(value: Descriptor<crate::video::cmd::packet::Descriptor>) -> Self {
		Self {
			num_packets: value.num_packets,
			rate: value.rate,
			name: value.name,
			content: DescriptorContent::Video(value.content),
		}
	}
}
impl From<Descriptor<crate::audio::packet::Descriptor>> for Descriptor<DescriptorContent> {
	fn from(value: Descriptor<crate::audio::packet::Descriptor>) -> Self {
		Self {
			num_packets: value.num_packets,
			rate: value.rate,
			name: value.name,
			content: DescriptorContent::Audio(value.content),
		}
	}
}

impl From<Descriptor<crate::audio::packet::PcmDescriptor>> for Descriptor<DescriptorContent> {
	fn from(value: Descriptor<crate::audio::packet::PcmDescriptor>) -> Self {
		Self {
			num_packets: value.num_packets,
			rate: value.rate,
			name: value.name,
			content: DescriptorContent::Pcm(value.content),
		}
	}
}

#[derive(Debug, DekuWrite, DekuRead, EnumAsInner)]
#[deku(id_type = "u8")]
pub enum DescriptorContent {
	#[deku(id = 0x00)] Video(crate::video::cmd::packet::Descriptor),
	#[deku(id = 0x01)] Audio(crate::audio::packet::Descriptor),
	#[deku(id = 0x02)] Pcm(crate::audio::packet::PcmDescriptor),
}

impl DescriptorContent {
	pub fn tag(&self) -> u8 {
		match self {
			Self::Video(_) => 0x00,
			Self::Audio(_) => 0x01,
			Self::Pcm(_) => 0x02,
		}
	}
}

#[derive(DekuWrite, DekuRead)]
#[deku(ctx = "stream_descs: &[Descriptor<DescriptorContent>]")]
pub struct Packet {
	#[deku(endian = "little")] pub stream_id: u8,
	#[deku(ctx = "&stream_descs[*stream_id as usize]")] pub content: PacketContent,
}

#[derive(DekuWrite, DekuRead, EnumAsInner)]
#[deku(ctx = "desc: &Descriptor<DescriptorContent>", id = "desc.content.tag()")]
pub enum PacketContent {
	#[deku(id = 0x00)] Video(#[deku(ctx = "desc.content.as_video().unwrap()")] crate::video::cmd::packet::Frame),
	#[deku(id = 0x01)] Audio(#[deku(ctx = "desc.content.as_audio().unwrap()")] crate::audio::packet::Sample),
}

#[derive(DekuWrite, DekuRead)]
#[deku(endian = "little", magic = b"sztb")]
pub struct Header {
	pub version: u16,
	pub num_streams: u8,
}

#[derive(DekuWrite, DekuRead)]
pub struct MediaFile {
	pub header: Header,

	#[deku(count = "header.num_streams")]
	pub stream_descs: Vec<Descriptor<DescriptorContent>>,

	#[deku(
		count = "stream_descs.iter().map(|desc| desc.num_packets).sum::<u32>()",
		ctx = "stream_descs.as_slice()",
	)]
	pub packets: Vec<Packet>,

	/// the DFPWM blocks of the pcm streams. pcm streams have no packets
	#[deku(count = "stream_descs.iter().filter_map(|desc| desc.content.as_pcm()).map(|pcm| pcm.num_bytes as usize).sum::<usize>()")]
	pub pcm_data: Vec<u8>,
}

impl MediaFile {
	pub fn new() -> Self {
		Self {
			header: Header {
				version: FORMAT_VERSION,
				num_streams: 0,
			},
			stream_descs: Vec::new(),
			packets: Vec::new(),
			pcm_data: Vec::new(),
		}
	}

	pub fn push_pcm(&mut self, stream_id: u8, data: Vec<u8>) {
		let pcm = self.stream_descs[stream_id as usize].content.as_pcm_mut().unwrap();
		pcm.offset = self.pcm_data.len() as u32; //relative for now, into_bytes makes it absolute
		pcm.num_bytes = data.len() as u32;
		pcm.checksum = crate::audio::dfpwm::checksum(&data);
		self.pcm_data.extend(data);
	}

	pub fn get_pcm(&self, stream_id: u8) -> &[u8] {
		let pcm = self.stream_descs[stream_id as usize].content.as_pcm().unwrap();
		//the pcm data is the end of the file, so its blocks start at their offset minus where it starts
		let start = pcm.offset as usize + self.pcm_data.len() - self.pcm_end();
		&self.pcm_data[start..start + pcm.num_bytes as usize]
	}

	fn pcm_end(&self) -> usize {
		self.stream_descs
			.iter()
			.filter_map(|desc| desc.content.as_pcm())
			.map(|pcm| pcm.offset as usize + pcm.num_bytes as usize)
			.max()
			.unwrap_or(0)
	}

	/// serializes the file and points the pcm offsets at their blocks
	pub fn into_bytes(mut self) -> Result<Vec<u8>, DekuError> {
		if self.pcm_data.is_empty() {
			return self.to_bytes();
		}
		let pcm_start = (self.to_bytes()?.len() - self.pcm_data.len()) as u32;
		for desc in self.stream_descs.iter_mut() {
			if let Some(pcm) = desc.content.as_pcm_mut() {
				pcm.offset += pcm_start;
			}
		}
		self.to_bytes()
	}
}

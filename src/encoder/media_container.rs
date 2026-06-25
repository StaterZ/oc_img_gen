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

#[derive(Debug, DekuWrite, DekuRead, EnumAsInner)]
#[deku(id_type = "u8")]
pub enum DescriptorContent {
	#[deku(id = 0x00)] Video(crate::video::cmd::packet::Descriptor),
	#[deku(id = 0x01)] Audio(crate::audio::packet::Descriptor),
}

impl DescriptorContent {
	pub fn tag(&self) -> u8 {
		match self {
			Self::Video(_) => 0x00,
			Self::Audio(_) => 0x01,
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
	pub num_streams: u16,
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
		}
	}
}

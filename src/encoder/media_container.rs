use deku::prelude::*;
use enum_as_inner::EnumAsInner;

use crate::FORMAT_VERSION;

#[derive(Clone, DekuWrite)]
pub struct SizedString<TLen: DekuWriter> {
	len: TLen,

	#[deku(count = "len")]
	data: Vec<u8>,
}

impl<TLen: DekuWriter + TryFrom<usize>> SizedString<TLen> {
	pub fn new(text: &str) -> Option<Self> {
		Some(Self {
			len: text.as_bytes().len().try_into().ok()?,
			data: text.as_bytes().to_owned(),
		})
	}
}

#[derive(DekuWrite)]
pub struct Descriptor {
	#[deku(endian = "little")] pub num_packets: u32,
	pub name: SizedString<u8>,
	pub content: DescriptorContent,
}


#[derive(DekuWrite, EnumAsInner)]
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

#[derive(DekuWrite)]
#[deku(ctx = "stream_descs: &[Descriptor]")]
pub struct Packet {
	#[deku(endian = "little")] pub stream_id: u8,
	#[deku(ctx = "&stream_descs[*stream_id as usize]")] pub content: PacketContent,
}

#[derive(DekuWrite)]
#[deku(ctx = "desc: &Descriptor", id = "desc.content.tag()")]
pub enum PacketContent {
	#[deku(id = 0x00)] Video(#[deku(ctx = "desc.content.as_video().unwrap()")] crate::video::cmd::packet::Frame),
	#[deku(id = 0x01)] Audio(#[deku(ctx = "desc.content.as_audio().unwrap()")] crate::audio::packet::Sample),
}

#[derive(DekuWrite)]
#[deku(endian = "little", magic = b"sztb")]
pub struct Header {
	pub version: u16,
	pub num_streams: u8,
}

#[derive(DekuWrite)]
pub struct MediaFile {
	pub header: Header,

	#[deku(count = "header.num_streams")]
	pub stream_descs: Vec<Descriptor>,

	#[deku(
		count = "stream_descs.iter().map(|desc| desc.num_packets).sum()",
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

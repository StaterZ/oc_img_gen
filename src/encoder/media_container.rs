use deku::prelude::*;

use crate::FORMAT_VERSION;

#[derive(DekuWrite)]
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
#[deku(id_type = "u8")]
pub enum StreamDescriptor {
	#[deku(id = 0x00)] Video(crate::video::cmd::packet::Descriptor),
	#[deku(id = 0x01)] Audio(crate::audio::packet::Descriptor),
}

impl StreamDescriptor {
	pub fn tag(&self) -> u8 {
		match self {
			Self::Video(_) => 0x00,
			Self::Audio(_) => 0x01,
		}
	}
}

#[derive(DekuWrite)]
#[deku(ctx = "stream_descs: &[StreamDescriptor]")]
pub struct Packet {
	#[deku(endian = "little")] pub stream_id: u8,
	#[deku(ctx = "stream_descs[*stream_id as usize].tag()")] pub data: PacketData,
}

#[derive(DekuWrite)]
#[deku(ctx = "id: u8", id = "id")]
pub enum PacketData {
	#[deku(id = 0x00)] Video(crate::video::cmd::packet::Frame),
	#[deku(id = 0x01)] Audio(crate::audio::packet::Packet),
}

#[derive(DekuWrite)]
pub struct DescriptorHeader {
	#[deku(endian = "little")] pub num_packets: u16,
	pub name: SizedString<u8>,
}

#[derive(DekuWrite)]
#[deku(ctx = "header: &Header")]
pub struct SeekTable {
	#[deku(count = "ctx.num_streams")]
	#[deku(endian = "little")] pub start_offsets: Vec<u32>,
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
	pub stream_descs: Vec<StreamDescriptor>,

	#[deku(count = "header.num_streams", ctx = "header")]
	pub seek_tables: Vec<SeekTable>,

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
			seek_tables: Vec::new(),
			packets: Vec::new(),
		}
	}
}

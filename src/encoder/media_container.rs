use std::{fmt::Debug, io::Cursor};
use deku::{prelude::*, reader::Reader, writer::Writer};
use deku_string::StringDeku;
use enum_as_inner::EnumAsInner;

use crate::math::*;
use crate::FORMAT_VERSION;

#[derive(Clone, DekuWrite, DekuRead)]
pub struct Descriptor<TContent: Debug + DekuWriter + for<'a> DekuReader<'a>> {
	#[deku(endian = "little")] pub num_packets: u32,
	#[deku(endian = "little")] pub rate: Frac<u16>,
	#[deku(endian = "little", ctx = "deku_string::Encoding::Utf8, deku_string::StringLayout::LengthPrefix(deku_string::Size::U8)")] pub name: StringDeku,
	/// the stream that holds the packets of this track
	pub stream_id: u8,
	pub content: TContent,
}
impl From<Descriptor<crate::video::cmd::packet::Descriptor>> for Descriptor<DescriptorContent> {
	fn from(value: Descriptor<crate::video::cmd::packet::Descriptor>) -> Self {
		Self {
			num_packets: value.num_packets,
			rate: value.rate,
			name: value.name,
			stream_id: value.stream_id,
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
			stream_id: value.stream_id,
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
			stream_id: value.stream_id,
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
#[deku(ctx = "track_descs: &[Descriptor<DescriptorContent>]")]
pub struct Packet {
	#[deku(endian = "little")] pub track_id: u8,
	#[deku(ctx = "&track_descs[*track_id as usize]")] pub content: PacketContent,
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
	pub num_tracks: u8,
	pub num_streams: u8,
	/// the settings the file was encoded with, as "key: value" lines
	#[deku(ctx = "deku_string::Encoding::Utf8, deku_string::StringLayout::LengthPrefix(deku_string::Size::U16)")] pub settings: StringDeku,
}

/// where a stream is in the file. a stream is the interleaved packets of its tracks, or the DFPWM block of one pcm track
#[derive(Debug, Clone, Copy, Default, DekuWrite, DekuRead)]
#[deku(endian = "little")]
pub struct StreamDescriptor {
	/// counted from the start of the file
	pub offset: u32,
	pub num_bytes: u32,
}

/// everything before the streams
#[derive(DekuWrite, DekuRead)]
struct FileHeader {
	header: Header,
	#[deku(count = "header.num_tracks")]
	track_descs: Vec<Descriptor<DescriptorContent>>,
	#[deku(count = "header.num_streams")]
	stream_descs: Vec<StreamDescriptor>,
}

#[derive(EnumAsInner)]
pub enum StreamContent {
	Packets(Vec<Packet>),
	/// a pcm track is alone on its stream, so a tape drive can play the stream as is
	Pcm(Vec<u8>),
}

pub struct Stream {
	/// the encoder starts the stream on a multiple of this. 1 when read from a file
	pub align: u32,
	pub content: StreamContent,
}

/// the drive plays up to 250ms at a time, so it can overshoot a pcm stream. this much silence after one keeps the next stream out of the speaker
pub const PCM_PAD_SIZE: usize = 3000;
/// alternating DFPWM bits hold the level
pub const PCM_PAD_BYTE: u8 = 0x55;

pub struct MediaFile {
	pub header: Header,
	pub track_descs: Vec<Descriptor<DescriptorContent>>,
	pub streams: Vec<Stream>,
}

impl MediaFile {
	pub fn new(settings: String) -> Self {
		Self {
			header: Header {
				version: FORMAT_VERSION,
				num_tracks: 0,
				num_streams: 0,
				settings: settings.into(),
			},
			track_descs: Vec::new(),
			streams: Vec::new(),
		}
	}

	pub fn from_bytes(bytes: &[u8]) -> Result<Self, DekuError> {
		let (_, file_header) = FileHeader::from_bytes((bytes, 0))?;
		let streams = file_header.stream_descs
			.iter()
			.enumerate()
			.map(|(stream_id, stream_desc)| {
				let start = stream_desc.offset as usize;
				let data = bytes
					.get(start..start + stream_desc.num_bytes as usize)
					.ok_or_else(|| DekuError::Parse(format!("stream {stream_id} is past the end of the file").into()))?;

				let tracks = file_header.track_descs
					.iter()
					.filter(|desc| desc.stream_id as usize == stream_id);
				let content = match tracks.clone().next() {
					Some(desc) if desc.content.is_pcm() => StreamContent::Pcm(data.to_vec()),
					_ => {
						let num_packets = tracks.map(|desc| desc.num_packets as usize).sum::<usize>();
						let mut reader = Reader::new(Cursor::new(data));
						let packets = (0..num_packets)
							.map(|_| Packet::from_reader_with_ctx(&mut reader, file_header.track_descs.as_slice()))
							.collect::<Result<Vec<_>, _>>()?;
						StreamContent::Packets(packets)
					},
				};
				Ok(Stream { align: 1, content })
			})
			.collect::<Result<Vec<_>, DekuError>>()?;

		Ok(Self {
			header: file_header.header,
			track_descs: file_header.track_descs,
			streams,
		})
	}

	pub fn create_stream(&mut self, content: StreamContent, align: u32) -> u8 {
		let stream_id = self.streams.len() as u8;
		self.streams.push(Stream { align: align.max(1), content });
		stream_id
	}

	pub fn push_pcm(&mut self, track_id: u8, data: Vec<u8>) {
		let desc = &mut self.track_descs[track_id as usize];
		desc.content.as_pcm_mut().unwrap().checksum = crate::audio::dfpwm::checksum(&data);
		self.streams[desc.stream_id as usize].content = StreamContent::Pcm(data);
	}

	pub fn get_pcm(&self, track_id: u8) -> &[u8] {
		let desc = &self.track_descs[track_id as usize];
		self.streams[desc.stream_id as usize].content.as_pcm().unwrap()
	}

	/// the packets of all streams in the order they play, like they were one stream
	pub fn packets(&self) -> Vec<&Packet> {
		let mut iters = self.streams
			.iter()
			.filter_map(|stream| stream.content.as_packets())
			.map(|packets| packets.iter().peekable())
			.collect::<Vec<_>>();
		let mut num_played = vec![0u64; self.track_descs.len()];
		let mut out = Vec::with_capacity(iters.iter().map(|iter| iter.len()).sum());
		//each stream is in order already, so take the packet that ends first, like the muxer does
		while let Some(iter) = iters
			.iter_mut()
			.filter_map(|iter| {
				let packet = iter.peek()?;
				let desc = &self.track_descs[packet.track_id as usize];
				let end_time = Frac::from(num_played[packet.track_id as usize] + 1) * desc.rate.cast::<u64>();
				Some((end_time, iter))
			})
			.min_by_key(|(end_time, _)| *end_time)
			.map(|(_, iter)| iter)
		{
			let packet = iter.next().unwrap(); //SAFETY: peeked above
			num_played[packet.track_id as usize] += 1;
			out.push(packet);
		}
		out
	}

	/// serializes the file and lays out the streams after the header, each one aligned
	pub fn into_bytes(self) -> Result<Vec<u8>, DekuError> {
		let datas = self.streams
			.iter()
			.map(|stream| match &stream.content {
				StreamContent::Pcm(data) => Ok(data.clone()),
				StreamContent::Packets(packets) => {
					let mut data = Vec::new();
					let mut writer = Writer::new(Cursor::new(&mut data));
					for packet in packets {
						packet.to_writer(&mut writer, self.track_descs.as_slice())?;
					}
					writer.finalize()?;
					Ok(data)
				},
			})
			.collect::<Result<Vec<_>, DekuError>>()?;

		let mut file_header = FileHeader {
			header: Header {
				num_tracks: self.track_descs.len() as u8,
				num_streams: self.streams.len() as u8,
				..self.header
			},
			track_descs: self.track_descs,
			stream_descs: vec![StreamDescriptor::default(); self.streams.len()],
		};

		//the header has the same size whatever the offsets are
		let mut pos = file_header.to_bytes()?.len();
		let mut layout = Vec::with_capacity(datas.len());
		for (stream_id, (stream, data)) in self.streams.iter().zip(&datas).enumerate() {
			let align = stream.align as usize;
			let offset = pos.next_multiple_of(align);
			let is_last = stream_id + 1 == self.streams.len();
			let pad = if stream.content.is_pcm() && !is_last { PCM_PAD_SIZE } else { 0 };
			layout.push((offset - pos, pad));
			file_header.stream_descs[stream_id] = StreamDescriptor {
				offset: u32::try_from(offset).map_err(|_| DekuError::Assertion("the file is over 4GiB".into()))?,
				num_bytes: data.len() as u32,
			};
			pos = offset + data.len() + pad;
		}

		let mut bytes = file_header.to_bytes()?;
		bytes.reserve(pos - bytes.len());
		for (data, (gap, pad)) in datas.iter().zip(layout) {
			bytes.resize(bytes.len() + gap, 0);
			bytes.extend(data);
			bytes.resize(bytes.len() + pad, PCM_PAD_BYTE);
		}
		Ok(bytes)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::audio::packet::{Descriptor as AudioDescriptor, PcmDescriptor, Sample, VoiceState};

	fn audio_desc(stream_id: u8) -> Descriptor<DescriptorContent> {
		Descriptor::<AudioDescriptor> {
			num_packets: 0,
			rate: Frac::new(1, 10),
			name: "main".to_string().into(),
			stream_id,
			content: AudioDescriptor { num_voices: 1 },
		}.into()
	}

	fn sample(volume: u8) -> Packet {
		Packet { track_id: 0, content: PacketContent::Audio(Sample { voices: vec![VoiceState { volume, frequency: 0 }] }) }
	}

	#[test]
	fn streams_round_trip_and_align() {
		let mut file = MediaFile::new("a: b".to_string());
		let packets = file.create_stream(StreamContent::Packets(Vec::new()), 1);
		let pcm = file.create_stream(StreamContent::Pcm(Vec::new()), 4096);
		file.track_descs.push(audio_desc(packets));
		file.track_descs.push(Descriptor::<PcmDescriptor> {
			num_packets: 0,
			rate: Frac::new(1, 6000),
			name: "pcm".to_string().into(),
			stream_id: pcm,
			content: PcmDescriptor { sample_rate: 48000, checksum: 0 },
		}.into());
		file.track_descs[0].num_packets = 3;
		*file.streams[0].content.as_packets_mut().unwrap() = vec![sample(1), sample(2), sample(3)];
		file.push_pcm(1, vec![0xab; 100]);

		let bytes = file.into_bytes().unwrap();
		let file = MediaFile::from_bytes(&bytes).unwrap();
		assert_eq!(file.header.num_tracks, 2);
		assert_eq!(file.header.num_streams, 2);
		assert_eq!(file.get_pcm(1), &[0xab; 100][..]);
		assert_eq!(&bytes[4096..4196], &[0xab; 100][..]); //the pcm stream is aligned, and it is the last one, so no padding after it
		assert_eq!(bytes.len(), 4196);
		let volumes = file.packets()
			.iter()
			.map(|packet| packet.content.as_audio().unwrap().voices[0].volume)
			.collect::<Vec<_>>();
		assert_eq!(volumes, [1, 2, 3]);
	}

	#[test]
	fn packets_merge_by_end_time() {
		let mut file = MediaFile::new(String::new());
		let a = file.create_stream(StreamContent::Packets(Vec::new()), 1);
		let b = file.create_stream(StreamContent::Packets(Vec::new()), 1);
		file.track_descs.push(audio_desc(a));
		file.track_descs.push(audio_desc(b));
		file.track_descs[1].rate = Frac::new(1, 5); //half as many packets per second
		file.track_descs[0].num_packets = 4;
		file.track_descs[1].num_packets = 2;
		*file.streams[0].content.as_packets_mut().unwrap() = (1..=4).map(sample).collect();
		*file.streams[1].content.as_packets_mut().unwrap() = (1..=2).map(|v| Packet { track_id: 1, ..sample(v * 10) }).collect();

		let file = MediaFile::from_bytes(&file.into_bytes().unwrap()).unwrap();
		let volumes = file.packets()
			.iter()
			.map(|packet| packet.content.as_audio().unwrap().voices[0].volume)
			.collect::<Vec<_>>();
		assert_eq!(volumes, [1, 2, 10, 3, 4, 20]);
	}
}

use std::{ops::RangeInclusive, time::Duration};

use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use ffmpeg_next::{
	software::scaling::Context as Scaler,
	util::{
		frame::video::Video as VideoFrame,
		error::Error as FfmpegError,
	},
	codec::decoder::Video as VideoDecoder,
	media::Type as MediaType,
};
use num_traits::ConstZero;

use super::{
	media_container::SizedString,
	reader::{DecoderInterface, FrameInterface, Reader, ReaderData},
	muxer::{Muxer, PacketWriter},
};
use crate::encoder::media_container::{Descriptor as StreamDescriptor, DescriptorContent};
use crate::math::{Frac, Rect, Size};
use crate::video::{
	self,
	cmd::packet::{self, Descriptor as VideoStreamDesc},
	oc_color::RGB8,
	Image,
};

pub struct VideoConfig {
	pub stream_descs_data: Vec<VideoStreamDescData>,
	pub container_size: Size<usize>,
	pub fill_color: RGB8,
	pub cmds_per_sec: Option<usize>,
}

pub struct VideoStreamDescData {
	pub name: SizedString<u8>,
	pub frame_rate: Option<u16>,
	pub size: Size<u8>,
	pub source_area: Option<Rect<usize>>,
}

pub struct VideoReader<'a> {
	reader_data: ReaderData<'a, VideoDecoder>,
	scaler: Scaler,
	scaler_buffer: VideoFrame,
	container_size: Size<usize>,
	fill_color: RGB8,
	cmds_per_sec: Option<usize>,
	in_frame_rate: Frac<i64>,
	formatter: video::oc_color::formatters::HybridFormatter,
	pub encoders: Vec<packet::VideoEncoder>,
	stream_timers: Vec<Frac<i64>>,
}

impl DecoderInterface for VideoDecoder {
	type Frame = VideoFrame;
	
	fn new(decoder: ffmpeg_next::decoder::decoder::Decoder) -> Result<Self, FfmpegError> {
		decoder.video()
	}
	
	fn receive_frame(&mut self, frame: &mut Self::Frame) -> Result<(), FfmpegError> {
		ffmpeg_next::decoder::Opened::receive_frame(self, frame)
	}

	fn send_eof(&mut self) -> Result<(), FfmpegError> {
		ffmpeg_next::decoder::Opened::send_eof(self)
	}

	fn send_packet<P: ffmpeg_next::packet::Ref>(&mut self, packet: &P) -> Result<(), FfmpegError> {
		ffmpeg_next::decoder::Opened::send_packet(self, packet)
	}
}

impl FrameInterface for VideoFrame {
	fn empty() -> Self {
		VideoFrame::empty()
	}

	fn pts(&self) -> Option<i64> {
		ffmpeg_next::Frame::pts(self)
	}
}

impl<'a> VideoReader<'a> {
	pub fn new(
		ictx: &ffmpeg_next::format::context::Input,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		config: VideoConfig,
		muxer: &mut Muxer,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Video)?;

		let reader_data = ReaderData::<'a, VideoDecoder>::new("video", &stream, multi_progress, range);
		
		let in_frame_rate = Frac::from(stream.rate()).cast::<i64>();

		//setup down-scaler
		let content_size = Size::new(reader_data.decoder.width() as usize, reader_data.decoder.height() as usize);
		let fit_size = config.container_size.contain(content_size);

		let scaler = Scaler::get(
			reader_data.decoder.format(),
			content_size.x as u32,
			content_size.y as u32,
			ffmpeg_next::util::format::Pixel::RGB24,
			fit_size.x as u32,
			fit_size.y as u32,
			ffmpeg_next::software::scaling::Flags::BILINEAR,
		).unwrap();

		
		//setup SZT stream encoders
		let streams = config.stream_descs_data
			.into_iter()
			.map(|stream_data| {
				let size = if stream_data.source_area.is_some() {
					stream_data.size
				} else {
					let size = (fit_size / video::braille::SIZE).try_cast().unwrap();
					println!("auto-size-fit: {}", size);
					size
				};
				let desc = VideoStreamDesc {
					frame_rate: stream_data.frame_rate.unwrap_or(in_frame_rate.into_int() as u16),
					size,
				};

				let stream_id = muxer.create_stream(StreamDescriptor {
					num_packets: 0,
					name: stream_data.name.clone(),
					content: DescriptorContent::Video(desc.clone()),
				});

				packet::VideoEncoder::new(
					stream_data.name,
					stream_data.source_area,
					desc,
					stream_id,
				)
			})
			.collect_vec();
		
		let stream_timers = streams.iter().map(|_| Frac::ZERO).collect_vec();
		
		Some(Self {
			reader_data,
			scaler,
			scaler_buffer: VideoFrame::empty(),
			container_size: config.container_size,
			fill_color: config.fill_color,
			cmds_per_sec: config.cmds_per_sec,
			in_frame_rate,
			formatter: video::oc_color::formatters::HybridFormatter::new(),
			encoders: streams,
			stream_timers,
		})
	}
}

impl<'a> Reader<'a> for VideoReader<'a> {
	type Decoder = VideoDecoder;
	
	fn get_data(&self) -> &ReaderData<'a, Self::Decoder> {
		&self.reader_data
	}
	fn get_data_mut(&mut self) -> &mut ReaderData<'a, Self::Decoder> {
		&mut self.reader_data
	}
	
	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter> {
		self.encoders
			.iter_mut()
			.map(|encoder| encoder as &mut dyn PacketWriter)
			.collect()
	}

	fn process(&mut self, stream_time_s: Frac<i64>, should_force_emit: bool) {
		let frame = &self.reader_data.receive_buffer;

		let emit_streams = self.encoders
			.iter_mut()
			.zip_eq(self.stream_timers.iter_mut())
			.filter_map(|(stream, last_frame_time)| {
				if should_force_emit {
					return Some((stream, 1));
				}

				if stream_time_s < *last_frame_time { return None; }

				let delta = stream_time_s - *last_frame_time;
				let frame_rate = stream.desc.frame_rate as i64;
				let mut emit_count = (delta * frame_rate).into_int_trunc();
				if stream_time_s == 0.into() {
					emit_count = emit_count.max(1);
				}
				*last_frame_time += Frac::new(emit_count, frame_rate);
				(emit_count > 0).then_some((stream, emit_count))
			}).collect_vec();

		if cfg!(feature = "log") {
			println!();
			println!("emit: {}", if !emit_streams.is_empty() { format!("yes ({})", emit_streams.iter().map(|s| s.1.to_string()).join(",")) } else { "no".to_string() });
		}

		if emit_streams.is_empty() { return; }
		
		let frame = crate::stage("Frame  | Preamble  | Scale", || {
			self.scaler.run(frame, &mut self.scaler_buffer).unwrap();
			&self.scaler_buffer
		});
		
		let img = crate::stage("Frame  | Preamble  | Into Image", || Image::from(frame));
		let img = crate::stage("Frame  | Preamble  | Resize", || img.resize(self.container_size, self.fill_color));
		
		let emit_streams_len = emit_streams.len();
		let frame_progress = self.reader_data.multi_progress.add(ProgressBar::new(emit_streams_len as u64)
			.with_style(crate::build_progress_style())
			.with_message("frame"));
		if cfg!(not(feature = "log")) && emit_streams_len > 1 {
			frame_progress.tick();
		}

		for (stream, emit_count) in emit_streams {
			for _ in 0..emit_count {
				stream.process(&img, &self.formatter);
			}

			if cfg!(not(feature = "log")) && emit_streams_len > 1 {
				frame_progress.inc(1);
			}
		}
		if cfg!(not(feature = "log")) && emit_streams_len > 1 {
			frame_progress.finish_and_clear();
		}
	}
}

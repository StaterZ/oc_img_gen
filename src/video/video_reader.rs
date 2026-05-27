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
	format::context::Input as FfmpegInput,
};
use num_traits::ConstZero;

use crate::{
	encoder::{
		cli::{Budget, VideoFilter}, media_container::Descriptor as StreamDescriptor, muxer::{Muxer, PacketWriter}, reader::{DecoderInterface, FrameInterface, Reader, ReaderData}
	}, math::*, video::cmd::machine::Machine
};

use super::{
	cmd::packet::{self, Descriptor},
	oc_color::{self, RGB8},
	braille,
	Image,
};

pub struct VideoConfig {
	pub stream_descs_data: Vec<VideoDescData>,
	pub container_size: Size<usize>,
	pub fill_color: RGB8,
	pub cmds_per_sec: Option<usize>,
}

pub struct VideoDescData {
	pub name: String,
	pub frame_rate: Option<Frac<u16>>,
	pub size: Size<u8>,
	pub source_area: Option<Rect<usize>>,
	pub filter: Option<VideoFilter>,
	pub budget: Option<Budget>,
	pub acceptable_loss: Frac<u64>,
}

pub struct VideoReader<'a, 'b> {
	reader_data: ReaderData<'a, VideoDecoder>,
	scaler: Scaler,
	scaler_buffer: VideoFrame,
	container_size: Size<usize>,
	fill_color: RGB8,
	cmds_per_sec: Option<usize>,
	in_frame_rate: Frac<i64>,
	formatter: oc_color::formatters::HybridFormatter,
	pub encoders: Vec<packet::VideoEncoder<'b>>,
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

impl<'a, 'b> VideoReader<'a, 'b> {
	pub fn new(
		ictx: &FfmpegInput,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		machine: &'b Machine,
		config: VideoConfig,
		muxer: &mut Muxer,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Video)?;

		let reader_data = ReaderData::<'a, VideoDecoder>::new("video", ictx, &stream, multi_progress, range);
		
		let in_frame_rate = Frac::from(stream.rate()).cast::<i64>().inverse();

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
				let (source_area, size) = if let Some(source_area) = stream_data.source_area {
					debug_assert_eq!(source_area.size, stream_data.size.cast() * braille::SIZE);
					(source_area, stream_data.size)
				} else {
					let size = szu::int_div_ceil!(fit_size, braille::SIZE).try_cast().unwrap();
					eprintln!("auto-size-fit: {} (smallest possible resolution for stream that fits largest possible source resolution)", size);
					(Rect {
						pos: Point::ZERO,
						size: size.cast() * braille::SIZE,
					}, size)
				};
				
				let desc = StreamDescriptor::<Descriptor> {
					num_packets: 0,
					rate: stream_data.frame_rate.unwrap_or(in_frame_rate.try_cast::<u16>().unwrap()),
					name: stream_data.name.into(),
					content: Descriptor {
						size,
					},
				};

				let stream_id = muxer.create_stream(desc.clone().into());

				packet::VideoEncoder::new(
					desc,
					stream_id,
					source_area,
					stream_data.filter,
					stream_data.budget,
					&machine,
					stream_data.acceptable_loss,
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
			formatter: oc_color::formatters::HybridFormatter::new(),
			encoders: streams,
			stream_timers,
		})
	}
}

impl<'a, 'b> Reader<'a> for VideoReader<'a, 'b> {
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
				let frame_rate = stream.desc.rate.cast::<i64>();
				let mut emit_count = (delta / frame_rate).into_int_trunc();
				if stream_time_s == 0.into() {
					emit_count = emit_count.max(1);
				}
				*last_frame_time += Frac::from(emit_count) * frame_rate;
				(emit_count > 0).then_some((stream, emit_count))
			}).collect_vec();

		if cfg!(feature = "log") {
			eprintln!();
			eprintln!("emit: {}", if !emit_streams.is_empty() { format!("yes ({})", emit_streams.iter().map(|s| s.1.to_string()).join(",")) } else { "no".to_string() });
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

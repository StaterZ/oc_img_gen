use std::{
	ops::RangeInclusive,
	time::Duration,
};
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use ffmpeg_next::{
	software::scaling::Context as Scaler,
	util::frame::video::Video as VideoFrame,
	codec::{
		Context as CodecCtx,
		decoder::Video as VideoDecoder,
	},
	media::Type as MediaType,
};
use num_traits::ConstZero;

use super::{
	media_container::{DescriptorHeader, SizedString},
	build_progress_style,
	stage,
};
use crate::{math::{Frac, Rect, Size}, LOG};
use crate::video::{
	self,
	cmd::packet::{self, Descriptor as VideoStreamDesc},
	oc_color::{formatters::*, RGB8, PaletteOr},
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
	pub source: Option<Rect<usize>>,
}

pub struct VideoEncoderData {
	pub encoder: packet::VideoEncoder,
	source: Option<Rect<usize>>,
}

pub struct VideoReader<'a> {
	stream_index: usize,
	decoder: VideoDecoder,
	time_base: Frac<i64>,
	receive_buffer: VideoFrame,
	range_ts: RangeInclusive<i64>,
	pub is_done: bool,
	progress: ProgressBar,
	multi_progress: &'a MultiProgress,
	
	scaler: Scaler,
	scaler_buffer: VideoFrame,
	container_size: Size<usize>,
	fill_color: RGB8,
	cmds_per_sec: Option<usize>,
	in_frame_rate: Frac<i64>,
	formatter: video::oc_color::formatters::HybridFormatter,
	pub out_streams: Vec<VideoEncoderData>,
	stream_timers: Vec<Frac<i64>>,
}

impl<'a> VideoReader<'a> {
	pub fn new(
		ictx: &ffmpeg_next::format::context::Input,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		config: VideoConfig,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Video)?;

		let codec_ctx = CodecCtx::from_parameters(stream.parameters())
			.expect("failed to create codec context");

		let decoder = codec_ctx.decoder().video().unwrap();

		let in_frame_rate = Frac::from(stream.rate()).cast::<i64>();

		let time_base = Frac::from(stream.time_base()).cast::<i64>();
		let end_ts = stream.start_time() + stream.duration();
		
		let ms_to_ts = |ms: u128| (Frac::new(ms, 1000) / time_base.try_cast::<u128>().unwrap()).into_int() as i64;
		let range_ts = range.start().map_or(0, |start| ms_to_ts(start.as_millis()))..=range.end().map_or(end_ts, |end| ms_to_ts(end.as_millis()));

		let num_frames = (Frac::new(stream.frames(), stream.duration()) * range_ts.try_len().unwrap() as i64).into_int();

		let progress = multi_progress.add(ProgressBar::new(num_frames as u64)
			.with_style(build_progress_style())
			.with_message("video"));
		if !LOG {
			progress.tick();
		}
	
		
		//setup down-scaler
		let content_size = Size::new(decoder.width() as usize, decoder.height() as usize);
		let fit_size = config.container_size.contain(content_size);

		let scaler = Scaler::get(
			decoder.format(),
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
			.map(|stream_data| VideoEncoderData {
				encoder: packet::VideoEncoder::new({
					let size = if stream_data.source.is_some() {
						stream_data.size
					} else {
						(fit_size / video::braille::SIZE).try_cast().unwrap()
					};
					VideoStreamDesc {
						header: DescriptorHeader {
							num_packets: 0,
							name: stream_data.name,
						},
						frame_rate: stream_data.frame_rate.unwrap_or(in_frame_rate.into_int() as u16),
						size,
					}
				}),
				source: stream_data.source,
			})
			.collect_vec();
		
		let stream_timers = streams.iter().map(|_| Frac::ZERO).collect_vec();
		
		Some(Self {
			stream_index: stream.index(),
			decoder,
			time_base,
			receive_buffer: VideoFrame::empty(),
			range_ts,
			is_done: false,
			progress,
			multi_progress,

			scaler,
			scaler_buffer: VideoFrame::empty(),
			out_streams: streams,
			container_size: config.container_size,
			fill_color: config.fill_color,
			cmds_per_sec: config.cmds_per_sec,
			in_frame_rate,
			formatter: video::oc_color::formatters::HybridFormatter::new(),
			stream_timers,
		})
	}

	fn receive_and_process(&mut self, should_force_emit: bool) {
		let frame = &self.receive_buffer;

		let p = frame.pts().unwrap_or(0);
		if p < *self.range_ts.start() { return; } //await start
		if p > *self.range_ts.end() { //check is we're done yet
			self.is_done = true;
			return;
		}
		let stream_time_s = Frac::from(p - self.range_ts.start()) * self.time_base;
		
		let emit_streams = self.out_streams
			.iter_mut()
			.zip_eq(self.stream_timers.iter_mut())
			.filter_map(|(stream, last_frame_time)| {
				if should_force_emit {
					return Some((stream, 1));
				}

				if stream_time_s < *last_frame_time { return None; }

				let delta = stream_time_s - *last_frame_time;
				let frame_rate = stream.encoder.get_desc().frame_rate as i64;
				let emit_count = (delta * frame_rate).into_int_trunc();
				*last_frame_time += Frac::new(emit_count, frame_rate);
				(emit_count > 0).then_some((stream, emit_count))
			}).collect_vec();

		if LOG {
			println!();
			println!("emit: {}", if !emit_streams.is_empty() { format!("yes ({})", emit_streams.iter().map(|s| s.1.to_string()).join(",")) } else { "no".to_string() });
		}

		if !emit_streams.is_empty() {
			let frame = stage("Frame  | Preamble  | Scale", || {
				self.scaler.run(&frame, &mut self.scaler_buffer).unwrap();
				&self.scaler_buffer
			});
			
			let img = stage("Frame  | Preamble  | Into Image", || Image::from(frame));
			let img = stage("Frame  | Preamble  | Resize", || img.resize(self.container_size, self.fill_color));
			
			let emit_streams_len = emit_streams.len();
			let frame_progress = self.multi_progress.add(ProgressBar::new(emit_streams_len as u64)
				.with_style(build_progress_style())
				.with_message("frame"));
			if !LOG && emit_streams_len > 1 {
				frame_progress.tick();
			}

			for (stream, emit_count) in emit_streams {
				let img = match &stream.source {
					Some(source) => stage("Stream | Preamble  | Crop", || img.crop(source)),
					None => img.clone(),
				};

				// let img = stage("Stream | Process   | Black&White", || img.map(|p| {
				// 	const BLACK: RGB8 = RGB8::new(0x000000);
				// 	const WHITE: RGB8 = RGB8::new(0xffffff);
				// 	if p.perceptual_delta(WHITE) < p.perceptual_delta(BLACK) { WHITE } else { BLACK }
				// }));

				// let img = stage("Stream | Process   | Deflate", || img.map(|p| self.formatter.deflate(PaletteOr::NonPalette(*p))));
				// let img = stage("Stream | Process   | Inflate", || img.map(|p| self.formatter.inflate(*p)));
				
				let img = stage("Stream | Process   | Braille", || video::braille::as_braille(&img));
				let img = stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| self.formatter.deflate(PaletteOr::NonPalette(*p)))));
				for _ in 0..emit_count {
					stage("Stream | Postamble | Cmd Gen", || stream.encoder.push_frame_braille(&img));
					//println!("{}", cmd::code_gen(&img.map(|braille| braille.into()), None, &formatter));
				}

				if !LOG && emit_streams_len > 1 {
					frame_progress.inc(1);
				}
			}
			if !LOG && emit_streams_len > 1 {
				frame_progress.finish_and_clear();
			}
		}
		if !LOG {
			self.progress.inc(1);
		}
	}

	fn process_frame(&mut self, should_force_emit: bool) {
		while self.decoder.receive_frame(&mut self.receive_buffer).is_ok() {
			self.receive_and_process(should_force_emit);
		}
	}

	pub fn try_process_packet(&mut self, stream: &ffmpeg_next::Stream, packet: &ffmpeg_next::Packet) -> bool {
		if stream.index() != self.stream_index { return false; }

		self.decoder.send_packet(packet).unwrap();
		self.process_frame(false);
		true
	}

	pub fn process_eof(&mut self) {
		self.decoder.send_eof().unwrap();
		self.process_frame(true);
	}
}

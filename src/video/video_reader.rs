use std::{ops::RangeInclusive, path::Path, time::Duration};
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
use num::traits::ConstZero;
use palette::Srgb;

use crate::{
	encoder::{
		cli::{BrailleStrategy, Budget, ColorDiffStrategy, VideoFilter}, media_container::Descriptor as TrackDescriptor, muxer::{Muxer, PacketWriter}, reader::{DecoderInterface, FrameInterface, Reader, ReaderData}
	}, math::*, video::cmd::machine::Machine
};

use super::{
	cmd::packet::{self, Descriptor},
	oc_color::self,
	braille,
	Image,
};

pub struct VideoConfig {
	pub track_descs_data: Vec<VideoDescData>,
	pub container_size: Size<usize>,
	pub fill_color: Srgb<u8>,
}

pub struct VideoDescData {
	pub name: String,
	pub frame_rate: Option<Frac<u16>>,
	pub size: Size<u8>,
	pub source_area: Option<Rect<usize>>,
	pub filter: Option<VideoFilter>,
	pub braille_strategy: BrailleStrategy,
	pub color_diff_strategy: ColorDiffStrategy,
	pub budget: Option<Budget>,
	pub acceptable_loss: Frac<u32>,
	pub loss_step: Frac<u32>,
}

pub struct VideoReader<'a, 'b> {
	reader_data: ReaderData<'a, VideoDecoder>,
	scaler: Scaler,
	scaler_buffer: VideoFrame,
	container_size: Size<usize>,
	fill_color: Srgb<u8>,
	in_frame_rate: Frac<i64>,
	formatter: oc_color::formatters::HybridFormatter,
	pub encoders: Vec<packet::VideoEncoder<'b>>,
	track_timers: Vec<Frac<i64>>,
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
		out_path: &Path,
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
			content_size.w as u32,
			content_size.h as u32,
			ffmpeg_next::util::format::Pixel::RGB24,
			fit_size.w as u32,
			fit_size.h as u32,
			ffmpeg_next::software::scaling::Flags::BILINEAR,
		).unwrap();

		
		//setup SZT track encoders
		let tracks = config.track_descs_data
			.into_iter()
			.map(|track_data| {
				let (source_area, size) = if let Some(source_area) = track_data.source_area {
					debug_assert_eq!(source_area.size, track_data.size.cast() * braille::SIZE);
					(source_area, track_data.size)
				} else {
					let size = szu::int_div_ceil!(fit_size, braille::SIZE).try_cast().unwrap();
					eprintln!("auto-size-fit: {} (smallest possible resolution for track that fits largest possible source resolution)", size);
					(Rect {
						pos: (config.container_size - fit_size) / 2,
						size: size * braille::SIZE,
					}, size.cast())
				};
				
				let desc = TrackDescriptor::<Descriptor> {
					num_packets: 0,
					rate: track_data.frame_rate.unwrap_or(in_frame_rate.try_cast::<u16>().unwrap()),
					name: track_data.name.into(),
					stream_id: muxer.packet_stream(),
					content: Descriptor {
						size,
					},
				};

				let track_id = muxer.create_track(desc.clone().into(), desc.stream_id);

				packet::VideoEncoder::new(
					desc,
					track_id,
					source_area,
					track_data.filter,
					track_data.braille_strategy,
					track_data.color_diff_strategy,
					track_data.budget,
					&machine,
					track_data.acceptable_loss,
					track_data.loss_step,
					out_path,
				)
			})
			.collect_vec();
		
		let track_timers = tracks.iter().map(|_| Frac::ZERO).collect_vec();
		
		Some(Self {
			reader_data,
			scaler,
			scaler_buffer: VideoFrame::empty(),
			container_size: config.container_size,
			fill_color: config.fill_color,
			in_frame_rate,
			formatter: oc_color::formatters::HybridFormatter::new(),
			encoders: tracks,
			track_timers,
		})
	}

	//totals over all tracks, the per track breakdown is in the summary
	fn progress_message(&self) -> String {
		if let [encoder] = self.encoders.as_slice() {
			return encoder.progress_message();
		}
		let num_bytes = self.encoders.iter().map(|encoder| encoder.stats.num_bytes).sum::<usize>();
		let num_dropped = self.encoders.iter().map(|encoder| encoder.stats.num_dropped).sum::<usize>();
		format!("{} tracks | {} | {} dropped", self.encoders.len(), crate::format_bytes(num_bytes), num_dropped)
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

	fn report(&self) -> Vec<String> {
		self.encoders.iter().map(|encoder| encoder.report()).collect()
	}

	fn process(&mut self, stream_time_s: Frac<i64>, should_force_emit: bool) {
		let frame = &self.reader_data.receive_buffer;

		let emit_tracks = self.encoders
			.iter_mut()
			.zip_eq(self.track_timers.iter_mut())
			.filter_map(|(track, last_frame_time)| {
				if should_force_emit {
					return Some((track, 1));
				}

				if stream_time_s < *last_frame_time { return None; }

				let delta = stream_time_s - *last_frame_time;
				let frame_rate = track.desc.rate.cast::<i64>();
				let mut emit_count = (delta / frame_rate).into_int_trunc();
				if stream_time_s == 0.into() {
					emit_count = emit_count.max(1);
				}
				*last_frame_time += Frac::from(emit_count) * frame_rate;
				(emit_count > 0).then_some((track, emit_count))
			}).collect_vec();

		if cfg!(feature = "log") {
			eprintln!();
			eprintln!("emit: {}", if !emit_tracks.is_empty() { format!("yes ({})", emit_tracks.iter().map(|s| s.1.to_string()).join(",")) } else { "no".to_string() });
		}

		if emit_tracks.is_empty() { return; }
		
		let frame = crate::stage("Frame  | Preamble  | Scale", || {
			self.scaler.run(frame, &mut self.scaler_buffer).unwrap();
			&self.scaler_buffer
		});
		
		let img = crate::stage("Frame  | Preamble  | Into Image", || Image::from(frame));
		let img = crate::stage("Frame  | Preamble  | Resize", || img.resize(self.container_size, self.fill_color, Point::splat(Frac::half())));
		
		//only worth showing when a frame is split over several tracks
		let emit_tracks_len = emit_tracks.len();
		let frame_progress = (cfg!(not(feature = "log")) && emit_tracks_len > 1).then(|| {
			let frame_progress = self.reader_data.multi_progress.add(ProgressBar::new(emit_tracks_len as u64)
				.with_style(crate::build_progress_style())
				.with_prefix("frame"));
			frame_progress.tick();
			frame_progress
		});

		for (track, emit_count) in emit_tracks {
			for _ in 0..emit_count {
				track.process(&img, &self.formatter);
			}

			if let Some(frame_progress) = &frame_progress {
				frame_progress.inc(1);
			}
		}
		if let Some(frame_progress) = frame_progress {
			frame_progress.finish_and_clear();
		}

		if cfg!(not(feature = "log")) {
			self.reader_data.progress.set_message(self.progress_message());
		}
	}
}

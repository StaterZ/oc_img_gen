use std::{
	ops::RangeInclusive,
	time::Duration,
};
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use ffmpeg_next::{
	codec::Context as CodecCtx,
	util::error::Error as FfmpegError,
	decoder::decoder::Decoder as FfmpegDecoder,
	format::stream::Stream as FfmpegStream,
	format::context::Input as FfmpegInput,
	ffi::AV_NOPTS_VALUE,
};
use num_traits::ConstZero;
use crate::math::*;
use super::muxer::PacketWriter;

pub struct ReaderData<'a, TDecoder: DecoderInterface> {
	stream_index: usize,
	pub decoder: TDecoder,
	time_base: Frac<i64>,
	pub receive_buffer: TDecoder::Frame,
	range_ts: RangeInclusive<i64>,
	pub is_done: bool,
	progress: ProgressBar,
	pub multi_progress: &'a MultiProgress,
}

pub trait DecoderInterface: Sized {
	type Frame: FrameInterface;

	fn new(decoder: FfmpegDecoder) -> Result<Self, FfmpegError>;

	fn receive_frame(&mut self, frame: &mut Self::Frame) -> Result<(), FfmpegError>;
	fn send_eof(&mut self) -> Result<(), FfmpegError>;
	fn send_packet<P: ffmpeg_next::packet::Ref>(&mut self, packet: &P) -> Result<(), FfmpegError>;
}

pub trait FrameInterface: Sized {
	fn empty() -> Self;

	fn pts(&self) -> Option<i64>;
}

impl<'a, TDecoder: DecoderInterface> ReaderData<'a, TDecoder> {
	pub fn new(
		name: &'static str,
		ictx: &FfmpegInput,
		stream: &FfmpegStream,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
	) -> Self {
		let codec_ctx = CodecCtx::from_parameters(stream.parameters())
			.expect("failed to create codec context");

		let decoder = TDecoder::new(codec_ctx.decoder()).unwrap();

		let mut time_base = Frac::from(stream.time_base()).cast::<i64>();
		if time_base == Frac::ZERO { time_base = Frac::from(stream.rate()).cast::<i64>().inverse(); }
		let mut start_ts = stream.start_time();
		if start_ts == AV_NOPTS_VALUE { start_ts = 0; }
		let mut duration_ts = stream.duration();
		if duration_ts == AV_NOPTS_VALUE || duration_ts <= 0 { duration_ts = ictx.duration(); }
		if duration_ts == AV_NOPTS_VALUE || duration_ts <= 0 { duration_ts = 1; }
		let end_ts = start_ts + duration_ts;
		
		let ns_to_ts = |ns: u128| (Frac::new(ns, std::time::Duration::SECOND.as_nanos()) / time_base.try_cast::<u128>().unwrap()).into_int_round() as i64;
		let range_ts = range.start().map_or(start_ts, |start| ns_to_ts(start.as_nanos()))..=range.end().map_or(end_ts, |end| ns_to_ts(end.as_nanos()));

		let num_frames = (Frac::new(stream.frames(), duration_ts) * range_ts.try_len().unwrap() as i64).into_int_round();

		let progress = multi_progress.add(ProgressBar::new(num_frames as u64)
			.with_style(crate::build_progress_style())
			.with_message(name));

		Self {
			stream_index: stream.index(),
			decoder,
			time_base,
			receive_buffer: TDecoder::Frame::empty(),
			range_ts,
			is_done: false,
			progress,
			multi_progress,
		}
	}

	pub fn init(&mut self) {
		if cfg!(not(feature = "log")) {
			self.progress.tick();
		}
	}
}

pub trait Reader<'a> {
	type Decoder: DecoderInterface;

	fn get_data(&self) -> &ReaderData<'a, Self::Decoder>;
	fn get_data_mut(&mut self) -> &mut ReaderData<'a, Self::Decoder>;

	fn is_done(&self) -> bool {
		self.get_data().is_done
	}

	fn init(&mut self) {
		self.get_data_mut().init();
	}

	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter>;

	fn process(&mut self, stream_time_s: Frac<i64>, should_force_emit: bool);

	fn receive_and_process(&mut self, should_force_emit: bool) {
		let frame = &self.get_data().receive_buffer;

		let p = frame.pts().unwrap_or(0);
		if p < *self.get_data().range_ts.start() { return; } //await start
		if p > *self.get_data().range_ts.end() { //check if we're done yet
			self.get_data_mut().is_done = true;
			return;
		}
		let stream_time_s = Frac::from(p - self.get_data().range_ts.start()) * self.get_data().time_base;
		
		self.process(stream_time_s, should_force_emit);
		
		if cfg!(not(feature = "log")) {
			self.get_data().progress.inc(1);
		}
	}

	fn process_frame(&mut self, should_force_emit: bool) {
		while {
			let reader_data = self.get_data_mut(); //FUCK YOU RUST!!!!
			reader_data.decoder.receive_frame(&mut reader_data.receive_buffer).is_ok()
		} {
			self.receive_and_process(should_force_emit);
		}
	}

	fn try_process_packet(&mut self, stream: &ffmpeg_next::Stream, packet: &ffmpeg_next::Packet) -> bool {
		if stream.index() != self.get_data().stream_index { return false; }

		self.get_data_mut().decoder.send_packet(packet).unwrap();
		self.process_frame(false);
		true
	}

	fn process_eof(&mut self) {
		self.get_data_mut().decoder.send_eof().unwrap();
		self.process_frame(true);
	}
}

pub trait CommonReader {
	fn is_done(&self) -> bool;
	fn init(&mut self);
	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter>;
	fn try_process_packet(&mut self, stream: &ffmpeg_next::Stream, packet: &ffmpeg_next::Packet) -> bool;
	fn process_eof(&mut self);
}

impl<'a, T: Reader<'a>> CommonReader for T {
	fn is_done(&self) -> bool {
		Reader::is_done(self)
	}

	fn init(&mut self) {
		Reader::init(self)
	}

	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter> {
		Reader::get_writers(self)
	}

	fn try_process_packet(&mut self, stream: &ffmpeg_next::Stream, packet: &ffmpeg_next::Packet) -> bool {
		Reader::try_process_packet(self, stream, packet)
	}

	fn process_eof(&mut self) {
		Reader::process_eof(self)
	}
}

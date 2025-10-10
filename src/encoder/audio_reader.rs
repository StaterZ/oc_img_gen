use std::{
	ops::RangeInclusive,
	time::Duration,
};
use indicatif::{MultiProgress, ProgressBar};
use itertools::Itertools;
use ffmpeg_next::{
	codec::{
		decoder::Audio as AudioDecoder,
		Context as CodecCtx,
	},
	media::Type as MediaType,
	software::resampling::Context as Resampler,
	util::{
		format::Sample as SampleFormat,
		frame::audio::Audio as AudioFrame,
	}
};

use crate::{math::Frac, LOG};
use crate::audio::{
	packet::{
		Descriptor as AudioStreamDesc,
		AudioEncoder,
	},
	Config as AudioConfig,
	encode as encode_audio,
};
use super::{
	media_container::{DescriptorHeader, SizedString},
	build_progress_style,
	stage,
};

pub struct AudioReader<'a> {	
	stream_index: usize,
	decoder: AudioDecoder,
	receive_buffer: AudioFrame,
	range_ts: RangeInclusive<i64>,
	pub is_done: bool,
	multi_progress: &'a MultiProgress,
	progress: ProgressBar,

	config: AudioConfig,
	resampler: Resampler,
	resampler_buffer: AudioFrame,
	pcm: Vec<f32>,
}

impl<'a> AudioReader<'a> {
	pub fn new(
		ictx: &ffmpeg_next::format::context::Input,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		config: AudioConfig,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Audio)?;

		let codec_ctx = CodecCtx::from_parameters(stream.parameters())
			.expect("failed to create codec context");

		let decoder = codec_ctx.decoder().audio().unwrap();

		let tb = Frac::from(stream.time_base()).cast::<i64>();
		let end_ts = stream.start_time() + stream.duration();
		
		let ms_to_ts = |ms: u128, tb: Frac<i64>| (Frac::new(ms, 1000) / tb.try_cast::<u128>().unwrap()).into_int() as i64;
		let range_ts = range.start().map_or(0, |start| ms_to_ts(start.as_millis(), tb))..=range.end().map_or(end_ts, |end| ms_to_ts(end.as_millis(), tb));

		let num_frames = (Frac::new(stream.frames(), stream.duration()) * range_ts.try_len().unwrap() as i64).into_int();

		let progress = multi_progress.add(ProgressBar::new(num_frames as u64)
			.with_style(build_progress_style())
			.with_message("audio"));
		if !LOG {
			progress.tick();
		}
		
		// Setup resampler to f32 planar @ cli.rate, mono
		let in_fmt = decoder.format();
		let in_ch = decoder.channel_layout();
		let in_rate = decoder.rate();

		let target_rate = 22050; // Target analysis sample rate (Hz)
		let target_layout = ffmpeg_next::channel_layout::ChannelLayout::MONO;
		let target_fmt = SampleFormat::F32(ffmpeg_next::format::sample::Type::Planar); // f32

		let resampler = Resampler::get(
			in_fmt, in_ch, in_rate,
			target_fmt, target_layout, target_rate,
		).unwrap();
		
		
		Some(Self {
			stream_index: stream.index(),
			decoder,
			receive_buffer: AudioFrame::empty(),
			range_ts,
			is_done: false,
			progress,
			multi_progress,

			config,
			resampler,
			resampler_buffer: AudioFrame::empty(),
			pcm: Vec::new(),
		})
	}

	fn receive_and_process(&mut self) {
		let frame = &self.receive_buffer;

		let p = frame.pts().unwrap_or(0);
		if p < *self.range_ts.start() { return; } //await start
		if p > *self.range_ts.end() { //check is we're done yet
			self.is_done = true;
			return;
		}
		
		let frame = stage("Audio  | Preamble  | Resample", || {
			self.resampler.run(&frame, &mut self.resampler_buffer).unwrap();
			&mut self.resampler_buffer
		});

		let nb_samples = frame.samples() as usize;
		let data = frame.data(0);
		let slice: &[f32] = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, nb_samples) }; //SAFETY: interpret as f32 slice
		self.pcm.extend_from_slice(slice);

		if !LOG {
			self.progress.inc(1);
		}
	}

	fn process_frame(&mut self) {
		while self.decoder.receive_frame(&mut self.receive_buffer).is_ok() {
			self.receive_and_process();
		}
	}

	pub fn try_process_packet(&mut self, stream: &ffmpeg_next::Stream, packet: &ffmpeg_next::Packet) -> bool {
		if stream.index() != self.stream_index { return false; }

		self.decoder.send_packet(packet).unwrap();
		self.process_frame();
		true
	}

	pub fn process_eof(&mut self) {
		self.decoder.send_eof().unwrap();
		self.process_frame();
	}

	pub fn encode(self) -> AudioEncoder {
		let samples = encode_audio(&self.config, &self.pcm);
		AudioEncoder {
			desc: AudioStreamDesc {
				header: DescriptorHeader { num_packets: samples.len() as u32, name: SizedString::new("main").unwrap() },
				num_voices: self.config.num_voices as u8,
			},
			samples,
		}
	}
}

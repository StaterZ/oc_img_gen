use std::{ops::RangeInclusive, time::Duration};
use indicatif::MultiProgress;
use ffmpeg_next::{
	codec::decoder::Audio as AudioDecoder,
	media::Type as MediaType,
	software::resampling::Context as Resampler,
	util::{
		format::Sample as SampleFormat,
		frame::audio::Audio as AudioFrame,
	},
	format::context::Input as FfmpegInput,
};

use crate::{
	encoder::{
		media_container::Descriptor as TrackDescriptor,
		muxer::{Muxer, PacketWriter},
		reader::{Reader, ReaderData},
	}, math::Frac,
};

use super::{
	Config as AudioConfig,
	dfpwm,
	packet::PcmDescriptor,
};

/// Resamples the audio for a tape drive and encodes it to one DFPWM block at eof.
pub struct PcmReader<'a> {
	reader_data: ReaderData<'a, AudioDecoder>,
	resampler: Resampler,
	resampler_buffer: AudioFrame,
	config: AudioConfig,
	track_id: u8,
	pcm: Vec<f32>,
	peak: f32,
	data: Option<Vec<u8>>,
	num_bytes: usize,
}

impl<'a> PcmReader<'a> {
	pub fn new(
		ictx: &FfmpegInput,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		config: AudioConfig,
		muxer: &mut Muxer,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Audio)?;

		let reader_data = ReaderData::<'a, AudioDecoder>::new("pcm", ictx, &stream, multi_progress, range);

		let resampler = Resampler::get(
			reader_data.decoder.format(),
			reader_data.decoder.channel_layout(),
			reader_data.decoder.rate(),
			SampleFormat::F32(ffmpeg_next::format::sample::Type::Planar),
			ffmpeg_next::channel_layout::ChannelLayout::MONO,
			config.pcm_rate,
		).unwrap();

		let desc = TrackDescriptor::<PcmDescriptor> {
			num_packets: 0,
			rate: Frac::new(1, config.pcm_rate / 8).try_cast::<u16>().unwrap(), //seconds per byte
			name: config.name.clone().into(),
			stream_id: 0,
			content: PcmDescriptor {
				sample_rate: config.pcm_rate,
				checksum: 0,
			},
		};
		let stream_id = muxer.create_pcm_stream(config.pcm_align);
		let track_id = muxer.create_track(desc.into(), stream_id);

		Some(Self {
			reader_data,
			resampler,
			resampler_buffer: AudioFrame::empty(),
			config,
			track_id,
			pcm: Vec::new(),
			peak: 0.0,
			data: None,
			num_bytes: 0,
		})
	}
}

impl PcmReader<'_> {
	fn push_resampled(&mut self) {
		let nb_samples = self.resampler_buffer.samples();
		let data = self.resampler_buffer.data(0);
		let slice = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, nb_samples) }; //SAFETY: interpret as f32 slice
		for sample in slice {
			self.peak = self.peak.max(sample.abs());
		}
		self.pcm.extend(slice);
	}
}

impl<'a> Reader<'a> for PcmReader<'a> {
	type Decoder = AudioDecoder;

	fn get_data(&self) -> &ReaderData<'a, Self::Decoder> {
		&self.reader_data
	}
	fn get_data_mut(&mut self) -> &mut ReaderData<'a, Self::Decoder> {
		&mut self.reader_data
	}

	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter> {
		Vec::new()
	}

	fn take_pcm(&mut self) -> Option<(u8, Vec<u8>)> {
		self.data.take().map(|data| (self.track_id, data))
	}

	fn process(&mut self, _stream_time_s: Frac<i64>, _should_force_emit: bool) {
		let frame = &self.reader_data.receive_buffer;
		self.resampler.run(frame, &mut self.resampler_buffer).unwrap();
		self.push_resampled();
	}

	fn on_eof(&mut self) {
		//the resampler holds back the tail
		while self.resampler.flush(&mut self.resampler_buffer).unwrap().is_some() && self.resampler_buffer.samples() > 0 {
			self.push_resampled();
		}

		//DFPWM has 1 bit per sample, so use the whole 8 bit range it encodes from
		let gain = if self.config.normalize { 1.0 / self.peak.max(1e-9) } else { 1.0 };
		let samples = self.pcm
			.drain(..)
			.map(|sample| (sample * gain * 127.0).round().clamp(-128.0, 127.0) as i8)
			.collect::<Vec<_>>();
		let data = dfpwm::encode(&samples);
		self.num_bytes = data.len();
		self.data = Some(data);
	}

	fn report(&self) -> Vec<String> {
		let duration = Duration::from_secs_f64(self.num_bytes as f64 * 8.0 / self.config.pcm_rate as f64);
		vec![format!("{:>6} pcm @ {}Hz (tape speed {:.2}) | {} | {} | peak {:.3}",
			self.config.name,
			self.config.pcm_rate,
			dfpwm::tape_speed(self.config.pcm_rate),
			humantime::Duration::from(crate::playback::clean_duration(duration)),
			crate::format_bytes(self.num_bytes),
			self.peak,
		)]
	}
}

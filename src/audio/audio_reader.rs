use std::{cmp::Ordering, collections::VecDeque, ops::RangeInclusive, sync::Arc, time::Duration};
use grid::Grid;
use indicatif::MultiProgress;
use ffmpeg_next::{
	codec::decoder::Audio as AudioDecoder,
	media::Type as MediaType,
	software::resampling::Context as Resampler,
	util::{
		format::Sample as SampleFormat,
		frame::audio::Audio as AudioFrame,
		error::Error as FfmpegError,
	},
	format::context::Input as FfmpegInput,
};
use itertools::Itertools;
use num::Complex;
use realfft::{RealFftPlanner, RealToComplex};

use crate::{
	audio::VoiceStateFlt, encoder::{
		media_container::Descriptor as StreamDescriptor,
		muxer::{Muxer, PacketWriter},
		reader::{DecoderInterface, FrameInterface, Reader, ReaderData},
	}, math::Frac,
};

use super::{
	Config as AudioConfig,
	encode as encode_audio,
	optimize_channel_jumping,
	packet::{AudioEncoder, Descriptor},
};

pub struct AudioReader<'a> {
	reader_data: ReaderData<'a, AudioDecoder>,
	resampler: Resampler,
	resampler_buffer: AudioFrame,
	config: AudioConfig,
	encoder: AudioEncoder,
	pcm: VecDeque<f32>,
	fft_planner: RealFftPlanner<f32>,
	r2c: Arc<dyn RealToComplex<f32>>,
	fft_in: Vec<f32>,
	fft_out: Vec<Complex<f32>>,
	fft_window: Vec<f32>,
	global_peak: f32,
	timeline: Grid<VoiceStateFlt>,
}

impl DecoderInterface for AudioDecoder {
	type Frame = AudioFrame;
	
	fn new(decoder: ffmpeg_next::decoder::decoder::Decoder) -> Result<Self, FfmpegError> {
		decoder.audio()
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

impl FrameInterface for AudioFrame {
	fn empty() -> Self {
		AudioFrame::empty()
	}

	fn pts(&self) -> Option<i64> {
		ffmpeg_next::Frame::pts(self)
	}
}

impl<'a> AudioReader<'a> {
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

		let reader_data = ReaderData::<'a, AudioDecoder>::new("audio", ictx, &stream, multi_progress, range);
		
		// Setup resampler to f32 planar @ config.analysis_rate, mono
		let resampler = Resampler::get(
			reader_data.decoder.format(),
			reader_data.decoder.channel_layout(),
			reader_data.decoder.rate(),
			SampleFormat::F32(ffmpeg_next::format::sample::Type::Planar),
			ffmpeg_next::channel_layout::ChannelLayout::MONO,
			config.analysis_rate,
		).unwrap();

		let desc = StreamDescriptor::<Descriptor> {
			num_packets: 0,
			rate: Frac::new(config.hop_length, config.analysis_rate as usize).try_cast::<u16>().unwrap(),
			name: config.name.clone().into(),
			content: Descriptor {
				num_voices: config.num_voices as u8,
			},
		};
		
		let stream_id = muxer.create_stream(desc.clone().into());
		
		let encoder = AudioEncoder::new(desc, stream_id);
		
		// Prepare FFT
		let mut fft_planner = RealFftPlanner::<f32>::new();
		let r2c = fft_planner.plan_fft_forward(config.fft_window_size);
		let fft_in = r2c.make_input_vec();
		let fft_out = r2c.make_output_vec();
		let fft_window = (0..config.fft_window_size) //Hann
			.map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / config.fft_window_size as f32).cos())
			.collect();

		let timeline = Grid::<VoiceStateFlt>::new_with_order(
			0,
			config.num_voices,
			grid::Order::RowMajor
		);

		Some(Self {
			reader_data,
			resampler,
			resampler_buffer: AudioFrame::empty(),
			config,
			encoder,
			pcm: VecDeque::new(),
			fft_planner,
			r2c,
			fft_in,
			fft_out,
			fft_window,
			global_peak: 0.0,
			timeline,
		})
	}
}


impl<'a> Reader<'a> for AudioReader<'a> {
	type Decoder = AudioDecoder;
	
	fn get_data(&self) -> &ReaderData<'a, Self::Decoder> {
		&self.reader_data
	}
	fn get_data_mut(&mut self) -> &mut ReaderData<'a, Self::Decoder> {
		&mut self.reader_data
	}

	fn get_writers(&mut self) -> Vec<&mut dyn PacketWriter> {
		vec![&mut self.encoder as &mut dyn PacketWriter]
	}

	fn process(&mut self, _stream_time_s: Frac<i64>, should_force_emit: bool) {
		let frame = &self.reader_data.receive_buffer;
		
		let frame = crate::stage("Audio  | Preamble  | Resample", || {
			self.resampler.run(&frame, &mut self.resampler_buffer).unwrap();
			&mut self.resampler_buffer
		});

		let nb_samples = frame.samples() as usize;
		let data = frame.data(0);
		let slice = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, nb_samples) }; //SAFETY: interpret as f32 slice
		self.pcm.extend(slice);

		if should_force_emit {
			//TODO: stupid shit, remove me
			let last_full_start = ((self.pcm.len() - 1) / self.config.hop_length) * self.config.hop_length;
			let needed_len = last_full_start + self.config.fft_window_size;
			if needed_len > self.pcm.len() {
				self.pcm.resize(needed_len, 0.0);
			}
		}
		
		crate::stage("Audio  | Preamble  | FFT", || {
			// Window/Hop analysis
			while self.pcm.len() >= self.config.fft_window_size {
				// Copy & window (Hann)
				for (i, (sample, w)) in self.pcm.iter().zip(&self.fft_window).enumerate() {
					self.fft_in[i] = sample * w;
				}
				self.r2c.process(&mut self.fft_in, &mut self.fft_out).unwrap();

				// Magnitudes up to Nyquist
				let bin_hz = self.config.analysis_rate as f32 / self.config.fft_window_size as f32;
				let mut peaks = self.fft_out
					.iter()
					.enumerate()
					.map(|(k, c)| VoiceStateFlt {
						volume: (c.re * c.re + c.im * c.im).sqrt(),
						frequency: k as f32 * bin_hz,
					})
					.collect_vec();
				
				// Ignore DC (k=0)
				if !peaks.is_empty() {
					peaks[0].volume = 0.0;
				}

				// Pick top-k non-overlapping peaks (greedy)
				peaks.sort_by(|a, b| b.volume.partial_cmp(&a.volume).unwrap_or(Ordering::Equal));
				let mut chosen = Vec::with_capacity(self.config.num_voices);
				let mut used_bins = vec![false; self.fft_out.len()];
				for peak in peaks.into_iter() {
					if chosen.len() >= self.config.num_voices { break; }

					let bin = (peak.frequency / bin_hz).round() as isize;
					let mut ok = true;
					for off in -self.config.guard..=self.config.guard {
						let idx = bin + off;
						if idx >= 0 && (idx as usize) < used_bins.len() && used_bins[idx as usize] {
							ok = false;
							break;
						}
					}
					if ok {
						if bin >= 0 && (bin as usize) < used_bins.len() {
							used_bins[bin as usize] = true;
						}
						chosen.push(peak);
					}
				}

				// Track global peak for later normalization
				for local_peak in chosen.iter() {
					self.global_peak = self.global_peak.max(local_peak.volume);
				}

				self.timeline.push_row(chosen);
				let _ = self.pcm.drain(..self.config.hop_length); //pop!
			}
		});
	}

	fn on_eof(&mut self) {
		optimize_channel_jumping(&mut self.timeline);
		let master_volume = if self.config.normalize { 1.0 / self.global_peak.max(1e-9) } else { 1.0 };
		self.encoder.samples = encode_audio(master_volume, &self.timeline).into();
	}

	fn report(&self) -> Vec<String> {
		let num_samples = self.encoder.samples.len() + self.encoder.desc.num_packets as usize;
		let num_bytes = num_samples * (1 + self.config.num_voices * 3); //stream id, then volume u8 & frequency u16 per voice
		vec![format!("{:>6} {} voices @ {:.2}Hz | {} samples | {} ({}/sample) | peak {:.3}",
			self.config.name,
			self.config.num_voices,
			Frac::new(self.config.analysis_rate as usize, self.config.hop_length).into_flt::<f32>(),
			num_samples,
			crate::format_bytes(num_bytes),
			crate::format_bytes(1 + self.config.num_voices * 3),
			self.global_peak,
		)]
	}
}

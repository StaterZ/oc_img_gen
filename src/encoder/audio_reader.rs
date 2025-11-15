use std::{ops::RangeInclusive, time::Duration};
use indicatif::MultiProgress;
use ffmpeg_next::{
	codec::decoder::Audio as AudioDecoder,
	media::Type as MediaType,
	software::resampling::Context as Resampler,
	util::{
		format::Sample as SampleFormat,
		frame::audio::Audio as AudioFrame,
		error::Error as FfmpegError,
	}
};

use crate::{
	math::Frac,
	audio::{
		Config as AudioConfig,
		encode as encode_audio,
		packet::{AudioEncoder, Descriptor},
	},
};
use super::{
	media_container::Descriptor as StreamDescriptor,
	reader::{DecoderInterface, FrameInterface, Reader, ReaderData},
	muxer::{Muxer, PacketWriter},
};

pub struct AudioReader<'a> {
	reader_data: ReaderData<'a, AudioDecoder>,
	resampler: Resampler,
	resampler_buffer: AudioFrame,
	config: AudioConfig,
	encoder: AudioEncoder,
	pcm: Vec<f32>,
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
		ictx: &ffmpeg_next::format::context::Input,
		multi_progress: &'a MultiProgress,
		range: &RangeInclusive<Option<Duration>>,
		config: AudioConfig,
		muxer: &mut Muxer,
	) -> Option<Self> {
		let stream = ictx
			.streams()
			.best(MediaType::Audio)?;

		let reader_data = ReaderData::<'a, AudioDecoder>::new("audio", &stream, multi_progress, range);
		
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

		Some(Self {
			reader_data,
			resampler,
			resampler_buffer: AudioFrame::empty(),
			config,
			encoder,
			pcm: Vec::new(),
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

	fn process(&mut self, _stream_time_s: Frac<i64>, _should_force_emit: bool) {
		let frame = &self.reader_data.receive_buffer;
		
		let frame = crate::stage("Audio  | Preamble  | Resample", || {
			self.resampler.run(&frame, &mut self.resampler_buffer).unwrap();
			&mut self.resampler_buffer
		});

		let nb_samples = frame.samples() as usize;
		let data = frame.data(0);
		let slice: &[f32] = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const f32, nb_samples) }; //SAFETY: interpret as f32 slice
		self.pcm.extend_from_slice(slice);
	}

	fn process_eof(&mut self) { //remove me later
		//TODO: move to CommonReader
		self.get_data_mut().decoder.send_eof().unwrap();
		self.process_frame(true);
		
		self.encoder.samples = encode_audio(&self.config, &self.pcm).into(); //TODO: this is just so terrible...
	}
}

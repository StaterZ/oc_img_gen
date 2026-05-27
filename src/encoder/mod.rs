use std::{ops::RangeInclusive, path::PathBuf, time::Duration};
use indicatif::MultiProgress;
use deku::prelude::*;
use itertools::Itertools;
// use tracing_indicatif::IndicatifLayer;
// use tracing_subscriber::prelude::*;

use crate::{math::*, video::cmd::machine::Machine};
pub use crate::video::video_reader::{VideoConfig, VideoReader, VideoDescData};
use crate::audio::{Config as AudioConfig, audio_reader::AudioReader};
use muxer::Muxer;
use reader::CommonReader;


pub mod cli;
pub mod media_container;
pub mod muxer;
pub mod reader;
//mod ffmpeg_tracing;

#[cfg(feature = "debug-mode")]
mod test;

pub struct EncoderConfig {
	pub in_path: PathBuf,
	pub out_path: PathBuf,

	pub range: RangeInclusive<Option<Duration>>,

	pub machine: Machine,
	pub video: Option<VideoConfig>,
	pub audio: Option<AudioConfig>,
}

pub fn encode(config: EncoderConfig) -> anyhow::Result<()> {
	ffmpeg_next::init().map_err(AppError::FfmpegInitFailed)?;
	ffmpeg_next::util::log::set_level(ffmpeg_next::log::Level::Quiet);
	//ffmpeg_tracing::install();

	let mut ictx = ffmpeg_next::format::input(&config.in_path).expect("failed to create decoder");
	let multi_progress = MultiProgress::new();
	
	let mut muxer = Muxer::new();
	let video = config.video.and_then(|video_config| VideoReader::new(&ictx, &multi_progress, &config.range, &config.machine, video_config, &mut muxer));
	let audio = config.audio.and_then(|audio_config| AudioReader::new(&ictx, &multi_progress, &config.range, audio_config, &mut muxer));
	
	let mut readers = Vec::<Box<dyn CommonReader>>::new();
	if let Some(video) = video {
		readers.push(Box::new(video));
	}
	if let Some(audio) = audio {
		readers.push(Box::new(audio));
	}

	for reader in readers.iter_mut() {
		reader.init();
	}
	
	for (stream, packet) in ictx.packets() {
		if readers.iter().all(|reader | reader.is_done()) { break; }

		if readers
			.iter_mut()
			.any(|reader| reader.try_process_packet(&stream, &packet))
		{
			muxer.process(readers
				.iter_mut()
				.flat_map(|r| r.get_writers())
				.collect_vec()
				.as_mut_slice()); //TODO: PERF
		}
	}
	for reader in readers.iter_mut() {
		reader.process_eof();
	}
	let media_file = muxer.process_eof(readers
		.iter_mut()
		.flat_map(|r| r.get_writers())
		.collect_vec()
		.as_mut_slice()); //TODO: PERF
	
	if cfg!(feature = "log") {
		println!();
	}
	crate::stage("Postamble  | Writing...  ", || std::fs::write(&config.out_path, media_file.to_bytes().unwrap())
		.map_err(AppError::WriteFailed))?;

	println!("All Done! saved to: {}", config.out_path.display());
	Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum AppError {
	#[error("Failed to init FFMPEG. INNER: {0}")]
	FfmpegInitFailed(ffmpeg_next::Error),
	#[error("Failed to write output file. INNER: {0}")]
	WriteFailed(std::io::Error),

	#[error("stream name '{name}' ({{name.as_bytes().len()}} bytes) is too long. max length is: {max_length}")]
	StreamNameTooLong {
		name: String,
		max_length: usize,
	},
	#[error("stream size '{size}' is too large. max is: {max_size}")]
	StreamSizeTooLarge {
		size: Size<usize>,
		max_size: Size<usize>,
	}
}

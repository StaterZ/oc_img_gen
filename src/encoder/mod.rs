use std::{ops::RangeInclusive, path::PathBuf, time::Duration};
use indicatif::MultiProgress;
use deku::prelude::*;
use itertools::Itertools;

pub use crate::audio::Config as AudioConfig;
use crate::AppError;
use muxer::Muxer;
use reader::CommonReader;
pub use video_reader::{VideoConfig, VideoReader, VideoStreamDescData};
use audio_reader::AudioReader;

pub mod media_container;
pub mod muxer;
mod reader;
mod video_reader;
mod audio_reader;

pub struct EncoderConfig {
	pub in_path: PathBuf,
	pub out_path: PathBuf,

	pub range: RangeInclusive<Option<Duration>>,

	pub video: Option<VideoConfig>,
	pub audio: Option<AudioConfig>,
}

pub fn encode(config: EncoderConfig) -> anyhow::Result<()> {
	let mut ictx = ffmpeg_next::format::input(&config.in_path).expect("failed to create decoder");
	let multi_progress = MultiProgress::new();
	
	let mut muxer = Muxer::new();
	let video = config.video.and_then(|video_config| VideoReader::new(&ictx, &multi_progress, &config.range, video_config, &mut muxer));
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

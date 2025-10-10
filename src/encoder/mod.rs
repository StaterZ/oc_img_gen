use std::{
	io::Write,
	path::PathBuf,
	ops::RangeInclusive,
	time::Duration,
};
use indicatif::{MultiProgress, ProgressStyle};
use stopwatch::Stopwatch;
use szu::flush_print;

pub use crate::audio::Config as AudioConfig;
use crate::{AppError, LOG};
use media_container::MediaFile;
pub use video_reader::{VideoConfig, VideoReader, VideoStreamDescData};
pub use audio_reader::AudioReader;

pub mod media_container;
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
	
	let mut video = config.video.and_then(|video_config| VideoReader::new(&ictx, &multi_progress, &config.range, video_config));
	let mut audio = config.audio.and_then(|audio_config| AudioReader::new(&ictx, &multi_progress, &config.range, audio_config));
	for (stream, packet) in ictx.packets() {
		if
			video.as_ref().map_or(true, |video| video.is_done) &&
			audio.as_ref().map_or(true, |audio| audio.is_done)
		{ break; }

		if let Some(video) = video.as_mut() {
			if video.try_process_packet(&stream, &packet) { continue; }
		}
		if let Some(audio) = audio.as_mut() {
			if audio.try_process_packet(&stream, &packet) { continue; }
		}
	}
	if let Some(video) = video.as_mut() {
		video.process_eof();
	}
	if let Some(audio) = audio.as_mut() {
		audio.process_eof();
	}

	let mut media_file = MediaFile::new();
	if let Some(video) = video {
		for stream in video.out_streams {
			stream.encoder.attach(&mut media_file);
		}
	}
	if let Some(audio) = audio {
		audio.encode().attach(&mut media_file);
	}

	if LOG {
		println!();
	}
	stage("Postamble  | Writing...  ", || std::fs::write(&config.out_path, media_file.finalize())
		.map_err(AppError::WriteFailed))?;

	println!("All Done! saved to: {}", config.out_path.display());
	Ok(())
}

fn build_progress_style() -> ProgressStyle {
	ProgressStyle::with_template("{msg} [{bar}] {pos}/{len} {eta}")
		.unwrap()
		.progress_chars("█▉▊▋▌▍▎▏ ")
}

fn stage<B>(title: &str, f: impl FnOnce() -> B) -> B {
	if !LOG {
		f()
	} else {
		flush_print!("{}", title);
		let mut timer = Stopwatch::start_new();
		let output = f();
		timer.stop();
		println!(" time: {}ms", timer.elapsed().as_millis());
		output
	}
}

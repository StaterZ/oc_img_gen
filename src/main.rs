#![feature(iter_array_chunks)]
#![feature(anonymous_lifetime_in_impl_trait)]
#![feature(const_for)]
#![feature(const_range_bounds)]
#![feature(adt_const_params)]
#![feature(duration_constants)]
#![feature(const_trait_impl)]
#![feature(exact_length_collection)]
#![feature(exact_size_is_empty)]
#![feature(stmt_expr_attributes)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]

use std::io::Write;
use indicatif::ProgressStyle;
use stopwatch::Stopwatch;
// use tracing_indicatif::IndicatifLayer;
// use tracing_subscriber::prelude::*;
use szu::flush_print;

use math::Size;

mod video;
mod audio;
mod math;
mod encoder;
mod cli;
//mod ffmpeg_tracing;

#[cfg(feature = "debug-mode")]
mod test;

const EXT: &str = "szt";
const FORMAT_VERSION: u16 = 5;

fn main() -> anyhow::Result<()> {
	#[cfg(feature = "debug-mode")]
	return test::run();
	
	// let indicatif_layer = IndicatifLayer::new();
	// tracing_subscriber::registry()
	// 	.with(tracing_subscriber::EnvFilter::new("info"))
	// 	.with(tracing_subscriber::fmt::layer().with_writer(indicatif_layer.get_stderr_writer()))
	// 	.with(indicatif_layer)
	// 	.init();

	ffmpeg_next::init().map_err(AppError::FfmpegInitFailed)?;
	ffmpeg_next::util::log::set_level(ffmpeg_next::log::Level::Quiet);
	//ffmpeg_tracing::install();

	let args = cli::parse_args();

	let mut watch = Stopwatch::start_new();
	let result = encoder::encode(args);
	watch.stop();
	eprintln!("took: {}s & {}ms", watch.elapsed().as_secs(), watch.elapsed().subsec_millis());
	result
}

pub fn stage<B>(title: &str, f: impl FnOnce() -> B) -> B {
	if cfg!(feature = "log") {
		flush_print!("{}", title);
		let mut timer = Stopwatch::start_new();
		let output = f();
		timer.stop();
		eprintln!(" time: {}ms", timer.elapsed().as_millis());
		output
	} else {
		f()
	}
}

fn build_progress_style() -> ProgressStyle {
	ProgressStyle::with_template("{msg} [{bar}] {pos}/{len} {eta}")
		.unwrap()
		.progress_chars("█▉▊▋▌▍▎▏ ")
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

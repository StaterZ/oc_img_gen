#![feature(
	iter_array_chunks,
	array_chunks,
	anonymous_lifetime_in_impl_trait,
	const_for,
	const_range_bounds,
	adt_const_params,
)]
#![allow(dead_code)]


use math::Size;

mod video;
mod audio;
mod math;
mod encoder;
mod cli;

#[cfg(feature = "debug-mode")]
mod test;

const LOG: bool = false;
const EXT: &str = "szt";
const FORMAT_VERSION: u16 = 4;

fn main() -> anyhow::Result<()> {
	#[cfg(feature = "debug-mode")]
	return test::run();

	ffmpeg_next::init().map_err(AppError::FfmpegInitFailed)?;

	let args = cli::parse_args();

	encoder::encode(args)
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

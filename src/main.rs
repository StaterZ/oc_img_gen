#![feature(iter_array_chunks)]
#![feature(adt_const_params)]
#![feature(duration_constants)]
#![feature(const_trait_impl)]
#![feature(try_blocks)]
#![allow(unsafe_op_in_unsafe_fn)]
#![allow(dead_code)]
#![allow(incomplete_features)]
#![allow(unused_features)]

use std::io::Write;
use std::time::Instant;
use indicatif::ProgressStyle;
use szu::flush_print;
use clap::{Parser, Subcommand};

mod video;
mod audio;
mod math;
mod encoder;
mod playback;

const EXT: &str = "szt";
const FORMAT_VERSION: u16 = 6;

#[derive(Parser, Debug)]
#[command(author, version)]
pub struct Cli {
	#[command(subcommand)]
	command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
	Encode(encoder::cli::Cli),
	Play(playback::player::Cli),
	Export(playback::exporter::Cli),
}

fn main() -> anyhow::Result<()> {
	let args = Cli::parse();
	
	let start = Instant::now();
	match args.command {
		CliCommand::Encode(args) => encoder::encode(encoder::cli::process_args(args))?,
		CliCommand::Play(args) => playback::player::play(args)?,
		CliCommand::Export(args) => playback::exporter::export(args)?,
	};
	let e = start.elapsed();
	eprintln!("took: {}s & {}ms", e.as_secs(), e.subsec_millis());
	Ok(())
}

pub fn stage<B>(title: &str, f: impl FnOnce() -> B) -> B {
	if cfg!(feature = "log") {
		flush_print!("{:<32}", title);
		let start = Instant::now();
		let output = f();
		let e = start.elapsed();
		eprintln!(" time: {}.{}ms", e.as_millis(), e.as_micros() - e.as_millis() * 1000);
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

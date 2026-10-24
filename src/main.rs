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
use indicatif::{ProgressState, ProgressStyle};
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
		eprintln!(" time: {:.3}ms", start.elapsed().as_secs_f64() * 1000.0);
		output
	} else {
		f()
	}
}

pub fn format_bytes(num_bytes: usize) -> String {
	const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
	let mut value = num_bytes as f64;
	let mut unit = 0;
	while value >= 1024.0 && unit < UNITS.len() - 1 {
		value /= 1024.0;
		unit += 1;
	}
	if unit == 0 { format!("{}B", num_bytes) } else { format!("{:.1}{}", value, UNITS[unit]) }
}

fn build_progress_style() -> ProgressStyle {
	ProgressStyle::with_template("{prefix:>5} {elapsed_precise} [{bar:32}] {counter} {percent:>3}% eta {eta:<4} {msg}")
		.unwrap()
		.with_key("counter", |state: &ProgressState, w: &mut dyn std::fmt::Write| { //pos padded to the digits of len
			let len = state.len().unwrap_or(0);
			let _ = write!(w, "{:>width$}/{}", state.pos(), len, width = len.to_string().len());
		})
		.progress_chars("█▉▊▋▌▍▎▏ ")
}

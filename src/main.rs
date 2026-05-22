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
use szu::flush_print;
use clap::{Parser, Subcommand};

mod video;
mod audio;
mod math;
mod encoder;
mod player;

const EXT: &str = "szt";
const FORMAT_VERSION: u16 = 5;

#[derive(Parser, Debug)]
#[command(author, version)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    Encode(encoder::cli::Cli),
    Play(player::Cli),
}

fn main() -> anyhow::Result<()> {
	let args = Cli::parse();
	
	let mut watch = Stopwatch::start_new();
	match args.command {
		CliCommand::Encode(args) => encoder::encode(encoder::cli::process_args(args))?,
		CliCommand::Play(args) => player::play(args)?,
	};
	watch.stop();
	eprintln!("took: {}s & {}ms", watch.elapsed().as_secs(), watch.elapsed().subsec_millis());
	Ok(())
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

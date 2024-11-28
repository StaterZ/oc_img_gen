#![feature(
	iter_array_chunks,
	array_chunks,
	anonymous_lifetime_in_impl_trait,
	const_for, const_range_bounds,
	adt_const_params
)]
#![allow(dead_code)]

use std::{io::Write, path::{Path, PathBuf}};
use clap::Parser;
use cmd::szt::{self, SizedString, StreamDesc};
use color_print::cprintln;
use image::Image;
use indicatif::{ProgressBar, ProgressStyle};
use math::Size;
use stopwatch::Stopwatch;

use oc_color::formatters::*;
use oc_color::PaletteOr;
use szu::flush_print;

mod oc_color;
mod braille;
mod cmd;
mod math;
mod image;

const LOG: bool = false;

#[derive(Parser, Debug)]
#[clap(author = "StaterZ")]
struct Args {
	#[arg(short = 'd', long = "debug", action = clap::ArgAction::SetTrue)]
	is_debug: bool,

	#[arg(short = 'i', long = "in_path")]
	in_path: Option<PathBuf>,
	#[arg(short = 'o', long = "out_path")]
	out_path: Option<PathBuf>,

	#[arg(long = "max_size")]
	max_size: Option<usize>,

	#[arg(long = "f_begin")]
	begin_frame: Option<usize>,
	#[arg(long = "f_end")]
	last_frame: Option<usize>,
	#[arg(long = "f_rate")]
	frame_rate: Option<u16>,
}

fn main() {
	// test(RGB8::new(0x9949C0));
	// return;

	if let Err(err) = run() {
		cprintln!("<red>[ERR]: {}</>", err);
	}
}

fn run() -> Result<(), String> {
	video_rs::init().unwrap();

	let args = Args::parse();

	let in_path = if args.is_debug {
		Path::new("data/test.png")
	} else if let Some(in_path) = &args.in_path {
		in_path.as_path()
	} else {
		return Err("No input path".to_string());
	};

	let out_path = args.out_path.unwrap_or({
		let mut path= in_path.to_owned();
		if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
			path.set_file_name(format!("out_{}", name));
		}
		path.set_extension("szt");
		path
	});

	compute(
		&in_path,
		&out_path,
		args.begin_frame,
		args.last_frame,
		args.frame_rate,
	)
}

fn compute(
	in_path: &Path,
	out_path: &Path,
	begin_frame: Option<usize>,
	last_frame: Option<usize>,
	out_frame_rate: Option<u16>,
) -> Result<(), String> {
	let mut decoder = video_rs::Decoder::new(&video_rs::Location::File(in_path.to_path_buf()))
		.expect("failed to create decoder");
	
	let in_frame_rate = (1.0 / decoder.frame_rate()).round() as u16;
	let out_frame_rate = out_frame_rate.unwrap_or(in_frame_rate);
	
	let num_frames = std::cmp::max(decoder.frames().unwrap() as usize, 1);
	let num_frames_to_process = last_frame.unwrap_or_else(|| (num_frames - 1) - begin_frame.unwrap_or(0)) + 1;
	let progress = ProgressBar::new(num_frames_to_process as u64)
		.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ "));
	
	let mut writer = szt::FileWriter::new(out_frame_rate);
	
	let mut stream = szt::StreamWriter::new(StreamDesc {
		name: SizedString::new("main").unwrap(),
		size: {
			let raw_size = decoder.size_out();
			Size::new(raw_size.0 as u8, raw_size.1 as u8)
		},
	});
	
	if let Some(begin_frame) = begin_frame {
		decoder.seek_to_frame(begin_frame as i64).unwrap();
	}

	let mut time = 0;
	for (i, frame) in decoder.decode_iter().enumerate() {
		if let Some(last_frame) = last_frame {
			if i + begin_frame.unwrap_or(0) > last_frame { break; }
		}
		
		let frame = match frame {
			Ok(frame) => frame.1,
			Err(_) => break,
		};

		time += out_frame_rate;
		if time >= in_frame_rate {
			let img = Image::from(frame);
			
			let formatter = HybridFormatter::new();
			let img = stage("Processing | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
			let img = stage("Processing | Inflate", || img.map(|p| formatter.inflate(*p)));
			let img = stage("Processing | Braille", || braille::as_braille(&img));
			let img = stage("Processing | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
			loop {
				stage("Postamble  | Cmd Gen", || stream.push_frame_braille(&img));

				time -= in_frame_rate;
				if !(time >= in_frame_rate && in_frame_rate != 0) { break; } //do-while cond
			}
		}
		
		progress.inc(1);
	}

	progress.finish();

	writer.push_stream(stream);

	stage("Postamble  | Writing...  ", || std::fs::write(&out_path, writer.serialize().unwrap())
		.map_err(|err| format!("Failed to write output file. INNER: {}", err)))?;

	println!("All Done! saved to: {}", out_path.display());
	Ok(())
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

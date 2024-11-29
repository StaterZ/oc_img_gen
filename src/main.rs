#![feature(
	iter_array_chunks,
	array_chunks,
	anonymous_lifetime_in_impl_trait,
	const_for,
	const_range_bounds,
	adt_const_params,
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
	ffmpeg_next::init().unwrap();

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
	//ffmpeg madness
	let mut input_format_context = ffmpeg_next::format::input(in_path)
		.expect("failed to create decoder");
	let input_stream = input_format_context.streams().best(ffmpeg_next::media::Type::Video)
		.expect("stream not found");
	let video_stream_index = input_stream.index();

	let input_codec_context = ffmpeg_next::codec::Context::from_parameters(input_stream.parameters())
		.expect("failed to create codec context");
	let mut decoder = input_codec_context.decoder().video()
		.expect("failed to create video decoder");

	//compute in/out frame rates
	let in_frame_rate = decoder.frame_rate().map_or(0, |rate| szu::int_div_round!(rate.numerator(), rate.denominator()) as u16);
	let out_frame_rate = out_frame_rate.unwrap_or(in_frame_rate);
	
	//setup progress bar
	let num_frames = std::cmp::max(input_stream.frames() as usize, 1);
	let num_frames_to_process = last_frame.unwrap_or(num_frames - 1) + 1 - begin_frame.unwrap_or(0);
	let progress = ProgressBar::new(num_frames_to_process as u64)
		.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ "));
	
	let size = Size::<usize>::new(240, 240);

	//setup down-scaler
	let mut scaler = ffmpeg_next::software::scaling::Context::get(
		decoder.format(),
		decoder.width(),
		decoder.height(),
		ffmpeg_next::util::format::Pixel::RGB24,
		size.x as u32,
		size.y as u32,
		ffmpeg_next::software::scaling::Flags::BILINEAR,
	).unwrap();

	//setup SZT writer
	let mut writer = szt::FileWriter::new(out_frame_rate);
	
	let mut stream = szt::StreamWriter::new(StreamDesc {
		name: SizedString::new("main").unwrap(),
		size: (size / Size::new(braille::WIDTH, braille::HEIGHT)).try_cast().unwrap(),
	});
	
	//loop frames
	let mut frame_index = 0;
	let mut time = 0;

	let mut receive_and_process_frames = |decoder: &mut ffmpeg_next::decoder::Video| {
		let mut decoded_frame = ffmpeg_next::frame::Video::empty();
		while decoder.receive_frame(&mut decoded_frame).is_ok() {
			if let Some(begin_frame) = begin_frame {
				if frame_index < begin_frame {
					frame_index += 1;
					continue;
				}
			}
			if let Some(last_frame) = last_frame {
				if frame_index > last_frame {
					frame_index += 1;
					return false;
				}
			}
			
			time += out_frame_rate;
			if time >= in_frame_rate {	
				let mut rgb_frame = ffmpeg_next::frame::Video::empty();
				scaler.run(&decoded_frame, &mut rgb_frame).unwrap();
				
				let img = Image::from(rgb_frame);
				
				//Debug
				// if frame_index == 0 {
				// 	std::fs::write("data/geh.png", lodepng::encode24(
				// 		&img
				// 			.buffer()
				// 			.iter()
				// 			.map(|p| lodepng::RGB::new(p.r, p.g, p.b))
				// 			.collect_vec(),
				// 		img.size().x,
				// 		img.size().y,
				// 	).unwrap()).unwrap();
				// }
				
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

			frame_index += 1;
			progress.inc(1);
		}

		return true;
	};

	for (_, packet) in input_format_context
		.packets()
		.filter(|(ffmpeg_stream, _)| ffmpeg_stream.index() == video_stream_index)
	{
		decoder.send_packet(&packet).unwrap();
		if !receive_and_process_frames(&mut decoder) { break; }
	}
	decoder.send_eof().unwrap();
	receive_and_process_frames(&mut decoder);

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

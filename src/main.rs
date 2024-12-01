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
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use math::{Point, Rect, Size};
use stopwatch::Stopwatch;

use oc_color::{formatters::*, RGB8};
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

struct MatrixCell {
	pos: Point<usize>,
	stream_writer: szt::StreamWriter,
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
	let multi_progress = MultiProgress::new();

	let num_frames = std::cmp::max(input_stream.frames() as usize, 1);
	let num_frames_to_process = last_frame.unwrap_or(num_frames - 1) + 1 - begin_frame.unwrap_or(0);
	let frames_progress = multi_progress.add(ProgressBar::new(num_frames_to_process as u64)
		.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len} {eta}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ ")));
	if !LOG {
		frames_progress.tick();
	}

	//setup down-scaler
	let matrix_size = Size::<usize>::new(2, 2);
	let matrix_gap_size = Size::<usize>::new(16, 16);
	let stream_size= Size::<usize>::new(120, 60);
	let fill_color = RGB8::new(0x000000);

	let stream_input_size = stream_size * braille::SIZE;
	let container_size = matrix_size * stream_input_size + (matrix_size - 1) * matrix_gap_size;
	let content_size = Size::<usize>::new(decoder.width() as usize, decoder.height() as usize);
	let fit_size = container_size.contain(content_size);

	let mut scaler = ffmpeg_next::software::scaling::Context::get(
		decoder.format(),
		content_size.x as u32,
		content_size.y as u32,
		ffmpeg_next::util::format::Pixel::RGB24,
		fit_size.x as u32,
		fit_size.y as u32,
		ffmpeg_next::software::scaling::Flags::BILINEAR,
	).unwrap();

	//setup SZT stream writers
	let mut cell_streams = if false {
		vec![MatrixCell {
			pos: Point::new(0, 0), //TODO: this is dumb
			stream_writer: szt::StreamWriter::new(StreamDesc {
				name: SizedString::new("main").unwrap(),
				size: (fit_size / braille::SIZE).try_cast().unwrap(),
			}),
		}]
	} else {
		(0..matrix_size.y)
			.flat_map(move |y| (0..matrix_size.x)
				.map(move |x| MatrixCell {
					pos: Point::new(x, y),
					stream_writer: szt::StreamWriter::new(StreamDesc {
						name: SizedString::new(&format!("{},{}", x, y)).unwrap(),
						size: stream_size.try_cast().expect("stream size too large"),
					}),
				}))
			.collect()
	};
	
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
				let emit_count = if in_frame_rate == 0 { 1 } else { 
					let emit_count = time / in_frame_rate;
					time %= in_frame_rate;
					emit_count
				};
				
				let frame = stage("Frame  | Preamble  | Scale", || {
					let mut frame = ffmpeg_next::frame::Video::empty();
					scaler.run(&decoded_frame, &mut frame).unwrap();
					frame
				});
				
				let img = stage("Frame  | Preamble  | Into Image", || Image::from(frame));
				let img = stage("Frame  | Preamble  | Resize", || img.resize(container_size, fill_color));
				
				let frame_progress = multi_progress.add(ProgressBar::new(cell_streams.len() as u64)
					.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len}")
						.unwrap()
						.progress_chars("█▉▊▋▌▍▎▏ ")));
				if !LOG {
					frame_progress.tick();
				}
				
				for cell in cell_streams.iter_mut() {
					if LOG {
						println!();
					}

					let img = stage("Stream | Preamble  | Crop", || img.crop(Rect {
						pos: cell.pos * (stream_input_size + matrix_gap_size),
						size: stream_input_size,
					}));

					write_image(&format!("geh/{}.png", cell.pos), &img);

					let formatter = HybridFormatter::new();
					let img = stage("Stream | Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
					let img = stage("Stream | Process   | Inflate", || img.map(|p| formatter.inflate(*p)));
					let img = stage("Stream | Process   | Braille", || braille::as_braille(&img));
					let img = stage("Stream | Process   | B_Deflate ", || img.map(|braille| braille.map(|p| formatter.deflate(PaletteOr::NonPalette(*p)))));
					for _ in 0..emit_count {
						stage("Stream | Postamble | Cmd Gen", || cell.stream_writer.push_frame_braille(&img));
					}

					if !LOG {
						frame_progress.inc(1);
					}
				}
				frame_progress.finish_and_clear();
			}
			frame_index += 1;
			if !LOG {
				frames_progress.inc(1);
			}
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

	frames_progress.finish();
	
	let mut writer = szt::FileWriter::new(out_frame_rate);
	for cell in cell_streams {
		writer.push_stream(cell.stream_writer);
	}

	if LOG {
		println!();
	}
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

fn write_image(path: impl AsRef<Path>, img: &Image<RGB8>) {
	std::fs::write(path, lodepng::encode24(
		&img
			.buffer()
			.iter()
			.map(|p| lodepng::RGB::new(p.r, p.g, p.b))
			.collect_vec(),
		img.size().x,
		img.size().y,
	).unwrap()).unwrap();
}

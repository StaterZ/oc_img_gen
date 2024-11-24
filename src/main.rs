#![feature(
	iter_array_chunks,
	array_chunks,
	anonymous_lifetime_in_impl_trait,
	const_for, const_range_bounds,
	adt_const_params
)]
#![allow(dead_code)]

use std::{io::Write, path::{Path, PathBuf}};
use braille::Braille;
use clap::Parser;
use cmd::szt;
use color_print::cprintln;
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use lodepng::{decode24, encode24, Bitmap};
use math::Size;
use stopwatch::Stopwatch;

use oc_color::{formatters::*, PackedColor};
use oc_color::{RGB8, PaletteOr};
use szu::flush_print;

mod oc_color;
mod braille;
mod cmd;
mod math;

#[derive(Parser, Debug)]
#[clap(author = "StaterZ")]
struct Args {
	#[arg(short = 'd', long = "debug", action = clap::ArgAction::SetTrue)]
	is_debug: bool,

	#[arg(short = 'i', long = "in_path")]
	in_path: Option<PathBuf>,
	#[arg(short = 'o', long = "out_path")]
	out_path: Option<PathBuf>,
	#[arg(short = 'm', long = "mode")]
	mode: Mode,
	#[arg(short = 'f', long = "format")]
	format: Format,

	#[arg(long = "f_begin")]
	begin_frame: Option<usize>,
	#[arg(long = "f_end")]
	end_frame: Option<usize>,
	#[arg(long = "f_rate")]
	frame_rate: Option<u16>,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Mode {
	Image,
	Video,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Format {
	Png,
	Chr,
	Lua,
	Szt,
}

const LOG: bool = false;

fn test(a: RGB8) {
	let formatter = HybridFormatter::new();

	println!("real {}", a);
	let b = formatter.deflate(PaletteOr::NonPalette(a));
	println!("deflate {}", b);
	let c = formatter.inflate(b);
	println!("inflate {}", c);
}

fn main() {
	// test(RGB8::new(0x9949C0));
	// return;

	if let Err(err) = run() {
		cprintln!("<red>[ERR]: {}</>", err);
	}
}

fn run() -> Result<(), String> {
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
		path.set_extension(match args.format {
			Format::Png => "png",
			Format::Szt => "szt",
			Format::Lua => "lua",
			Format::Chr => "txt",
		});
		path
	});

	match args.mode {
		Mode::Image => compute(args.format, &in_path, &out_path),
		Mode::Video => compute_video(
			&in_path,
			&out_path,
			args.begin_frame.unwrap_or(0),
			args.end_frame.unwrap() + 1,
			args.frame_rate.unwrap(),
		),
	}
}

fn compute(format: Format, in_path: &Path, out_path: &Path) -> Result<(), String> {
	let blob_in = stage("Preamble   | Reading...  ", || std::fs::read(in_path)
		.map_err(|err| format!("Failed to read input file. INNER: {}", err)))?;

	let img_in_raw = stage("Preamble   | Decoding... ", || decode24(blob_in)
		.map_err(|err| format!("Failed to decode input image. INNER: {}", err)))?;
	
	let img_in = stage("Preamble   | Importing...", || import_image(&img_in_raw));
	
	if LOG {
		println!("           |");
	}
	let formatter = HybridFormatter::new();
	let proc_a = stage("Processing | Deflate", || deflate_image(&formatter, &img_in));
	let proc_b =stage("Processing | Inflate", || inflate_image(&formatter, &proc_a));
	let proc_c = stage("Processing | Braille", || braille::as_braille(&proc_b));
	
	let blob_out = match format {
		Format::Png => {
			let img_out = stage("Processing | Raster ", || braille::raster(&proc_c));
			if LOG {
				println!("           |");
			}

			let img_out_raw = stage("Postamble  | Exporting...", || export_image(&img_out));
			
			stage("Postamble  | Encoding... ", || encode24(&img_out_raw.buffer, img_out_raw.width, img_out_raw.height)
				.map_err(|err| format!("Failed to encode output image. INNER: {}", err)))?
		},
		Format::Chr => {
			proc_c.buffer
				.chunks_exact(proc_c.width)
				.map(|row| row
					.into_iter()
					.map(|c| c.char())
					.collect::<String>())
				.join("\n")
				.bytes()
				.collect()
		},
		Format::Lua => {
			let img_out = stage("Processing | B_Deflate ", || deflate_braille(&formatter, &proc_c));
			if LOG {
				println!("           |");
			}

			let char_buffer = map_bitmap(&img_out, |braille| braille.into());
			stage("Postamble  | Cmd Gen", || cmd::code_gen(&char_buffer, None, &formatter)
				.bytes()
				.collect())
		},
		Format::Szt => {
			let img_out = stage("Processing | B_Deflate ", || deflate_braille(&formatter, &proc_c));
			if LOG {
				println!("           |");
			}

			stage("Postamble  | Cmd Gen", || {
				let size = Size::new(img_out.width as u8, img_out.height as u8);
				let mut writer = szt::Writer::new(size, 0);
				writer.push_frame_braille(img_out);
				writer.serialize().unwrap()
			})
			
			// stage("Postamble  | Encoding... ", || img_out.buffer
			// 	.iter()
			// 	.flat_map(|c| [c.bg.into(), c.fg.into(), c.char_index()])
			// 	.collect_vec())
		},
	};

	stage("Postamble  | Writing...  ", || std::fs::write(&out_path, blob_out)
		.map_err(|err| format!("Failed to write output file. INNER: {}", err)))?;

	println!("All Done! saved to: {}", out_path.display());
	Ok(())
}

fn compute_video(
	in_path: &Path,
	out_path: &Path,
	begin_frame: usize,
	end_frame: usize,
	frame_rate: u16,
) -> Result<(), String> {
	let frame_iter = begin_frame..end_frame;

	let progress = ProgressBar::new(frame_iter.len() as u64)
		.with_style(ProgressStyle::with_template("[{bar}] {pos}/{len}")
			.unwrap()
			.progress_chars("█▉▊▋▌▍▎▏ "));

	let mut writer = szt::Writer::new(Size::new(0, 0), frame_rate);

	for i in frame_iter {
		let in_path = in_path.to_string_lossy().replace('*', &format!("{:04}", i));
		let blob_in = std::fs::read(&in_path)
			.map_err(|err| format!("Failed to read input file. INNER: {}", err))?;

		let img_in = decode24(blob_in)
			.map_err(|err| format!("Failed to decode input image. INNER: {}", err))?;
		let img_in = import_image(&img_in);
		
		let formatter = HybridFormatter::new();
		let proc_a = stage("Processing | Deflate", || deflate_image(&formatter, &img_in));
		let proc_b =stage("Processing | Inflate", || inflate_image(&formatter, &proc_a));
		let proc_c = stage("Processing | Braille", || braille::as_braille(&proc_b));
		let img_out = stage("Processing | B_Deflate ", || deflate_braille(&formatter, &proc_c));
		
		let frame_size = Size::new(img_out.width as u8, img_out.height as u8);
		if i == begin_frame {
			writer.file.header.size = frame_size;
		}
		assert_eq!(frame_size, writer.file.header.size);

		stage("Postamble  | Cmd Gen", || writer.push_frame_braille(img_out));

		progress.update(|s| s.set_pos((i - begin_frame) as u64));
	}

	progress.finish();

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

fn map_bitmap<T, B>(value: &Bitmap<T>, f: impl FnMut(&T) -> B) -> Bitmap<B> {
	let buffer = value.buffer
		.iter()
		.map(f)
		.collect();

	Bitmap {
		buffer,
		width: value.width,
		height: value.height,
	}
}

fn inflate_image(formatter: &impl Formatter, value: &Bitmap<PackedColor>) -> Bitmap<RGB8> {
	map_bitmap(&value, |pixel| formatter.inflate(*pixel))
}

fn deflate_image(formatter: &impl Formatter, value: &Bitmap<RGB8>) -> Bitmap<PackedColor> {
	map_bitmap(&value, |pixel| formatter.deflate(PaletteOr::NonPalette(*pixel)))
}

fn deflate_braille(formatter: &impl Formatter, value: &Bitmap<Braille<RGB8>>) -> Bitmap<Braille<PackedColor>> {
	map_bitmap(&value, |braille| braille
		.map(|color| formatter
			.deflate(PaletteOr::NonPalette(*color))))
}

fn import_image(value: &Bitmap<lodepng::RGB<u8>>) -> Bitmap<RGB8> {
	map_bitmap(&value, |pixel| RGB8 { r: pixel.r, g: pixel.g, b: pixel.b })
}

fn export_image(value: &Bitmap<RGB8>) -> Bitmap<lodepng::RGB<u8>> {
	map_bitmap(&value, |pixel| lodepng::RGB { r: pixel.r, g: pixel.g, b: pixel.b })
}

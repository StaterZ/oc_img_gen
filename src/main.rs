#![allow(dead_code)]
#![feature(iter_array_chunks, array_chunks)]

use std::{
	path::{Path, PathBuf},
	io::Write,
};
use braille::Braille;
use clap::Parser;
use color_print::cprintln;
use lodepng::{decode24, encode24, Bitmap};
use stopwatch::Stopwatch;

use oc_color::{formatters::*, PackedColor};
use oc_color::{RGB8, PaletteOr};
use szu::flush_print;

mod oc_color;
mod braille;

#[derive(Parser, Debug)]
#[clap(author = "StaterZ")]
struct Args {
	#[arg(short = 'd', long = "debug", action = clap::ArgAction::SetTrue)]
	is_debug: bool,

	#[arg(short = 'i', long = "in_path")]
	in_path: Option<PathBuf>,
	#[arg(short = 'o', long = "out_path")]
	out_path: Option<PathBuf>,
}

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
		path
	});

	{
		let blob_in = stage("Preamble   | Reading...  ", || std::fs::read(in_path)
			.map_err(|err| format!("Failed to read input file. INNER: {}", err)))?;

		let img_in_raw = stage("Preamble   | Decoding... ", || decode24(blob_in)
			.map_err(|err| format!("Failed to decode input image. INNER: {}", err)))?;
		
		let img_in = stage("Preamble   | Importing...", || import_image(&img_in_raw));
		
		println!("           |");
		let img_out = process1(img_in);
		println!("           |");

		let img_out_raw = stage("Postamble  | Exporting...", || export_image(&img_out));
		
		let blob_out = stage("Postamble  | Encoding... ", || encode24(img_out_raw.buffer.as_slice(), img_out_raw.width, img_out_raw.height)
			.map_err(|err| format!("Failed to encode output image. INNER: {}", err)))?;
		
		stage("Postamble  | Writing...  ", || std::fs::write(out_path, blob_out)
			.map_err(|err| format!("Failed to write output file. INNER: {}", err)))?;
	}

	println!("All Done!");
	Ok(())
}

fn process0(img_in: Bitmap<RGB8>) -> Bitmap<RGB8> {
	let formatter = HybridFormatter::new();
	let a = stage("Processing | Deflate", || deflate_image(&formatter, &img_in));
	//let b = stage("Processing | Braille", || braille::as_braille(&a));
	//let c = stage("Processing | Pixels", || braille::as_pixels(&b));
	stage("Processing | Inflate", || inflate_image(&formatter, &a))
}

fn process1(img_in: Bitmap<RGB8>) -> Bitmap<RGB8> {
	let formatter = HybridFormatter::new();
	let a = stage("Processing | Braille", || braille::as_braille(&img_in));
	let b = stage("Processing | Deflate", || deflate_braille(&formatter, &a));
	let c = stage("Processing | Pixels ", || braille::as_pixels(&b));
	stage("Processing | Inflate", || inflate_image(&formatter, &c))
}

fn stage<B>(title: &str, f: impl FnOnce() -> B) -> B {
	flush_print!("{}", title);
	let mut timer = Stopwatch::start_new();
	let output = f();
	timer.stop();
	println!(" time: {}ms", timer.elapsed().as_millis());
	output
}

fn inflate_image(formatter: &impl Formatter, input: &Bitmap<PackedColor>) -> Bitmap<RGB8> {
	let output = input.buffer
		.iter()
		.map(|pixel| formatter.inflate(*pixel))
		.collect();

	Bitmap {
		buffer: output,
		width: input.width,
		height: input.height,
	}
}

fn deflate_braille(formatter: &impl Formatter, input: &Bitmap<Braille<RGB8>>) -> Bitmap<Braille<PackedColor>> {
	let output = input.buffer
		.iter()
		.map(|pixel| Braille {
			id: pixel.id,
			bg: formatter.deflate(PaletteOr::NonPalette(pixel.bg)),
			fg: formatter.deflate(PaletteOr::NonPalette(pixel.fg)),
		}).collect();

	Bitmap {
		buffer: output,
		width: input.width,
		height: input.height,
	}
}

fn deflate_image(formatter: &impl Formatter, input: &Bitmap<RGB8>) -> Bitmap<PackedColor> {
	let output = input.buffer
		.iter()
		.map(|pixel| formatter.deflate(PaletteOr::NonPalette(*pixel)))
		.collect();

	Bitmap {
		buffer: output,
		width: input.width,
		height: input.height,
	}
}

fn import_image(input: &Bitmap<lodepng::RGB<u8>>) -> Bitmap<RGB8> {
	Bitmap {
		buffer: input.buffer
			.iter()
			.map(|pixel| RGB8 { r: pixel.r, g: pixel.g, b: pixel.b })
			.collect(),
		width: input.width,
		height: input.height,
	}
}

fn export_image(input: &Bitmap<RGB8>) -> Bitmap<lodepng::RGB<u8>> {
	Bitmap {
		buffer: input.buffer
			.iter()
			.map(|pixel| lodepng::RGB { r: pixel.r, g: pixel.g, b: pixel.b })
			.collect(),
		width: input.width,
		height: input.height,
	}
}

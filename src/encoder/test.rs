use std::path::PathBuf;
use clap::Parser;

use crate::stage;
use crate::video::{
	oc_color::{RGB8,
		PaletteOr,
		formatters::{Formatter, HybridFormatter},
	},
	Image,
};

const EXT: &str = "png";
#[derive(Parser, Debug)]
#[clap(author = "StaterZ")]
struct Args {
	#[arg(
		short = 'i',
		long = "in",
		help = "Path to image or video to encode",
	)]
	in_path: PathBuf,

	#[arg(
		short = 'o',
		long = "out",
		help = "Path to where to the output png file, saves at in_path with .png extension if omitted",
	)]
	out_path: Option<PathBuf>,
}

pub fn run() -> Result<(), String> {
	let args = Args::parse();
	let out_path = match args.out_path {
		None => {
			let mut path = args.in_path.clone();
			path.set_extension(EXT);
			path
		},
		Some(mut path) => {
			if path.is_dir() {
				path.push(args.in_path.file_name().ok_or("Missing input".to_string())?);
				path.set_extension(EXT);
			} else if path.extension() == None {
				path.set_extension(EXT);
			}
			path
		}
	};

	let blob_in = stage("Preamble  | Reading...  ", || std::fs::read(args.in_path)
		.map_err(|err| format!("Failed to read input file. INNER: {}", err)))?;

	let img_in_raw = stage("Preamble  | Decoding... ", || lodepng::decode24(blob_in)
		.map_err(|err| format!("Failed to decode input image. INNER: {}", err)))?;
	
	let img_in = stage("Preamble  | Importing...", || img_in_raw.into());
	
	println!("          |");
	let img_out = process(img_in);
	println!("          |");

	let img_out_raw: lodepng::Bitmap<lodepng::RGB<u8>> = stage("Postamble | Exporting...", || img_out.into());
	
	let blob_out = stage("Postamble | Encoding... ", || lodepng::encode24(img_out_raw.buffer.as_slice(), img_out_raw.width, img_out_raw.height)
		.map_err(|err| format!("Failed to encode output image. INNER: {}", err)))?;
	
	stage("Postamble | Writing...  ", || std::fs::write(out_path, blob_out)
		.map_err(|err| format!("Failed to write output file. INNER: {}", err)))?;

	Ok(())
}

fn process(img: Image<RGB8>) -> Image<RGB8> {
	let formatter = HybridFormatter::new();
	let img = stage("Process   | Deflate", || img.map(|p| formatter.deflate(PaletteOr::NonPalette(*p))));
	for y in (0..img.buffer().len()).step_by(img.size().x) {
		let line = img.buffer()
			.iter()
			.copied()
			.skip(y)
			.take(img.size().x)
			.map(|p| p.0)
			.rev()
			.collect::<Vec<_>>();

		let line = line
			.chunks(2)
			.enumerate()
			.map(|(i, chunk)| match chunk {
				[a, b] => format!("{}({:02x} {:02x})", i + 1, a, b),
				[a] => format!("({:02x})", a), // handle odd-length case
				_ => String::new(),
			})
			.collect::<Vec<_>>()
			.join(" ");
		
		println!("{}", line);
	}
	let img = stage("Process   | Inflate", || img.map(|p| formatter.inflate(*p)));
	img
}

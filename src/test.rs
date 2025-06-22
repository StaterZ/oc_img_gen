use itertools::Itertools;

use crate::RGB8;
use crate::stage;
use crate::PaletteOr;
use crate::Formatter;
use crate::HybridFormatter;
use crate::Image;

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

pub fn run() -> Result<(), String> {
	let in_path = "chicken.png";
	let out_path = "out.png";

	let blob_in = stage("Preamble  | Reading...  ", || std::fs::read(in_path)
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

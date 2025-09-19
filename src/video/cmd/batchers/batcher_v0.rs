use crate::math::Point;
use super::super::{TermFrame, Renderer, TermPixel, TermChar, super::oc_color::PackedColor};

struct Batch {
	pub pos: Point<usize>,
	pub bg: PackedColor,
	pub fg: PackedColor,
	pub chars: String,
}

pub fn draw(renderer: &mut impl Renderer, frame: &TermFrame, _prev_frame: Option<&TermFrame>) {
	let mut emit = |batch: Batch| {
		renderer.set_background(batch.bg);
		renderer.set_foreground(batch.fg);
		renderer.set(&batch.pos, &batch.chars);
	};

	let mut batch = None::<Batch>;
	for y in 0..frame.size().y {
		for x in 0..frame.size().x {
			let i = y * frame.size().x + x;
			let char = &frame.buffer()[i];

			if let Some(batch) = batch.as_mut() {
				if let Some(smart_char) = find_smart_char(&batch, &char) {
					batch.chars.push(smart_char.into());
					continue;
				}
			}

			if let Some(batch) = batch.replace(Batch {
				pos: Point { x, y },
				bg: char.bg,
				fg: char.fg,
				chars: char.sym.into_inner().to_string(),
			}) {
				emit(batch);
			}
		}
		if let Some(batch) = batch.take() {
			emit(batch);
		}
	}
}

fn find_smart_char(current: &Batch, char: &TermPixel) -> Option<TermChar> {
	//base case
	if char.bg == current.bg && char.fg == current.fg {
		return Some(char.sym);
	}

	//single color case
	let effective_sym = if char.bg == char.fg { ' '.into() } else { char.sym };
	match effective_sym.into_inner() {
		' ' | '⠀' => return match char.bg {
			c if c == current.bg => Some(effective_sym),
			c if c == current.fg => effective_sym.flip(),
			_ => None,
		},
		'█' | '⣿' => return match char.fg {
			c if c == current.fg => Some(effective_sym),
			c if c == current.bg => effective_sym.flip(),
			_ => None,
		},
		_ => {},
	}

	//try flip it
	if char.fg == current.bg && char.bg == current.fg {
		return char.sym.flip();
	}

	//else, we've failed
	None
}

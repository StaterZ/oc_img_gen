use std::collections::HashMap;

use itertools::Itertools;

use crate::{cmd::term_char::TermChar, hybrid_formatter::StaticColor, math::Point, oc_color::{PackedColor, PaletteOr, RGB8}};

use super::super::{Frame, Renderer, TermPixel};

struct Batch {
	pub kind: BatchKind,
	pub pos: Point,
	pub bg: PackedColor,
	pub fg: PackedColor,
	pub chars: String,
}
impl Batch {
	fn flip(&mut self) {
		(self.bg, self.fg) = (self.fg, self.bg); //is this supposed to be here? me iz skeptic.... might break at call site as of current
		self.chars = self.chars
			.chars()
			.map(|c| TermChar::from(c)
				.flip()
				.unwrap()
				.into_inner())
			.join("");
	}
	
	fn draw(&self, renderer: &mut impl Renderer) {
		if self.kind != BatchKind::FgOnly {
			renderer.set_background(self.bg);
		}
		if self.kind != BatchKind::BgOnly {
			renderer.set_foreground(self.fg);
		}
		renderer.set(&self.pos, &self.chars);
	}
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum BatchKind {
	Unique,
	Flippable,
	BgOnly,
	FgOnly,
}

impl BatchKind {
	pub fn is_single_color(&self) -> bool {
		matches!(self, BatchKind::BgOnly | BatchKind::FgOnly)
	}
}

struct CondColors {
	bg: bool,
	fg: bool,
}

struct CondMatches {
	perfect: bool,
	flipped: bool,
	insert: bool,
}

struct CondVars {
	char: CondColors,
	batch: CondColors,
	matches: CondMatches,
}

type ColorTable = HashMap<PackedColor, HashMap<PackedColor, Vec<Batch>>>;
struct Buckets {
	bg_to_fgs: ColorTable,
	fg_to_bgs: ColorTable,
}

struct RenderState {
	bg: PackedColor,
	fg: PackedColor,
}

impl Buckets {
	pub fn new() -> Self {
		Self {
			bg_to_fgs: HashMap::new(),
			fg_to_bgs: HashMap::new(),
		}
	}
}

pub fn draw(renderer: &mut impl Renderer, frame: &Frame, prev_frame: Option<&Frame>) {
	let mut batches = generate_batches(frame, prev_frame);
	// let buckets = generate_buckets(batches);
	// draw_buckets(renderer, buckets);

	fn batch_cost(state: &RenderState, batch: &Batch) -> usize {
		let mut cost = 0;
		if state.bg != batch.bg && batch.kind != BatchKind::FgOnly { cost += 1 }
		if state.fg != batch.fg && batch.kind != BatchKind::BgOnly { cost += 1 }
		cost
	}

	let mut state = RenderState {
		bg: PackedColor::new(PaletteOr::NonPalette(StaticColor::deflate(RGB8::new(0x000000)))),
		fg: PackedColor::new(PaletteOr::NonPalette(StaticColor::deflate(RGB8::new(0xffffff)))),
	};
	while let Some(batch) = batches
		.iter()
		.enumerate()
		.min_by_key(|(_i, batch)| batch_cost(&state, &batch))
		.map(|(i, _batch)| i)
		.map(|i| batches.swap_remove(i))
	{
		batch.draw(renderer);
		state.bg = batch.bg;
		state.fg = batch.fg;
	}
}

fn generate_batches(frame: &Frame, prev_frame: Option<&Frame>) -> Vec<Batch> {
	let mut output = Vec::new();

	fn compute_char_batch_kind(c: &TermPixel) -> BatchKind {
		if c.bg == c.fg {
			debug_assert_eq!(c.sym, ' '.into());
			BatchKind::BgOnly
		} else if c.sym.is_bg_only() {
			BatchKind::BgOnly
		} else if c.sym.is_fg_only() {
			BatchKind::FgOnly
		} else if c.flip().is_some() { //TODO: optimize this shit
			BatchKind::Flippable
		} else {
			BatchKind::Unique
		}
	}

	for y in 0..frame.height {
		let mut batch = None::<Batch>;
		for x in 0..frame.width {
			let i = y * frame.width + x;
			let char = &frame.buffer[i];
			if let Some(prev_frame) = prev_frame {
				let char_prev = &prev_frame.buffer[i];
				if char == char_prev { continue; }
			}

			let char = char.to_canonical();

			if let Some(batch) = batch.as_mut() {
				let char_batch_kind = compute_char_batch_kind(&char);

				//    c b   c b
				// P: F=F ∩ B=B //perfect
				// F: F=B ∩ B=F //flipped
				// I: F≠B ∪ B≠F //insert
				// 
				//    | U   | F   | BG  | FG  |
				// ---|-----|-----|-----|-----|
				//  U | P__ | PF_ | PF_ | PF_ |
				//  F | PF_ | PF_ | PF_ | PF_ |
				// BG | PF_ | PF_ | P_I | *FI |
				// FG | PF_ | PF_ | *FI | P_I |

				let cond_vars = CondVars {
					char: CondColors { bg: char_batch_kind != BatchKind::FgOnly, fg: char_batch_kind != BatchKind::BgOnly },
					batch: CondColors { bg: batch.kind != BatchKind::FgOnly, fg: batch.kind != BatchKind::BgOnly },
					matches: CondMatches {
						perfect: true, //the only case we don't need it is if both char and batch are different single color batch kinds. for this case it's okay to have perfect matching on since it's impossible it match then, meaning we can just the entire perfect matcher to always occur
						flipped: char_batch_kind != batch.kind || char_batch_kind == BatchKind::Flippable, //flip everywhere except along the diagonal, with the exception of the flippables intersection where we still flip
						insert: char_batch_kind.is_single_color() && batch.kind.is_single_color(), //is both are sing colors, that leaves one color for each, so we can insert
					},
				};

				let bg_bg = (cond_vars.char.bg && cond_vars.batch.bg) && char.bg == batch.bg;
				let bg_fg = (cond_vars.char.bg && cond_vars.batch.fg) && char.bg == batch.fg;
				let fg_bg = (cond_vars.char.fg && cond_vars.batch.bg) && char.fg == batch.bg;
				let fg_fg = (cond_vars.char.fg && cond_vars.batch.fg) && char.fg == batch.fg;

				let perfect = cond_vars.matches.perfect && bg_bg && fg_fg;
				let flipped = cond_vars.matches.flipped && bg_fg && fg_bg;
				let insert = cond_vars.matches.insert && !(bg_fg && fg_bg); //TODO: effectively just a flipped inverse, kinda dumb?

				let can_append = perfect || flipped || insert;
				if can_append {
					batch.kind = if char_batch_kind == BatchKind::Unique || batch.kind == BatchKind::Unique {
						BatchKind::Unique
					} else if cond_vars.matches.insert && !insert {
						batch.kind
					} else {
						BatchKind::Flippable
					};

					if insert {
						if batch.kind == BatchKind::BgOnly {
							batch.fg = char.bg
						} else {
							batch.bg = char.fg
						};
					}
					
					let mut sym = char.sym;
					let is_flip = !perfect && flipped;
					if is_flip {
						if char_batch_kind == BatchKind::Unique { //if the char is unique, we need to flip the batch instead
							batch.flip();
						} else {
							sym = sym.flip().unwrap();
						}
					}

					batch.chars.push(sym.into());
					continue;
				}
			}
			
			if let Some(batch) = batch.replace(Batch {
				kind: compute_char_batch_kind(&char),
				pos: Point { x, y },
				bg: char.bg,
				fg: char.fg,
				chars: char.sym.into_inner().to_string(),
			}) {
				output.push(batch);
			}
		}
		if let Some(batch) = batch {
			output.push(batch);
		}
	}

	output
}

// fn generate_buckets(batches: Vec<Batch>) -> Buckets {
// 	fn ensure_bucket(color_table: &mut ColorTable, bg: PackedColor, fg: PackedColor) -> &mut Vec<Batch> {
// 		color_table
// 			.entry(bg)
// 			.or_insert_with(HashMap::new)
// 			.entry(fg)
// 			.or_insert_with(Vec::new)
// 	}

// 	let mut buckets = Buckets::new();
// 	for batch in batches {
// 		if batch != BatchKind::Unique {
// 			ensure_bucket(&mut buckets.bg_to_fgs, batch.fg, batch.bg).flipped.push(batch.flip());
// 		}
		
// 		ensure_bucket(&mut buckets.bg_to_fgs, batch.bg, batch.fg).perfect.push(batch);
// 	}

// 	buckets
// }

// fn get_best_next_bucket<'a>(color_tables: &'a mut ColorTables, render_state: &mut RenderState) -> Option<&'a ColorBuckets> {
// 	fn get_next_entry<K, V>(map: &HashMap<K, V>) -> Option<(&K, &V)> {
// 		map.iter().next()
// 	}

// 	if let Some(fgs) = color_tables.bg_to_fgs.get(&render_state.bg) {
// 		if let Some(buckets) = fgs.get(&render_state.fg) {
// 			Some(buckets)
// 		} else {
// 			get_next_entry(&fgs)
// 				.map(|(&fg, buckets)| {
// 					render_state.fg = fg;
// 					buckets
// 				})
// 		}
// 	} else {
// 		if let Some(bgs) = color_tables.fg_to_bgs.get(&render_state.fg) {
// 			get_next_entry(&bgs)
// 				.map(|(&bg, buckets)| {
// 					render_state.bg = bg;
// 					buckets
// 				})
// 		} else {
// 			get_next_entry(&color_tables.bg_to_fgs)
// 				.and_then(|(&bg, fgs)|
// 					get_next_entry(&fgs)
// 						.map(|(&fg, buckets)| {
// 							render_state.bg = bg;
// 							render_state.fg = fg;
// 							buckets
// 						}))
// 		}
// 	}
// }

// fn draw_buckets(renderer: &mut impl Renderer, buckets: Buckets) {
// 	while !buckets.bg_to_fgs.is_empty() {
// 		get_best_next_bucket()
		
// 	}
// 	// for batch in buckets. {
// 	// 	batch.draw(renderer);
// 	// }
// }

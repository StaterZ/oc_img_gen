use std::{cell::Cell, collections::{hash_map::{Entry, OccupiedEntry}, HashMap}, ops::Deref, rc::Rc};

use itertools::Itertools;

use crate::math::Point;
use super::super::{TermFrame, Renderer, TermPixel, TermChar, super::oc_color::PackedColor};

#[derive(Debug, Clone)]
struct Batch {
	pub kind: BatchKind,
	pub pos: Point<usize>,
	pub bg: PackedColor,
	pub fg: PackedColor,
	pub chars: String,
}

impl Batch {
	fn flip(&self) -> Self {
		debug_assert_ne!(self.kind, BatchKind::Unique);

		let chars = self.chars
			.chars()
			.map(|c| TermChar::from(c)
				.flip()
				.unwrap()
				.into_inner())
			.join("");

		Self {
			kind: self.kind,
			pos: self.pos,
			bg: self.fg,
			fg: self.bg,
			chars,
		}
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

	fn draw_with_state(&self, renderer: &mut impl Renderer, state: &mut RenderState) {
		if self.kind != BatchKind::FgOnly {
			renderer.set_background(self.bg);
			state.bg = Some(self.bg);
		}
		if self.kind != BatchKind::BgOnly {
			renderer.set_foreground(self.fg);
			state.fg = Some(self.fg);
		}
		renderer.set(&self.pos, &self.chars);
	}
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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
	using_bg: bool,
	using_fg: bool,
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

struct RenderState {
	bg: Option<PackedColor>,
	fg: Option<PackedColor>,
}

impl RenderState {
	fn new() -> Self {
		Self {
			bg: None,
			fg: None,
		}
	}
}

#[derive(Clone)]
struct BucketBatch {
	batch: Batch,
	is_dead: Cell<bool>,
}

impl BucketBatch {
	pub fn new(batch: Batch) -> Self {
		Self {
			batch,
			is_dead: Cell::new(false),
		}
	}
}

impl Deref for BucketBatch {
	type Target = Batch;

	fn deref(&self) -> &Self::Target {
		&self.batch
	}
}

type Bucket = Vec<Rc<BucketBatch>>;
type ColorTable = HashMap<Option<PackedColor>, HashMap<Option<PackedColor>, Bucket>>;
struct Accelerator {
	bg_to_fgs: ColorTable,
	fg_to_bgs: ColorTable,
}

impl Accelerator {
	pub fn new() -> Self {
		Self {
			bg_to_fgs: ColorTable::new(),
			fg_to_bgs: ColorTable::new(),
		}
	}

	pub fn is_empty(&self) -> bool {
		debug_assert_eq!(self.bg_to_fgs.len(), self.fg_to_bgs.len());
		self.bg_to_fgs.is_empty()
	}

	pub fn push(&mut self, batch: &Rc<BucketBatch>) {
		let bg = (batch.kind != BatchKind::FgOnly).then_some(batch.bg);
		let fg = (batch.kind != BatchKind::BgOnly).then_some(batch.fg);

		self.get_bg_to_fgs(bg, fg).push(batch.clone());
		self.get_fg_to_bgs(fg, bg).push(batch.clone());

		if batch.kind != BatchKind::Unique {
			self.get_bg_to_fgs(fg, bg).push(batch.clone());
			self.get_fg_to_bgs(bg, fg).push(batch.clone());
		}
	}

	fn get_bg_to_fgs(&mut self, bg: Option<PackedColor>, fg: Option<PackedColor>) -> &mut Bucket {
		Self::get(&mut self.bg_to_fgs, bg, fg)
	}

	fn get_fg_to_bgs(&mut self, fg: Option<PackedColor>, bg: Option<PackedColor>) -> &mut Bucket {
		Self::get(&mut self.fg_to_bgs, fg, bg)
	}

	fn get(color_table: &mut ColorTable, k0: Option<PackedColor>, k1: Option<PackedColor>) -> &mut Bucket {
		color_table
			.entry(k0)
			.or_insert_with(HashMap::new)
			.entry(k1)
			.or_insert_with(Bucket::new)
	}
	
	fn draw<>(mut self, renderer: &mut impl Renderer) {
		let mut state = RenderState::new();
		while let Some(bucket) = self.pop_best_bucket(&state) {
			for batch in bucket {
				if batch.is_dead.get() { continue; }

				batch.draw_with_state(renderer, &mut state);
				batch.is_dead.set(true);
			}
		}
	}
	
	fn pop_best_bucket<>(&mut self, state: &RenderState) -> Option<Bucket> {
		fn unwrap_entry<'a, K, V>(entry: Entry<'a, K, V>) -> OccupiedEntry<'a, K, V> {
			match entry {
				Entry::Occupied(occupied_entry) => occupied_entry,
				Entry::Vacant(_vacant_entry) => panic!("called `unwrap_entry()` on a `Vacant` value"),
			}
		}
		fn get_next_entry<V>(map: &mut HashMap<Option<PackedColor>, V>) -> Option<OccupiedEntry<'_, Option<PackedColor>, V>> {
			map.keys().next().copied().map(|k| unwrap_entry(map.entry(k)))
		}
		fn pop_next_entry<V>(map: &mut HashMap<Option<PackedColor>, V>) -> Option<V> {
			map.keys().next().copied().map(|k| map.remove(&k).unwrap())
		}

		if let Entry::Occupied(mut fgs) = self.bg_to_fgs.entry(state.bg) {
			let bucket = if let Entry::Occupied(bucket) = fgs.get_mut().entry(state.fg) {
				Some(bucket.remove())
			} else if let Entry::Occupied(bucket) = fgs.get_mut().entry(None) {
				Some(bucket.remove())
			} else {
				pop_next_entry(fgs.get_mut())
			};

			if fgs.get().is_empty() {
				fgs.remove();
			}
			bucket
		} else if let Entry::Occupied(mut bgs) = self.fg_to_bgs.entry(state.fg) {
			let bucket = if let Entry::Occupied(bucket) = bgs.get_mut().entry(None) {
				Some(bucket.remove())
			} else {
				pop_next_entry(bgs.get_mut())
			};

			if bgs.get().is_empty() {
				bgs.remove();
			}
			bucket
		} else {
			let mut fgs = get_next_entry(&mut self.bg_to_fgs)?;
			let bucket = pop_next_entry(fgs.get_mut());
			if fgs.get().is_empty() {
				fgs.remove();
			}
			bucket
		}
	}
}

pub fn draw(renderer: &mut impl Renderer, frame: &TermFrame, prev_frame: Option<&TermFrame>) {
	let batches = generate_batches(frame, prev_frame);
	
	let mut accelerator = Accelerator::new();
	for batch in batches {
		accelerator.push(&Rc::new(BucketBatch::new(batch)));
	}
	accelerator.draw(renderer);
}

struct WorkBatch {
	running: Batch,
	last_with_change: Batch,
}

impl WorkBatch {
	fn new(batch: Batch) -> Self {
		Self {
			running: batch.clone(),
			last_with_change: batch,
		}
	}
}

fn generate_batches(frame: &TermFrame, prev_frame: Option<&TermFrame>) -> Vec<Batch> {
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

	for y in 0..frame.size().y {
		let mut work_batch = None::<WorkBatch>;
		for x in 0..frame.size().x {
			let i = y * frame.size().x + x;
			let char = &frame.buffer()[i];

			let is_same_as_prev_frame = if let Some(prev_frame) = prev_frame {
				let char_prev = &prev_frame.buffer()[i];
				char == char_prev
			} else {
				false
			};

			if is_same_as_prev_frame && work_batch.is_none() { continue; }

			let char = char.to_canonical();
			let char_batch_kind = compute_char_batch_kind(&char);

			if let Some(work_batch) = work_batch.as_mut() {
				let batch = &mut work_batch.running;

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
					char: CondColors { using_bg: char_batch_kind != BatchKind::FgOnly, using_fg: char_batch_kind != BatchKind::BgOnly },
					batch: CondColors { using_bg: batch.kind != BatchKind::FgOnly, using_fg: batch.kind != BatchKind::BgOnly },
					matches: CondMatches {
						perfect: true, //the only case we don't need it is if both char and batch are different single color batch kinds. for this case it's okay to have perfect matching on since it's impossible it match then, meaning we can just the entire perfect matcher to always occur
						flipped: char_batch_kind != batch.kind || char_batch_kind == BatchKind::Flippable, //flip everywhere except along the diagonal, with the exception of the flippables intersection where we still flip
						insert: char_batch_kind.is_single_color() && batch.kind.is_single_color(), //if both are single colors, that leaves one color for each, so we can insert
					},
				};

				let bg_bg = (!cond_vars.char.using_bg || !cond_vars.batch.using_bg) || char.bg == batch.bg;
				let bg_fg = (!cond_vars.char.using_bg || !cond_vars.batch.using_fg) || char.bg == batch.fg;
				let fg_bg = (!cond_vars.char.using_fg || !cond_vars.batch.using_bg) || char.fg == batch.bg;
				let fg_fg = (!cond_vars.char.using_fg || !cond_vars.batch.using_fg) || char.fg == batch.fg;

				let perfect = cond_vars.matches.perfect && bg_bg && fg_fg;
				let flipped = cond_vars.matches.flipped && bg_fg && fg_bg;
				let insert = cond_vars.matches.insert && !(bg_fg && fg_bg); //TODO: effectively just a flipped inverse, kinda dumb?

				let can_append = perfect || flipped || insert;
				if can_append {
					batch.kind = if char_batch_kind == BatchKind::Unique || batch.kind == BatchKind::Unique {
						BatchKind::Unique
					} else if cond_vars.matches.insert && (perfect || flipped) {
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
							*batch = batch.flip();
						} else {
							sym = sym.flip().unwrap();
						}
					}

					batch.chars.push(sym.into());

					if !is_same_as_prev_frame {
						work_batch.last_with_change = work_batch.running.clone();
					}
					continue;
				}
			}
			
			if let Some(work_batch) = work_batch.take() {
				output.push(work_batch.last_with_change);
			}
			if !is_same_as_prev_frame {
				work_batch = Some(WorkBatch::new(Batch {
					kind: char_batch_kind,
					pos: Point { x, y },
					bg: char.bg,
					fg: char.fg,
					chars: char.sym.into_inner().to_string(),
				}));
			}
		}
		if let Some(work_batch) = work_batch {
			output.push(work_batch.last_with_change);
		}
	}

	output
}





fn generate_batches_naive(frame: &TermFrame, prev_frame: Option<&TermFrame>) -> Vec<Batch> {
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

	for y in 0..frame.size().y {
		let mut batch = None::<Batch>;
		for x in 0..frame.size().x {
			let i = y * frame.size().x + x;
			let char = &frame.buffer()[i];
			if let Some(prev_frame) = prev_frame {
				let char_prev = &prev_frame.buffer()[i];
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
					char: CondColors { using_bg: char_batch_kind != BatchKind::FgOnly, using_fg: char_batch_kind != BatchKind::BgOnly },
					batch: CondColors { using_bg: batch.kind != BatchKind::FgOnly, using_fg: batch.kind != BatchKind::BgOnly },
					matches: CondMatches {
						perfect: true, //the only case we don't need it is if both char and batch are different single color batch kinds. for this case it's okay to have perfect matching on since it's impossible it match then, meaning we can just the entire perfect matcher to always occur
						flipped: char_batch_kind != batch.kind || char_batch_kind == BatchKind::Flippable, //flip everywhere except along the diagonal, with the exception of the flippables intersection where we still flip
						insert: char_batch_kind.is_single_color() && batch.kind.is_single_color(), //is both are sing colors, that leaves one color for each, so we can insert
					},
				};

				let bg_bg = (cond_vars.char.using_bg && cond_vars.batch.using_bg) && char.bg == batch.bg;
				let bg_fg = (cond_vars.char.using_bg && cond_vars.batch.using_fg) && char.bg == batch.fg;
				let fg_bg = (cond_vars.char.using_fg && cond_vars.batch.using_bg) && char.fg == batch.bg;
				let fg_fg = (cond_vars.char.using_fg && cond_vars.batch.using_fg) && char.fg == batch.fg;

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
							*batch = batch.flip();
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

fn draw_batches_naive<>(renderer: &mut impl Renderer, mut batches: Vec<Batch>) {
	fn batch_cost(state: &RenderState, batch: &Batch) -> usize {
		let mut cost = 0;
		if state.bg != Some(batch.bg) && batch.kind != BatchKind::FgOnly { cost += 1 }
		if state.fg != Some(batch.fg) && batch.kind != BatchKind::BgOnly { cost += 1 }
		cost
	}

	fn batch_cost_with_flip(state: &RenderState, batch: &Batch) -> (usize, bool) {
		let perfect_cost = batch_cost(&state, &batch);
		if batch.kind != BatchKind::Unique {
			let flipped_cost = batch_cost(&state, &batch.flip());
			if flipped_cost < perfect_cost {
				return (flipped_cost, true);
			}
		}
		(perfect_cost, false)
	}

	let mut state = RenderState::new();
	while let Some(batch) = batches
		.iter()
		.enumerate()
		.map(|(i, batch)| (i, batch, batch_cost_with_flip(&state, &batch)))
		.min_by_key(|(_i, _batch, (cost, _needs_flip))| *cost)
		.map(|(i, _batch, (_cost, needs_flip))| (i, needs_flip))
		.map(|(i, needs_flip)| {
			let batch = batches.swap_remove(i);
			if needs_flip { batch.flip() } else { batch }
		})
	{
		batch.draw(renderer);
		state.bg = Some(batch.bg);
		state.fg = Some(batch.fg);
	}
}

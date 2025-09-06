use std::{
	cell::{Cell, RefCell},
	collections::{hash_map::{Entry, OccupiedEntry}, HashMap},
	ops::{Deref, DerefMut, Index, IndexMut},
	rc::Rc,
};


use crate::{oc_color::PackedColor, math::Point};

use super::super::{TermFrame, Renderer};

#[derive(PartialEq, Eq, Clone, Copy)]
enum BucketKind {
	Basic,
	Flippable,
	Flipped,
}

#[derive(Debug)]
struct Bucket {
	pixel_indices: Vec<Cell<usize>>,
	index_lookup: HashMap<usize, usize>,
}

impl Bucket {
	pub fn new() -> Self {
		Self {
			pixel_indices: Vec::new(),
			index_lookup: HashMap::new(),
		}
	}

	pub fn is_empty(&self) -> bool {
		debug_assert_eq!(self.pixel_indices.len(), self.index_lookup.len());
		self.pixel_indices.is_empty()
	}

	pub fn clear(&mut self) {
		self.pixel_indices.clear();
		self.index_lookup.clear();
	}

	pub fn push(&mut self, value: usize) {
		self.index_lookup.insert(value, self.pixel_indices.len());
		self.pixel_indices.push(Cell::new(value));
	}
}

#[derive(Debug)]
struct ColorBuckets {
	basic: Rc<RefCell<Bucket>>,
	flippable: Rc<RefCell<Bucket>>,
	flipped: Rc<RefCell<Bucket>>,
}

impl ColorBuckets {
	fn new() -> Self {
		Self {
			basic: Rc::new(RefCell::new(Bucket::new())),
			flippable: Rc::new(RefCell::new(Bucket::new())),
			flipped: Rc::new(RefCell::new(Bucket::new())),
		}
	}
	
	fn is_empty(&self) -> bool {
		self.basic.deref().borrow().is_empty() &&
		self.flippable.deref().borrow().is_empty() &&
		self.flipped.deref().borrow().is_empty()
	}
}

impl Index<BucketKind> for ColorBuckets {
	type Output = Rc<RefCell<Bucket>>;

	fn index(&self, index: BucketKind) -> &Self::Output {
		match index {
			BucketKind::Basic => &self.basic,
			BucketKind::Flippable => &self.flippable,
			BucketKind::Flipped => &self.flipped,
		}
	}
}

impl IndexMut<BucketKind> for ColorBuckets {
	fn index_mut(&mut self, index: BucketKind) -> &mut Self::Output {
		match index {
			BucketKind::Basic => &mut self.basic,
			BucketKind::Flippable => &mut self.flippable,
			BucketKind::Flipped => &mut self.flipped,
		}
	}
}

type ColorTable = HashMap<PackedColor, HashMap<PackedColor, ColorBuckets>>;

#[derive(Debug)]
struct ColorTables {
	bg_to_fgs: ColorTable,
	fg_to_bgs: ColorTable,
}

struct RenderState {
	bg: PackedColor,
	fg: PackedColor,
}

impl RenderState {
	fn new<>(renderer: &impl Renderer) -> Self {
		Self {
			bg: renderer.get_background(),
			fg: renderer.get_foreground(),
		}
	}
}

impl ColorTables {
	pub fn is_empty(&self) -> bool {
		debug_assert_eq!(self.bg_to_fgs.len(), self.fg_to_bgs.len());
		self.bg_to_fgs.is_empty()
	}
}

fn generate_color_tables(frame: &TermFrame, prev_frame: Option<&TermFrame>) -> ColorTables {
	let mut bg_to_fgs = ColorTable::new();
	let mut fg_to_bgs = ColorTable::new();

	fn ensure_bucket(color_table: &mut ColorTable, bg: PackedColor, fg: PackedColor) -> &mut ColorBuckets {
		color_table
			.entry(bg)
			.or_insert_with(HashMap::new)
			.entry(fg)
			.or_insert_with(ColorBuckets::new)
	}

	for (i, pixel) in frame.buffer().iter().enumerate() {
		if let Some(prev_frame) = prev_frame { //TODO: move out of this later? just take in a batch vector instead of frames
			let prev_pixel = &prev_frame.buffer()[i];
			if pixel == prev_pixel {
				continue;
			}
		}

		let can_flip = pixel.sym.flip().is_some();
		let bucket_kind = if can_flip { BucketKind::Flippable } else { BucketKind::Basic };

		let fg_buckets = ensure_bucket(&mut bg_to_fgs, pixel.bg, pixel.fg);
		let bucket = fg_buckets[bucket_kind].clone();
		if bucket.deref().borrow().is_empty() {
			ensure_bucket(&mut fg_to_bgs, pixel.fg, pixel.bg)[bucket_kind] = bucket.clone();
			
			if bucket_kind == BucketKind::Flippable {
				ensure_bucket(&mut bg_to_fgs, pixel.fg, pixel.bg).flipped = bucket.clone();
				ensure_bucket(&mut fg_to_bgs, pixel.bg, pixel.fg).flipped = bucket.clone();
			}
		}

		bucket.deref().borrow_mut().push(i);
	}

	ColorTables {
		bg_to_fgs,
		fg_to_bgs,
	}
}

fn get_buckets<'a>(color_tables: &'a mut ColorTables, render_state: &mut RenderState) -> Option<&'a ColorBuckets> {
	fn get_next_entry<K, V>(map: &HashMap<K, V>) -> Option<(&K, &V)> {
		map.iter().next()
	}

	if let Some(fgs) = color_tables.bg_to_fgs.get(&render_state.bg) {
		if let Some(buckets) = fgs.get(&render_state.fg) {
			Some(buckets)
		} else {
			get_next_entry(&fgs)
				.map(|(&fg, buckets)| {
					render_state.fg = fg;
					buckets
				})
		}
	} else {
		if let Some(bgs) = color_tables.fg_to_bgs.get(&render_state.fg) {
			get_next_entry(&bgs)
				.map(|(&bg, buckets)| {
					render_state.bg = bg;
					buckets
				})
		} else {
			get_next_entry(&color_tables.bg_to_fgs)
				.and_then(|(&bg, fgs)|
					get_next_entry(&fgs)
						.map(|(&fg, buckets)| {
							render_state.bg = bg;
							render_state.fg = fg;
							buckets
						}))
		}
	}
}

fn draw_bucket(renderer: &mut impl Renderer, bucket: &mut Bucket, frame: &TermFrame, should_flip: bool) {
	for pixel_index in bucket.pixel_indices.iter() {
		let pixel_index = pixel_index.get();
		if pixel_index == usize::MAX { continue; }
		
		let pos = Point {
			x: pixel_index % frame.size().y,
			y: pixel_index / frame.size().x,
		};
		let mut pixel = frame.buffer()[pixel_index].clone();
		if should_flip {
			pixel = pixel.flip().unwrap();
		}

		let pixel_iter_end = pixel_index - pos.x + frame.size().x;
		let mut syms = pixel.sym.into_inner().to_string();
		let mut pixel_iter = pixel_index + 1;
		while pixel_iter < pixel_iter_end && bucket.index_lookup.contains_key(&pixel_iter) {
			let mut sym = frame.buffer()[pixel_iter].sym;
			if should_flip {
				sym = sym.flip().unwrap();
			}
			syms.push(sym.into_inner());

			bucket.pixel_indices[bucket.index_lookup[&pixel_iter]].set(usize::MAX);
			pixel_iter += 1;
		}

		if !matches!(pixel.sym.into_inner(), '█' | '⣿') {
			renderer.set_background(pixel.bg);
		}
		if !matches!(pixel.sym.into_inner(), ' ' | '⠀') {
			renderer.set_foreground(pixel.fg);
		}

		renderer.set(&pos, &syms);
	}
}

fn draw_buckets(renderer: &mut impl Renderer, buckets: &ColorBuckets, frame: &TermFrame) -> (bool, bool) {
	draw_bucket(renderer, buckets.basic.borrow_mut().deref_mut(), frame, false);
	draw_bucket(renderer, buckets.flippable.borrow_mut().deref_mut(), frame, false);
	draw_bucket(renderer, buckets.flipped.borrow_mut().deref_mut(), frame, true);
	(!buckets.flippable.deref().borrow().is_empty(), !buckets.flipped.deref().borrow().is_empty())
}

fn remove_bucket_listings(color_tables: &mut ColorTables, render_state: &RenderState, has_flippable_bucket: bool, has_flipped_bucket: bool) {
	{
		fn unwrap_entry<'a, K, V>(entry: Entry<'a, K, V>) -> OccupiedEntry<'a, K, V> {
			match entry {
				Entry::Occupied(occupied_entry) => occupied_entry,
				Entry::Vacant(_vacant_entry) => panic!("called `unwrap_entry()` on a `Vacant` value"),
			}
		}

		let mut fgs = unwrap_entry(color_tables.bg_to_fgs.entry(render_state.bg));
		fgs.get_mut().remove(&render_state.fg);
		if fgs.get().is_empty() {
			fgs.remove();
		}
		let mut bgs = unwrap_entry(color_tables.fg_to_bgs.entry(render_state.fg));
		bgs.get_mut().remove(&render_state.bg);
		if bgs.get().is_empty() {
			bgs.remove();
		}
	}

	if has_flippable_bucket || has_flipped_bucket {
		if let Entry::Occupied(mut fgs) = color_tables.bg_to_fgs.entry(render_state.fg) {
			if let Entry::Occupied(mut fg_buckets) = fgs.get_mut().entry(render_state.bg) {
				if has_flippable_bucket {
					fg_buckets.get_mut().flipped.deref().borrow_mut().clear();
				}
				if has_flipped_bucket {
					fg_buckets.get_mut().flippable.deref().borrow_mut().clear();
				}

				if fg_buckets.get().is_empty() {
					fg_buckets.remove();
				}
			}

			if fgs.get().is_empty() {
				fgs.remove();
			}
		}
		if let Entry::Occupied(mut bgs) = color_tables.fg_to_bgs.entry(render_state.bg) {
			if let Entry::Occupied(mut bg_buckets) = bgs.get_mut().entry(render_state.fg) {
				if has_flippable_bucket {
					bg_buckets.get_mut().flipped.deref().borrow_mut().clear();
				}
				if has_flipped_bucket {
					bg_buckets.get_mut().flippable.deref().borrow_mut().clear();
				}

				if bg_buckets.get().is_empty() {
					bg_buckets.remove();
				}
			}

			if bgs.get().is_empty() {
				bgs.remove();
			}
		}
	}
}

pub fn draw(renderer: &mut impl Renderer, frame: &TermFrame, prev_frame: Option<&TermFrame>) {
	let mut color_tables = generate_color_tables(&frame, prev_frame);

	let mut render_state = RenderState::new(renderer);
	while !color_tables.is_empty() {
		//dbg(&color_tables);
		let buckets = get_buckets(&mut color_tables, &mut render_state).unwrap();
		let (has_flippable_bucket, has_flipped_bucket) = draw_buckets(renderer, buckets, frame);
		remove_bucket_listings(&mut color_tables, &render_state, has_flippable_bucket, has_flipped_bucket);
	}
}

fn dbg(color_tables: &ColorTables) {
	println!("\nTABLES:");
	println!("  bg -> fgs");
	for fgs in color_tables.bg_to_fgs.iter() {
		println!("    bg: {}", fgs.0);
		for fg_bucket in fgs.1.iter() {
			println!("      fg: {}", fg_bucket.0);
			println!("        B: {:?}", fg_bucket.1.basic);
			println!("        f: {:?}", fg_bucket.1.flippable);
			println!("        F: {:?}", fg_bucket.1.flipped);
		}
	}
	println!("  fg -> bgs");
	for bgs in color_tables.fg_to_bgs.iter() {
		println!("    fg: {}", bgs.0);
		for bg_bucket in bgs.1.iter() {
			println!("      bg: {}", bg_bucket.0);
			println!("        B: {:?}", bg_bucket.1.basic);
			println!("        f: {:?}", bg_bucket.1.flippable);
			println!("        F: {:?}", bg_bucket.1.flipped);
		}
	}
}

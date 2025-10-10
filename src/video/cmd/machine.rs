use crate::math::Frac;

pub struct Machine {
	pub color_cost: Frac<usize>,
	pub set_cost: Frac<usize>,
	pub set_palette_color_cost: Frac<usize>,
	pub copy_cost: Frac<usize>,
	pub fill_cost: Frac<usize>,
	pub bitblt_cost: Frac<usize>,
	pub call_budget: Frac<usize>,
}

impl Machine {
	pub fn new_t1() -> Self {
		let tier = 1usize;
		Self {
			//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
			color_cost: Frac::new(1, 32),
			set_cost: Frac::new(1, 64),
			set_palette_color_cost: Frac::new(1, 2),
			copy_cost: Frac::new(1, 16),
			fill_cost: Frac::new(1, 32),
			bitblt_cost: Frac::new(1, 2) * (1usize << tier),

			//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
			call_budget: Frac::new(1, 2),
		}
	}

	pub fn new_t2() -> Self {
		let tier = 2usize;
		Self {
			//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
			color_cost: Frac::new(1, 64),
			set_cost: Frac::new(1, 128),
			set_palette_color_cost: Frac::new(1, 8),
			copy_cost: Frac::new(1, 32),
			fill_cost: Frac::new(1, 64),
			bitblt_cost: Frac::new(1, 2) * (1usize << tier),
			
			//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
			call_budget: Frac::new(2, 2),
		}
	}

	pub fn new_t3() -> Self {
		let tier = 3usize;
		Self {
			//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
			color_cost: Frac::new(1, 128),
			set_cost: Frac::new(1, 256),
			set_palette_color_cost: Frac::new(1, 16),
			copy_cost: Frac::new(1, 64),
			fill_cost: Frac::new(1, 128),
			bitblt_cost: Frac::new(1, 2) * (1usize << tier),
			
			//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
			call_budget: Frac::new(3, 2),
		}
	}
}

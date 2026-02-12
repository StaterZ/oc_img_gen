use num_traits::ConstZero;

use crate::math::*;

pub struct Machine {
	pub cpu_speed: Frac<usize>,

	pub set_resolution_cost: Frac<usize>,
	pub set_color_cost: Frac<usize>,
	pub set_cost: Frac<usize>,
	pub set_palette_color_cost: Frac<usize>,
	pub copy_cost: Frac<usize>,
	pub fill_cost: Frac<usize>,
	pub bitblt_cost: Frac<usize>,
	pub call_budget: Frac<usize>,

	pub max_screen_size: Size::<usize>,
}

impl Machine {
	pub fn bitblt_cost(&self, size: Size<usize>) -> Frac<usize> {
		self.bitblt_cost * size.area() / self.max_screen_size.area()
	}

	pub const T1: Self = Self {
		cpu_speed: Frac::new(1, 1),

		set_resolution_cost: Frac::ZERO, //TODO
		
		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
		set_color_cost: Frac::new(1, 32),
		set_cost: Frac::new(1, 64),
		set_palette_color_cost: Frac::new(1, 2),
		copy_cost: Frac::new(1, 16),
		fill_cost: Frac::new(1, 32),
		bitblt_cost: Frac::new(1, 2),

		//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
		call_budget: Frac::new(1, 2),

		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/Settings.scala#L28
		max_screen_size: Size::new(50, 16),
	};

	pub const T2: Self = Self {
		cpu_speed: Frac::new(1, 1),

		set_resolution_cost: Frac::ZERO, //TODO

		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
		set_color_cost: Frac::new(1, 64),
		set_cost: Frac::new(1, 128),
		set_palette_color_cost: Frac::new(1, 8),
		copy_cost: Frac::new(1, 32),
		fill_cost: Frac::new(1, 64),
		bitblt_cost: Frac::new(2, 2),
		
		//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
		call_budget: Frac::new(2, 2),

		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/Settings.scala#L28
		max_screen_size: Size::new(80, 25),
	};

	pub const T3: Self = Self {
		cpu_speed: Frac::new(1, 1),
		
		set_resolution_cost: Frac::ZERO, //TODO
		
		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L64
		set_color_cost: Frac::new(1, 128),
		set_cost: Frac::new(1, 256),
		set_palette_color_cost: Frac::new(1, 16),
		copy_cost: Frac::new(1, 64),
		fill_cost: Frac::new(1, 128),
		bitblt_cost: Frac::new(4, 2),
		
		//https://github.com/MightyPirates/OpenComputers/blob/571482db88080d56329e8f8cf0db2a90825bf1d7/src/main/scala/li/cil/oc/server/machine/Machine.scala#L121
		call_budget: Frac::new(3, 2),

		//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/Settings.scala#L28
		max_screen_size: Size::new(160, 50),
	};
}

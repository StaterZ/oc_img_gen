use num::traits::ConstZero;

use crate::math::*;

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Tier {
	#[value(name = "1")] T1 = 1,
	#[value(name = "2")] T2 = 2,
	#[value(name = "3")] T3 = 3,
	
	#[value(name = "emu")] Emulator = 0,
}

pub struct Machine {
	pub cpu_speed: Frac<usize>,

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

	//share of a tick's call budget that one direct call of `cost` effectively uses
	//the budget refills to full every tick, unused budget does not carry over
	//a call that doesn't fit in what is left throws LimitReachedException and machine.lua retries it as a synchronized call on the next tick
	//synchronized calls never consume budget, so every tick fits floor(call_budget / cost) charged calls plus one free one
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/machine/Machine.scala#L281-L289
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/machine/Machine.scala#L519-L520
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/resources/assets/opencomputers/lua/machine.lua#L1080-L1103
	pub fn effective_call_cost(&self, cost: Frac<usize>) -> Frac<usize> {
		if cost == Frac::ZERO { return Frac::ZERO; }
		if cost > self.call_budget { return self.call_budget; } //never fits, so it always runs synchronized and takes a whole tick
		self.call_budget / ((self.call_budget / cost).into_int_trunc() + 1)
	}

	//share of a tick's call budget that bitblitting a dirty vram buffer of `size` to the screen effectively uses
	//going over the gpu's tier credit (which equals call_budget on every tier) throws once, so the retry runs synchronized on the next tick
	//there it costs no budget, but pauses for however far it went over a second tier credit, measured in tier credits (one per tick) and truncated to whole ticks
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L196-L208
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/component/GraphicsCard.scala#L233-L251
	//https://github.com/MightyPirates/OpenComputers/blob/master-MC1.7.10/src/main/scala/li/cil/oc/server/machine/Machine.scala#L250-L251
	pub fn present_cost(&self, size: Size<usize>) -> Frac<usize> {
		let cost = self.bitblt_cost(size);
		let credit = self.call_budget;
		if cost <= credit { return self.effective_call_cost(cost); }
		let pause_ticks = if cost > credit * 2 { ((cost - credit * 2) / credit).into_int_trunc() } else { 0 };
		credit * (1 + pause_ticks)
	}

	pub fn compute_max_resolution(&self, ratio: Frac<usize>) -> Size<usize> {
		Self::compute_max_resolution_impl(ratio, self.max_screen_size.area(), self.max_screen_size.w)
	}

	fn compute_max_resolution_impl(ratio: Frac<usize>, max_pixels: usize, max_width: usize) -> Size<usize> {
		debug_assert!(ratio.numerator > 0 && ratio.denominator > 0, "Invalid ratio");
		debug_assert!(max_pixels > 0, "max_pixels must be positive");

		// Start by assuming width is limited by pixel count
		// width * height <= max_pixels
		// height = width / ratio
		// so: width^2 / ratio <= max_pixels
		// => width <= sqrt(max_pixels * ratio)
		let width_limit = (ratio * max_pixels).sqrt();

		let width = width_limit.min(max_width.into());
		let height = width / ratio;

		Size::new(width.into_int_trunc(), height.into_int_trunc())
	}
}

impl From<Tier> for Machine {
	fn from(value: Tier) -> Self {
		match value {
			Tier::T1 => Self {
				cpu_speed: Frac::new(1, 1),

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
			},
			Tier::T2 => Self {
				cpu_speed: Frac::new(1, 1),

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
			},
			Tier::T3 => Self {
				cpu_speed: Frac::new(1, 1),
				
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
			},
			Tier::Emulator => Self {
				cpu_speed: Frac::new(1, 1),
				
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
				max_screen_size: Size::new(255, 255),
			},
		}
	}
}

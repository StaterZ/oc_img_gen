use szu::math::GoodNum;

use super::{Frac, IntoIntRound};

// exact ratios for ints, plain division for floats
pub trait HasRatio: GoodNum {
	type Ratio: GoodNum;

	fn ratio(num: Self, den: Self) -> Self::Ratio;
	fn mul_ratio(self, ratio: Self::Ratio) -> Self;
	fn div_ratio(self, ratio: Self::Ratio) -> Self;
}

macro_rules! impl_int_ratio {
	($($t:ty),*) => {$(
		impl HasRatio for $t {
			type Ratio = Frac<$t>;

			fn ratio(num: Self, den: Self) -> Self::Ratio {
				Frac::new(num, den)
			}
			fn mul_ratio(self, ratio: Self::Ratio) -> Self {
				(Frac::whole(self) * ratio).into_int_round()
			}
			fn div_ratio(self, ratio: Self::Ratio) -> Self {
				(Frac::whole(self) / ratio).into_int_round()
			}
		}
	)*};
}
impl_int_ratio!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize);

macro_rules! impl_float_ratio {
	($($t:ty),*) => {$(
		impl HasRatio for $t {
			type Ratio = $t;

			fn ratio(num: Self, den: Self) -> Self::Ratio {
				num / den
			}
			fn mul_ratio(self, ratio: Self::Ratio) -> Self {
				self * ratio
			}
			fn div_ratio(self, ratio: Self::Ratio) -> Self {
				self / ratio
			}
		}
	)*};
}
impl_float_ratio!(f32, f64);

use std::{fmt::Display, ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Rem, RemAssign, Sub, SubAssign}};

use deku::{no_std_io, prelude::*};
use num_traits::{ConstOne, ConstZero, Float, One, PrimInt, Zero};

pub trait GCD: Copy {
	fn gcd(x: Self, y: Self) -> Self;
}
impl<T: num::Integer + Copy> GCD for T {
	fn gcd(x: Self, y: Self) -> Self {
		num::integer::gcd(x, y)
	}
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub struct Frac<T: PrimInt + GCD> {
	pub numerator: T,
	pub denominator: T,
}

impl<T: PrimInt + GCD> Frac<T> {
	pub const fn new(numerator: T, denominator: T) -> Self {
		Self { numerator, denominator }
	}

	pub fn into_int_trunc(self) -> T {
		self.numerator / self.denominator
	}
	pub fn into_int(self) -> T {
		let two = T::one() + T::one();
		(self.numerator + (self.denominator / two)) / self.denominator
	}
	fn into_flt<U: Float>(self) -> U {
		U::from(self.numerator).unwrap() / U::from(self.denominator).unwrap()
	}

	pub fn cast<U: PrimInt + GCD + From<T>>(self) -> Frac::<U> {
		Frac {
			numerator: self.numerator.into(),
			denominator: self.denominator.into(),
		}
	}
	pub fn try_cast<U: PrimInt + GCD + TryFrom<T>>(self) -> Result<Frac::<U>, U::Error> {
		Ok(Frac {
			numerator: self.numerator.try_into()?,
			denominator: self.denominator.try_into()?,
		})
	}

	fn gcd(self) -> Self {
		let gcd = T::gcd(self.numerator, self.denominator);
		Self {
			numerator: self.numerator / gcd,
			denominator: self.denominator / gcd,
		}
	}
}

impl<T: PrimInt + GCD + Zero> Zero for Frac<T> {
	fn zero() -> Self {
		Self::from(T::zero())
	}

	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: PrimInt + GCD + ConstZero + ConstOne> ConstZero for Frac<T> {
	const ZERO: Self = Self::new(T::ZERO, T::ONE);
}

impl<T: PrimInt + GCD + One> One for Frac<T> {
	fn one() -> Self {
		Self::from(T::one())
	}

	fn is_one(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: PrimInt + GCD + ConstOne> ConstOne for Frac<T> {
	const ONE: Self = Self::new(T::ONE, T::ONE);
}

impl<T: PrimInt + GCD + Display> Display for Frac<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({},{} | ~{})", self.numerator, self.denominator, self.into_flt::<f32>())
	}
}

impl<T: PrimInt + GCD + DekuWriter> DekuWriter for Frac<T> {
	#[doc = " Write type to bytes"]
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: ()) -> Result<(), DekuError> {
		self.numerator.to_writer(writer, ctx)?;
		self.denominator.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<T: PrimInt + GCD> From<T> for Frac<T> {
	fn from(value: T) -> Self {
		Self {
			numerator: value,
			denominator: T::one(),
		}
	}
}

impl From<ffmpeg_next::Rational> for Frac<i32> {
	fn from(value: ffmpeg_next::Rational) -> Self {
		Self {
			numerator: value.numerator(),
			denominator: value.denominator(),
		}
	}
}

impl<T: PrimInt + GCD> PartialOrd for Frac<T> {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		(self.numerator * other.denominator).partial_cmp(&(other.numerator * self.denominator))
	}
}

impl<T: PrimInt + GCD> Add for Frac<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator + rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: PrimInt + GCD> Add<T> for Frac<T> {
	type Output = Self;

	fn add(self, rhs: T) -> Self::Output {
		self + Frac::from(rhs)
	}
}

impl<T: PrimInt + GCD> AddAssign for Frac<T> {
	fn add_assign(&mut self, rhs: Self) {
		*self = *self + rhs;
	}
}

impl<T: PrimInt + GCD> AddAssign<T> for Frac<T> {
	fn add_assign(&mut self, rhs: T) {
		*self = *self + rhs;
	}
}

impl<T: PrimInt + GCD> Sub for Frac<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator - rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: PrimInt + GCD> Sub<T> for Frac<T> {
	type Output = Self;

	fn sub(self, rhs: T) -> Self::Output {
		self - Frac::from(rhs)
	}
}

impl<T: PrimInt + GCD> SubAssign for Frac<T> {
	fn sub_assign(&mut self, rhs: Self) {
		*self = *self - rhs;
	}
}

impl<T: PrimInt + GCD> SubAssign<T> for Frac<T> {
	fn sub_assign(&mut self, rhs: T) {
		*self = *self - rhs;
	}
}

impl<T: PrimInt + GCD> Mul for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.numerator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: PrimInt + GCD> Mul<T> for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		self * Self::from(rhs)
	}
}

impl<T: PrimInt + GCD> MulAssign for Frac<T> {
	fn mul_assign(&mut self, rhs: Self) {
		*self = *self * rhs;
	}
}

impl<T: PrimInt + GCD> MulAssign<T> for Frac<T> {
	fn mul_assign(&mut self, rhs: T) {
		*self = *self * rhs;
	}
}

impl<T: PrimInt + GCD> Div for Frac<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator,
			denominator: self.denominator * rhs.numerator,
		}.gcd()
	}
}

impl<T: PrimInt + GCD> Div<T> for Frac<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		self / Self::from(rhs)
	}
}

impl<T: PrimInt + GCD> DivAssign for Frac<T> {
	fn div_assign(&mut self, rhs: Self) {
		*self = *self / rhs;
	}
}

impl<T: PrimInt + GCD> DivAssign<T> for Frac<T> {
	fn div_assign(&mut self, rhs: T) {
		*self = *self / rhs;
	}
}

impl<T: PrimInt + GCD> Rem for Frac<T> {
	type Output = Self;

	fn rem(self, rhs: Self) -> Self::Output {
		Self {
			numerator: (self.numerator * rhs.denominator) % (self.denominator * rhs.numerator),
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: PrimInt + GCD> Rem<T> for Frac<T> {
	type Output = Self;

	fn rem(self, rhs: T) -> Self::Output {
		self % Self::from(rhs)
	}
}

impl<T: PrimInt + GCD> RemAssign for Frac<T> {
	fn rem_assign(&mut self, rhs: Self) {
		*self = *self % rhs;
	}
}

impl<T: PrimInt + GCD> RemAssign<T> for Frac<T> {
	fn rem_assign(&mut self, rhs: T) {
		*self = *self % rhs;
	}
}

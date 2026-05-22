use std::{fmt::Display, ops::*, str::FromStr};
use deku::{no_std_io, prelude::*};
use integer_sqrt::IntegerSquareRoot;
use itertools::Itertools;
use num_traits::{NumAssignOps, ConstOne, ConstZero, Float, One, PrimInt, Zero};

pub trait GoodInt: PrimInt + NumAssignOps + Copy {
	fn gcd(x: Self, y: Self) -> Self;
	fn abs(x: Self) -> Self;
}
impl<T: num::Integer + PrimInt + NumAssignOps + Copy> GoodInt for T {
	fn gcd(x: Self, y: Self) -> Self {
		num::integer::gcd(x, y)
	}
	fn abs(x: Self) -> Self {
		if x >= Self::zero() { x } else { Self::zero() - x }
	}
}

#[derive(Debug, Clone, Copy, Eq)]
pub struct Frac<T: GoodInt> {
	pub numerator: T,
	pub denominator: T,
}

impl<T: GoodInt> Frac<T> {
	pub const fn new(numerator: T, denominator: T) -> Self {
		Self { numerator, denominator }
	}

	pub fn fract(self) -> Self {
		Self {
			numerator: self.numerator % self.denominator,
			denominator: self.denominator,
		}
	}

	pub fn into_int_trunc(self) -> T {
		self.numerator / self.denominator
	}
	pub fn into_int_frac(self, denominator: T) -> T {
		self.fract().numerator / denominator
	}
	pub fn into_int_round(self) -> T {
		let two = T::one() + T::one();
		let bias = T::abs(self.denominator) / if self.numerator > T::zero() { two } else { T::zero() - two };
		(self.numerator + bias) / self.denominator
	}
	pub fn into_flt<U: Float>(self) -> U {
		U::from(self.numerator).unwrap() / U::from(self.denominator).unwrap()
	}

	pub fn cast<U: GoodInt + From<T>>(self) -> Frac::<U> {
		Frac {
			numerator: self.numerator.into(),
			denominator: self.denominator.into(),
		}
	}
	pub fn try_cast<U: GoodInt + TryFrom<T>>(self) -> Result<Frac::<U>, U::Error> {
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

impl<T: GoodInt + IntegerSquareRoot> Frac<T> {
	pub fn sqrt(self) -> Self {
		Self {
			numerator: (self.numerator * self.denominator).integer_sqrt(),
			denominator: self.denominator,
		}
	}

	pub fn inverse(self) -> Self {
		Self {
			numerator: self.denominator,
			denominator: self.numerator,
		}
	}
}

impl<T: GoodInt + Zero> Zero for Frac<T> {
	fn zero() -> Self {
		Self::from(T::zero())
	}

	fn is_zero(&self) -> bool {
		*self == Self::zero()
	}
}

impl<T: GoodInt + ConstZero + ConstOne> ConstZero for Frac<T> {
	const ZERO: Self = Self::new(T::ZERO, T::ONE);
}

impl<T: GoodInt + One> One for Frac<T> {
	fn one() -> Self {
		Self::from(T::one())
	}
}

impl<T: GoodInt + ConstOne> ConstOne for Frac<T> {
	const ONE: Self = Self::new(T::ONE, T::ONE);
}

impl<T: GoodInt + Display> Display for Frac<T> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "({}/{} | ~{})", self.numerator, self.denominator, self.into_flt::<f32>())
	}
}

impl<T: GoodInt + DekuWriter<Ctx>, Ctx: Copy> DekuWriter<Ctx> for Frac<T> {
	#[doc = "Write type to bytes"]
	fn to_writer<W: no_std_io::Write + no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError> {
		self.numerator.to_writer(writer, ctx)?;
		self.denominator.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<'a, Ctx: Copy, T: GoodInt + DekuReader<'a, Ctx>> DekuReader<'a, Ctx> for Frac<T> {
	fn from_reader_with_ctx<R: no_std_io::Read + no_std_io::Seek>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Self, DekuError> {
		Ok(Self {
			numerator: T::from_reader_with_ctx(reader, ctx)?,
			denominator: T::from_reader_with_ctx(reader, ctx)?,
		})
	}
}


impl<T: GoodInt> From<T> for Frac<T> {
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

#[derive(thiserror::Error, Debug)]
pub enum FracParseErr<E> {
	#[error("Frac must be in NUM or NUM/DENOM format")] BadFormat,
	#[error("parse failed: {0}")] ParseError(#[from] E),
}

impl<T: GoodInt + FromStr> FromStr for Frac<T> {
	type Err = FracParseErr<<T as FromStr>::Err>;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		match s.split('/').collect_vec().as_slice() {
			[denominator] => Ok(Frac::new(T::one(), denominator.parse::<T>()?)),
			[numerator, denominator] => Ok(Frac::new(
				numerator.parse::<T>()?,
				denominator.parse::<T>()?,
			)),
			_ => Err(FracParseErr::BadFormat),
		}
	}
}

impl<T: GoodInt> PartialEq for Frac<T> {
	fn eq(&self, other: &Self) -> bool {
		self.numerator * other.denominator == other.numerator * self.denominator
	}
}

impl<T: GoodInt> PartialOrd for Frac<T> {
	fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
		(self.numerator * other.denominator).partial_cmp(&(other.numerator * self.denominator))
	}
}

impl<T: GoodInt> Ord for Frac<T> {
	fn cmp(&self, other: &Self) -> std::cmp::Ordering {
		(self.numerator * other.denominator).cmp(&(other.numerator * self.denominator))
	}
}

impl<T: GoodInt> Add for Frac<T> {
	type Output = Self;

	fn add(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator + rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: GoodInt> Add<T> for Frac<T> {
	type Output = Self;

	fn add(self, rhs: T) -> Self::Output {
		self + Frac::from(rhs)
	}
}

impl<T: GoodInt> AddAssign for Frac<T> {
	fn add_assign(&mut self, rhs: Self) {
		*self = *self + rhs;
	}
}

impl<T: GoodInt> AddAssign<T> for Frac<T> {
	fn add_assign(&mut self, rhs: T) {
		*self = *self + rhs;
	}
}

impl<T: GoodInt> Sub for Frac<T> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator - rhs.numerator * self.denominator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: GoodInt> Sub<T> for Frac<T> {
	type Output = Self;

	fn sub(self, rhs: T) -> Self::Output {
		self - Frac::from(rhs)
	}
}

impl<T: GoodInt> SubAssign for Frac<T> {
	fn sub_assign(&mut self, rhs: Self) {
		*self = *self - rhs;
	}
}

impl<T: GoodInt> SubAssign<T> for Frac<T> {
	fn sub_assign(&mut self, rhs: T) {
		*self = *self - rhs;
	}
}

impl<T: GoodInt> Mul for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.numerator,
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: GoodInt> Mul<T> for Frac<T> {
	type Output = Self;

	fn mul(self, rhs: T) -> Self::Output {
		self * Self::from(rhs)
	}
}

impl<T: GoodInt> MulAssign for Frac<T> {
	fn mul_assign(&mut self, rhs: Self) {
		*self = *self * rhs;
	}
}

impl<T: GoodInt> MulAssign<T> for Frac<T> {
	fn mul_assign(&mut self, rhs: T) {
		*self = *self * rhs;
	}
}

impl<T: GoodInt> Div for Frac<T> {
	type Output = Self;

	fn div(self, rhs: Self) -> Self::Output {
		Self {
			numerator: self.numerator * rhs.denominator,
			denominator: self.denominator * rhs.numerator,
		}.gcd()
	}
}

impl<T: GoodInt> Div<T> for Frac<T> {
	type Output = Self;

	fn div(self, rhs: T) -> Self::Output {
		self / Self::from(rhs)
	}
}

impl<T: GoodInt> DivAssign for Frac<T> {
	fn div_assign(&mut self, rhs: Self) {
		*self = *self / rhs;
	}
}

impl<T: GoodInt> DivAssign<T> for Frac<T> {
	fn div_assign(&mut self, rhs: T) {
		*self = *self / rhs;
	}
}

impl<T: GoodInt> Rem for Frac<T> {
	type Output = Self;

	fn rem(self, rhs: Self) -> Self::Output {
		Self {
			numerator: (self.numerator * rhs.denominator) % (self.denominator * rhs.numerator),
			denominator: self.denominator * rhs.denominator,
		}.gcd()
	}
}

impl<T: GoodInt> Rem<T> for Frac<T> {
	type Output = Self;

	fn rem(self, rhs: T) -> Self::Output {
		self % Self::from(rhs)
	}
}

impl<T: GoodInt> RemAssign for Frac<T> {
	fn rem_assign(&mut self, rhs: Self) {
		*self = *self % rhs;
	}
}

impl<T: GoodInt> RemAssign<T> for Frac<T> {
	fn rem_assign(&mut self, rhs: T) {
		*self = *self % rhs;
	}
}

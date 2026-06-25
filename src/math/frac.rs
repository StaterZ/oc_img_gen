use std::{fmt::Display, ops::*, str::FromStr, time::Duration,};
use deku::prelude::*;
use integer_sqrt::IntegerSquareRoot;
use itertools::Itertools;
use num::{Num, NumCast, Signed, ToPrimitive};
use num_traits::{ConstOne, ConstZero, Float, One, Zero};
use szu::math::{GoodInt, GoodNum};

pub trait IntoIntRound {
	type Output;
	
	fn into_int_round(self) -> Self::Output;
}
impl<T: GoodInt> IntoIntRound for Frac<T> {
	type Output = T;

	default fn into_int_round(self) -> Self::Output {
		let bias = self.denominator / T::from(2).unwrap();
		(self.numerator + bias) / self.denominator
	}
}
impl<T: GoodInt + Signed> IntoIntRound for Frac<T> {
	fn into_int_round(self) -> Self::Output {
		let bias = self.numerator.signum() * (self.denominator.abs() / T::from(2).unwrap());
		(self.numerator + bias) / self.denominator
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

	pub fn half() -> Self {
		Self::new(T::ONE, T::ONE + T::ONE)
	}

	pub fn into_int_trunc(self) -> T {
		self.numerator / self.denominator
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

impl<T: GoodInt> GoodNum for Frac<T> { }

impl<T: GoodInt> NumCast for Frac<T> {
	fn from<U: num::ToPrimitive>(n: U) -> Option<Self> {
		Some(Self {
			numerator: T::from(n)?,
			denominator: T::one(),
		})
	}
}

impl<T: GoodInt> ToPrimitive for Frac<T> {
	fn to_isize(&self) -> Option<isize> {
		self.into_int_trunc().to_isize()
	}
	
	fn to_i8(&self) -> Option<i8> {
		self.into_int_trunc().to_i8()
	}
	
	fn to_i16(&self) -> Option<i16> {
		self.into_int_trunc().to_i16()
	}
	
	fn to_i32(&self) -> Option<i32> {
		self.into_int_trunc().to_i32()
	}

	fn to_i64(&self) -> Option<i64> {
		self.into_int_trunc().to_i64()
	}

	fn to_i128(&self) -> Option<i128> {
		self.into_int_trunc().to_i128()
	}
	
	fn to_usize(&self) -> Option<usize> {
		self.into_int_trunc().to_usize()
	}
	
	fn to_u8(&self) -> Option<u8> {
		self.into_int_trunc().to_u8()
	}
	
	fn to_u16(&self) -> Option<u16> {
		self.into_int_trunc().to_u16()
	}
	
	fn to_u32(&self) -> Option<u32> {
		self.into_int_trunc().to_u32()
	}
	
	fn to_u64(&self) -> Option<u64> {
		self.into_int_trunc().to_u64()
	}
	
	fn to_u128(&self) -> Option<u128> {
		self.into_int_trunc().to_u128()
	}
	
	fn to_f32(&self) -> Option<f32> {
		self.into_int_trunc().to_f32()
	}
	
	fn to_f64(&self) -> Option<f64> {
		self.into_int_trunc().to_f64()
	}
}

impl<T: GoodInt> Num for Frac<T> {
	type FromStrRadixErr = FracParseErr<<T as Num>::FromStrRadixErr>;

	fn from_str_radix(str: &str, radix: u32) -> Result<Self, Self::FromStrRadixErr> {
		match str.split('/').collect_vec().as_slice() {
			[numerator] => Ok(Frac::new(
				T::from_str_radix(*numerator, radix)?,
				T::one(),
			)),
			[numerator, denominator] => Ok(Frac::new(
				T::from_str_radix(*numerator, radix)?,
				T::from_str_radix(*denominator, radix)?,
			)),
			_ => Err(FracParseErr::BadFormat),
		}
	}
}

impl<T: GoodInt + Zero> Zero for Frac<T> {
	fn zero() -> Self {
		<Self as From<T>>::from(T::zero())
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
		<Self as From<T>>::from(T::one())
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
	fn to_writer<W: deku::no_std_io::Write + deku::no_std_io::Seek>(&self, writer: &mut Writer<W>, ctx: Ctx) -> Result<(), DekuError> {
		self.numerator.to_writer(writer, ctx)?;
		self.denominator.to_writer(writer, ctx)?;
		Ok(())
	}
}

impl<'a, Ctx: Copy, T: GoodInt + DekuReader<'a, Ctx>> DekuReader<'a, Ctx> for Frac<T> {
	fn from_reader_with_ctx<R: deku::no_std_io::Read + deku::no_std_io::Seek>(reader: &mut Reader<R>, ctx: Ctx) -> Result<Self, DekuError> {
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

impl<T: GoodInt + Into<u64> + Into<u32>> From<Frac<T>> for Duration where u128: From<T> {
	fn from(value: Frac<T>) -> Self {
		const NANOS_PER_SEC: u128 = 1_000_000_000;
		let ns = ((value % T::one()).cast::<u128>() * <Frac<u128> as From<u128>>::from(NANOS_PER_SEC)).into_int_trunc() as u32;
		Self::new(
			value.into_int_trunc().into(),
			ns,
		)
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
				T::from_str(numerator)?,
				T::from_str(denominator)?,
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
		self + <Self as From<T>>::from(rhs)
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
		self - <Self as From<T>>::from(rhs)
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
		self * <Self as From<T>>::from(rhs)
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
		self / <Self as From<T>>::from(rhs)
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
		self % <Self as From<T>>::from(rhs)
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

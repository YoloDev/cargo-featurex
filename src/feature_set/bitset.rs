use bytemuck::{bytes_of, bytes_of_mut, Pod};
use std::{
	fmt::{self, Write},
	iter::FusedIterator,
	mem, ops,
};

pub trait BitStorage: Pod {
	fn any(self) -> bool;
	fn bitor(self, other: Self) -> Self;
	fn bitand(self, other: Self) -> Self;
	fn not(self) -> Self;
}

macro_rules! impl_bit_storage {
	($($t:ty),*) => {
		$(
			impl BitStorage for $t {
				#[inline]
				fn any(self) -> bool {
					self != 0
				}

				#[inline]
				fn bitor(self, other: Self) -> Self {
					self | other
				}

				#[inline]
				fn bitand(self, other: Self) -> Self {
					self & other
				}

				#[inline]
				fn not(self) -> Self {
					!self
				}
			}
		)*
	};
}

impl_bit_storage!(u8, u16, u32, u64, u128, usize);

impl<B: BitStorage, const N: usize> BitStorage for [B; N] {
	#[inline]
	fn any(self) -> bool {
		self.iter().any(|v| v.any())
	}

	#[inline]
	fn bitor(self, other: Self) -> Self {
		let mut result = [B::zeroed(); N];
		for i in 0..N {
			result[i] = self[i].bitor(other[i]);
		}
		result
	}

	#[inline]
	fn bitand(self, other: Self) -> Self {
		let mut result = [B::zeroed(); N];
		for i in 0..N {
			result[i] = self[i].bitand(other[i]);
		}
		result
	}

	#[inline]
	fn not(self) -> Self {
		let mut result = [B::zeroed(); N];
		for i in 0..N {
			result[i] = self[i].not();
		}
		result
	}
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BitSet<S: BitStorage = usize> {
	bits: S,
}

impl<S: BitStorage> Default for BitSet<S> {
	fn default() -> Self {
		Self::new()
	}
}

impl<S: BitStorage> fmt::Debug for BitSet<S> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Binary::fmt(self, f)
	}
}

impl<S: BitStorage> fmt::Binary for BitSet<S> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		fmt::Binary::fmt(&BinaryDisplay(self.bits), f)
	}
}

impl<S: BitStorage> BitSet<S> {
	pub fn new() -> Self {
		Self { bits: S::zeroed() }
	}

	pub fn set(&mut self, bit: usize, value: bool) {
		let bytes = bytes_of_mut(&mut self.bits);
		let byte = bit / 8;
		let bit = bit % 8;

		if value {
			bytes[byte] |= 1 << bit;
		} else {
			bytes[byte] &= !(1 << bit);
		}
	}

	#[inline]
	pub fn insert(&mut self, bit: usize) {
		self.set(bit, true);
	}

	// #[inline]
	// pub fn remove(&mut self, bit: usize) {
	// 	self.set(bit, false);
	// }

	#[inline]
	pub fn iter(self) -> Bits<S> {
		self.into_iter()
	}

	#[inline]
	pub fn union(self, other: Self) -> Self {
		self | other
	}

	#[inline]
	pub fn union_with(&mut self, other: Self) {
		*self |= other;
	}

	// #[inline]
	// pub fn intersection(self, other: Self) -> Self {
	// 	self & other
	// }

	// #[inline]
	// pub fn intersection_with(&mut self, other: Self) {
	// 	*self &= other;
	// }

	#[inline]
	pub fn difference(self, other: Self) -> Self {
		self & !other
	}

	#[inline]
	pub fn difference_with(&mut self, other: Self) {
		*self &= !other;
	}

	// #[inline]
	// pub fn symmetric_difference(self, other: Self) -> Self {
	// 	(self | other) - (self & other)
	// }

	// #[inline]
	// pub fn symmetric_difference_with(&mut self, other: Self) {
	// 	*self = (*self | other) - (*self & other);
	// }

	#[inline]
	pub fn is_subset(self, other: Self) -> bool {
		!self.difference(other).any()
	}

	#[inline]
	pub fn is_superset(self, other: Self) -> bool {
		other.is_subset(self)
	}

	// #[inline]
	// pub fn is_disjoint(self, other: Self) -> bool {
	// 	self.intersection(other).any()
	// }

	// #[inline]
	// pub fn is_empty(self) -> bool {
	// 	!self.any()
	// }

	#[inline]
	pub fn any(self) -> bool {
		self.bits.any()
	}
}

impl<S: BitStorage> ops::BitOr for BitSet<S> {
	type Output = Self;

	fn bitor(self, rhs: Self) -> Self::Output {
		Self {
			bits: self.bits.bitor(rhs.bits),
		}
	}
}

impl<S: BitStorage> ops::BitOrAssign for BitSet<S> {
	fn bitor_assign(&mut self, rhs: Self) {
		self.bits = self.bits.bitor(rhs.bits);
	}
}

impl<S: BitStorage> ops::BitAnd for BitSet<S> {
	type Output = Self;

	fn bitand(self, rhs: Self) -> Self::Output {
		Self {
			bits: self.bits.bitand(rhs.bits),
		}
	}
}

impl<S: BitStorage> ops::BitAndAssign for BitSet<S> {
	fn bitand_assign(&mut self, rhs: Self) {
		self.bits = self.bits.bitand(rhs.bits);
	}
}

impl<S: BitStorage> ops::Not for BitSet<S> {
	type Output = Self;

	fn not(self) -> Self::Output {
		Self {
			bits: self.bits.not(),
		}
	}
}

impl<S: BitStorage> ops::Sub for BitSet<S> {
	type Output = Self;

	fn sub(self, rhs: Self) -> Self::Output {
		Self {
			bits: self.bits.bitand(rhs.bits.not()),
		}
	}
}

impl<S: BitStorage> ops::SubAssign for BitSet<S> {
	fn sub_assign(&mut self, rhs: Self) {
		self.bits = self.bits.bitand(rhs.bits.not());
	}
}

pub struct Bits<S: Pod> {
	bits: S,
	remaining: ops::Range<usize>,
}

impl<S: Pod> Iterator for Bits<S> {
	type Item = usize;

	fn next(&mut self) -> Option<Self::Item> {
		let bits = bytes_of(&self.bits);
		while !self.remaining.is_empty() {
			let index = self.remaining.start;
			let byte = index / 8;
			let bit = index % 8;
			self.remaining.start += 1;

			if bits[byte] & (1 << bit) != 0 {
				return Some(index);
			}
		}

		None
	}
}

impl<S: Pod> DoubleEndedIterator for Bits<S> {
	fn next_back(&mut self) -> Option<Self::Item> {
		let bits = bytes_of(&self.bits);
		while !self.remaining.is_empty() {
			let index = self.remaining.end - 1;
			let byte = index / 8;
			let bit = index % 8;
			self.remaining.end -= 1;

			if bits[byte] & (1 << bit) != 0 {
				return Some(index);
			}
		}

		None
	}
}

impl<S: BitStorage> FusedIterator for Bits<S> {}

impl<S: BitStorage> IntoIterator for BitSet<S> {
	type Item = usize;
	type IntoIter = Bits<S>;

	fn into_iter(self) -> Self::IntoIter {
		Bits {
			bits: self.bits,
			remaining: 0..mem::size_of::<S>() * 8,
		}
	}
}

impl<S: BitStorage> FromIterator<usize> for BitSet<S> {
	fn from_iter<I: IntoIterator<Item = usize>>(iter: I) -> Self {
		let mut set = Self::new();
		for bit in iter {
			set.insert(bit);
		}

		set
	}
}

struct BinaryDisplay<S: Pod>(S);

impl<S: Pod> fmt::Binary for BinaryDisplay<S> {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		let bytes = bytes_of(&self.0);
		f.write_str("0b")?;
		for byte in bytes.iter().rev() {
			for bit in 0..8 {
				if bit == 0 || bit == 4 {
					f.write_char('_')?;
				}

				if byte & (1 << bit) != 0 {
					f.write_char('1')?;
				} else {
					f.write_char('0')?;
				}
			}
		}

		Ok(())
	}
}

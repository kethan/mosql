//! Just enough big integers to check an RSA signature.
//!
//! An RSA public key is a modulus and an exponent, and checking a
//! signature is one modular exponentiation. That needs a big integer, and
//! this crate has no dependencies, so here is one: base 2³² limbs, little
//! endian, with schoolbook multiplication and shift-and-subtract division.
//!
//! The division is the slow kind, and that is fine: the public exponent
//! is 65537, so a verification is a few dozen divisions, not thousands.
//! A public exponent the size of the modulus would be slow — and is also
//! something no real provider sends, so it is refused rather than waited
//! on.

use std::cmp::Ordering;

/// A non-negative integer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigUint {
    /// Base 2³² limbs, least significant first, with no leading zeros.
    limbs: Vec<u32>,
}

impl Ord for BigUint {
    fn cmp(&self, other: &BigUint) -> Ordering {
        self.compare(other)
    }
}

impl PartialOrd for BigUint {
    fn partial_cmp(&self, other: &BigUint) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl BigUint {
    /// Zero.
    pub fn zero() -> Self {
        BigUint { limbs: Vec::new() }
    }

    /// One.
    pub fn one() -> Self {
        BigUint { limbs: vec![1] }
    }

    /// The number in `bytes`, most significant byte first.
    pub fn from_bytes_be(bytes: &[u8]) -> Self {
        let mut limbs: Vec<u32> = Vec::with_capacity(bytes.len() / 4 + 1);
        let mut index = bytes.len();
        while index > 0 {
            let start = index.saturating_sub(4);
            let mut chunk = [0u8; 4];
            chunk[4 - (index - start)..].copy_from_slice(&bytes[start..index]);
            limbs.push(u32::from_be_bytes(chunk));
            index = start;
        }
        let mut number = BigUint { limbs };
        number.trim();
        number
    }

    /// The number, most significant byte first, without leading zeros.
    pub fn to_bytes_be(&self) -> Vec<u8> {
        if self.limbs.is_empty() {
            return vec![0];
        }
        let mut out = Vec::with_capacity(self.limbs.len() * 4);
        for limb in self.limbs.iter().rev() {
            out.extend_from_slice(&limb.to_be_bytes());
        }
        let first = out.iter().position(|b| *b != 0).unwrap_or(out.len() - 1);
        out.drain(..first);
        out
    }

    /// The number, most significant byte first, in exactly `len` bytes.
    ///
    /// A number that does not fit is a failure, not a truncation: the
    /// signature check compares the whole block.
    pub fn to_bytes_be_padded(&self, len: usize) -> Option<Vec<u8>> {
        let bytes = self.to_bytes_be();
        if bytes.len() > len {
            return None;
        }
        let mut out = vec![0u8; len - bytes.len()];
        out.extend_from_slice(&bytes);
        Some(out)
    }

    /// Whether the number is zero.
    pub fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    /// How many bits the number has.
    pub fn bit_len(&self) -> usize {
        match self.limbs.last() {
            None => 0,
            Some(top) => (self.limbs.len() - 1) * 32 + (32 - top.leading_zeros() as usize),
        }
    }

    /// One bit of the number.
    pub fn bit(&self, index: usize) -> bool {
        let limb = index / 32;
        match self.limbs.get(limb) {
            None => false,
            Some(value) => (value >> (index % 32)) & 1 == 1,
        }
    }

    fn trim(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    /// The number plus `other`.
    pub fn add(&self, other: &BigUint) -> BigUint {
        let mut limbs = Vec::with_capacity(self.limbs.len().max(other.limbs.len()) + 1);
        let mut carry = 0u64;
        for index in 0..self.limbs.len().max(other.limbs.len()) {
            let a = *self.limbs.get(index).unwrap_or(&0) as u64;
            let b = *other.limbs.get(index).unwrap_or(&0) as u64;
            let sum = a + b + carry;
            limbs.push((sum & 0xffff_ffff) as u32);
            carry = sum >> 32;
        }
        if carry > 0 {
            limbs.push(carry as u32);
        }
        let mut number = BigUint { limbs };
        number.trim();
        number
    }

    /// The number minus `other`. `other` must not be larger.
    pub fn sub(&self, other: &BigUint) -> BigUint {
        debug_assert!(self.compare(other) != Ordering::Less);
        let mut limbs = Vec::with_capacity(self.limbs.len());
        let mut borrow = 0i64;
        for (index, limb) in self.limbs.iter().enumerate() {
            let a = *limb as i64;
            let b = *other.limbs.get(index).unwrap_or(&0) as i64;
            let mut difference = a - b - borrow;
            if difference < 0 {
                difference += 1 << 32;
                borrow = 1;
            } else {
                borrow = 0;
            }
            limbs.push(difference as u32);
        }
        let mut number = BigUint { limbs };
        number.trim();
        number
    }

    /// The number times `other`.
    pub fn mul(&self, other: &BigUint) -> BigUint {
        if self.is_zero() || other.is_zero() {
            return BigUint::zero();
        }
        let mut limbs = vec![0u32; self.limbs.len() + other.limbs.len()];
        for (i, a) in self.limbs.iter().enumerate() {
            let mut carry = 0u64;
            let a = *a as u64;
            for (j, b) in other.limbs.iter().enumerate() {
                let index = i + j;
                let product = a * (*b as u64) + (limbs[index] as u64) + carry;
                limbs[index] = (product & 0xffff_ffff) as u32;
                carry = product >> 32;
            }
            let mut index = i + other.limbs.len();
            while carry > 0 {
                let sum = (limbs[index] as u64) + carry;
                limbs[index] = (sum & 0xffff_ffff) as u32;
                carry = sum >> 32;
                index += 1;
            }
        }
        let mut number = BigUint { limbs };
        number.trim();
        number
    }

    /// How the two numbers compare.
    fn compare(&self, other: &BigUint) -> Ordering {
        if self.limbs.len() != other.limbs.len() {
            return self.limbs.len().cmp(&other.limbs.len());
        }
        for index in (0..self.limbs.len()).rev() {
            match self.limbs[index].cmp(&other.limbs[index]) {
                Ordering::Equal => {}
                other => return other,
            }
        }
        Ordering::Equal
    }

    /// The number divided by `divisor`: the quotient and the remainder.
    pub fn divrem(&self, divisor: &BigUint) -> (BigUint, BigUint) {
        assert!(!divisor.is_zero(), "division by zero");
        if self.compare(divisor) == Ordering::Less {
            return (BigUint::zero(), self.clone());
        }
        let mut quotient = vec![0u32; self.limbs.len()];
        let mut remainder = BigUint::zero();
        for index in (0..self.bit_len()).rev() {
            remainder = remainder.shl1();
            if self.bit(index) {
                remainder.set_low_bit();
            }
            if remainder.cmp(divisor) != Ordering::Less {
                remainder = remainder.sub(divisor);
                quotient[index / 32] |= 1 << (index % 32);
            }
        }
        let mut quotient = BigUint { limbs: quotient };
        quotient.trim();
        (quotient, remainder)
    }

    /// The number left over after dividing by `divisor`.
    pub fn rem(&self, divisor: &BigUint) -> BigUint {
        self.divrem(divisor).1
    }

    /// The number times two.
    fn shl1(&self) -> BigUint {
        let mut limbs = Vec::with_capacity(self.limbs.len() + 1);
        let mut carry = 0u32;
        for limb in &self.limbs {
            limbs.push((limb << 1) | carry);
            carry = limb >> 31;
        }
        if carry > 0 {
            limbs.push(carry);
        }
        let mut number = BigUint { limbs };
        number.trim();
        number
    }

    /// Add one to the lowest bit.
    fn set_low_bit(&mut self) {
        if self.limbs.is_empty() {
            self.limbs.push(1);
            return;
        }
        self.limbs[0] |= 1;
    }

    /// `base` raised to `exponent`, modulo `modulus`.
    pub fn mod_exp(base: &BigUint, exponent: &BigUint, modulus: &BigUint) -> BigUint {
        if modulus.is_zero() {
            return BigUint::zero();
        }
        if modulus.cmp(&BigUint::one()) == Ordering::Equal {
            return BigUint::zero();
        }
        let mut result = BigUint::one();
        let mut factor = base.rem(modulus);
        for index in 0..exponent.bit_len() {
            if exponent.bit(index) {
                result = result.mul(&factor).rem(modulus);
            }
            factor = factor.mul(&factor).rem(modulus);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(value: u64) -> BigUint {
        BigUint::from_bytes_be(&value.to_be_bytes())
    }

    #[test]
    fn numbers_round_trip() {
        let bytes = [0x01, 0x23, 0x45, 0x67, 0x89];
        let number = BigUint::from_bytes_be(&bytes);
        assert_eq!(number.to_bytes_be(), bytes);
        assert_eq!(number.bit_len(), 33);
        assert!(number.bit(32));
        assert!(!number.bit(31));
    }

    #[test]
    fn leading_zeros_go_away() {
        let number = BigUint::from_bytes_be(&[0x00, 0x00, 0x05]);
        assert_eq!(number.to_bytes_be(), vec![5]);
        assert_eq!(number.bit_len(), 3);
    }

    #[test]
    fn arithmetic_is_right() {
        assert_eq!(n(7).add(&n(5)).to_bytes_be(), vec![12]);
        assert_eq!(n(7).sub(&n(5)).to_bytes_be(), vec![2]);
        assert_eq!(n(7).mul(&n(5)).to_bytes_be(), vec![35]);
        let (quotient, remainder) = n(35).divrem(&n(6));
        assert_eq!(quotient.to_bytes_be(), vec![5]);
        assert_eq!(remainder.to_bytes_be(), vec![5]);
    }

    #[test]
    fn division_by_a_larger_number_gives_zero() {
        let (quotient, remainder) = n(5).divrem(&n(9));
        assert_eq!(quotient.to_bytes_be(), vec![0]);
        assert_eq!(remainder.to_bytes_be(), vec![5]);
    }

    #[test]
    fn modular_exponentiation_is_right() {
        // 4^13 mod 497 = 445 — the classic example.
        let base = n(4);
        let exponent = n(13);
        let modulus = n(497);
        assert_eq!(
            BigUint::mod_exp(&base, &exponent, &modulus).to_bytes_be(),
            vec![0x01, 0xbd]
        );
        // And Fermat: a^(p-1) = 1 mod p for a prime p.
        let prime = n(1_000_003);
        let one_less = n(1_000_002);
        assert_eq!(
            BigUint::mod_exp(&n(2), &one_less, &prime).to_bytes_be(),
            vec![1u8]
        );
    }

    #[test]
    fn padding_is_exact() {
        let number = BigUint::from_bytes_be(&[0x01, 0x02]);
        assert_eq!(
            number.to_bytes_be_padded(4),
            Some(vec![0x00, 0x00, 0x01, 0x02])
        );
        assert_eq!(number.to_bytes_be_padded(1), None);
    }
}

use std::{fmt::Debug, ops::{Shr, BitXor, BitAnd, BitOr, Not}};

use itertools::izip;
#[cfg(feature = "rand")]
use rand::{Rng, prelude::Distribution, distributions::Standard};

use crate::consts::ID_LEN;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Id(pub [u8; ID_LEN]);

impl BitXor<Id> for Id {
    type Output = Id;

    fn bitxor(self, rhs: Id) -> Self::Output {
        self.bimap_bytes(rhs, |a, b| a ^ b)
    }
}

impl BitAnd<Id> for Id {
    type Output = Id;

    fn bitand(self, rhs: Id) -> Self::Output {
        self.bimap_bytes(rhs, |a, b| a & b)
    }
}

impl BitOr<Id> for Id {
    type Output = Id;

    fn bitor(self, rhs: Id) -> Self::Output {
        self.bimap_bytes(rhs, |a, b| a | b)
    }
}

impl Not for Id {
    type Output = Id;

    fn not(self) -> Self::Output {
        self.map_bytes(|x| !x)
    }
}

impl Id {
    pub const ZERO: Id = Id([0; ID_LEN]);
    pub const MAX: Id = Id([0xFF; ID_LEN]);

    pub fn create_left_mask(mut len: u8) -> Self {
        let mut res = Self::ZERO.clone();
        let mut i = ID_LEN - 1;
        loop {
            if len > 8 {
                res.0[i] = 0xFF;
            } else {
                res.0[i] = 0xFFu8.shr(8 - len);
                break;
            }
            len -= 8;
            i -= 1;
        }

        res
    }

    pub fn set_bit(self, bit: u8) -> Self {
        let mut res = self;
        res.0[(bit / 8) as usize] |= 1 << (7 - (bit % 8));
        res
    }

    pub fn bimap_bytes(self, rhs: Id, fun: impl Fn(u8, u8) -> u8) -> Self {
        let mut res = Id([0u8; ID_LEN]);
        for (a, b, r) in izip!(&self.0, &rhs.0, &mut res.0) {
            *r = fun(*a, *b);
        }

        res
    }

    pub fn map_bytes(self, fun: impl Fn(u8) -> u8) -> Self {
        let mut res = Id([0u8; ID_LEN]);
        for (a, r) in self.0.iter().zip(&mut res.0) {
            *r = fun(*a);
        }

        res
    }

    pub fn leading_zeros(&self) -> u8 {
        let mut res = 0u8;
        for x in self.0 {
            if x == 0 {
                res += 8;
            } else {
                res += x.leading_zeros() as u8;
                break;
            }
        }
        res
    }

    pub fn bitslice(&self, index: u32, len: u8) -> u8 {
        let entryi = (index / 8) as usize;
        let bytei = index as u8 & 7;
        let byte = self.0[entryi];
        let firstlen = 8.min(len + bytei) - bytei;
        let secondlen = len - firstlen;
        let res = (byte.wrapping_shr((8 - bytei - firstlen) as u32)) &
                (!255u8.wrapping_shl(firstlen as u32));
        if secondlen == 0 {
            res
        } else {
            let byte2l = (bytei + len) - 8;
            let byte2 = self.0[entryi + 1];
            (res << secondlen) | ((byte2 & !(255 >> byte2l)) >> (8 - secondlen))
        }
    }

    pub fn as_short_hex(&self) -> String {
        let hex_id = hex::encode(&self.0);
        hex_id.trim_start_matches('0').to_owned()
    }
}

impl Debug for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let short_id = self.as_short_hex();
        f.debug_tuple("Id").field(&short_id).finish()
    }
}

#[cfg(feature = "rand")]
impl Distribution<Id> for Standard {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Id {
        let mut id = [0u8; ID_LEN];
        for i in &mut id {
            *i = rng.gen();
        }
        Id(id)
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations() {
        let a = Id([1; ID_LEN]);
        let b = Id([0; ID_LEN]);
        // Xor
        assert_eq!(a ^ a, b);
        assert_eq!(a ^ b, a);
        assert_eq!(b ^ a, a);

        // And
        assert_eq!(a & a, a);
        assert_eq!(a & b, b);
        assert_eq!(b & a, b);

        // Or
        assert_eq!(a | a, a);
        assert_eq!(a | b, a);
        assert_eq!(b | a, a);

        // Not
        assert_eq!((!a).0[0], !a.0[0]);
        assert_eq!((!a).0[1], !a.0[1]);
        assert_eq!((!b).0[0], !b.0[1]);
    }

    #[test]
    fn op_mask() {
        let a = Id::create_left_mask(8);
        assert_eq!(a.0[ID_LEN-1], 0xFF);
        assert_eq!(a.0[ID_LEN-2], 0x00);


        let a = Id::create_left_mask(11);
        assert_eq!(a.0[ID_LEN-1], 0xFF);
        assert_eq!(a.0[ID_LEN-2], 0x07);


        let a = Id::ZERO.set_bit(0);
        assert_eq!(a.0[0], 0x80);
        assert_eq!(a.0[1], 0);

        let a = Id::ZERO.set_bit(1);
        assert_eq!(a.0[0], 0x40);
        assert_eq!(a.0[1], 0);

        let a = Id::ZERO.set_bit(9);
        assert_eq!(a.0[0], 0);
        assert_eq!(a.0[1], 0x40);
    }

    #[test]
    fn leading_zeros() {
        let mut a = Id([0; ID_LEN]);
        a.0[9] = 2;
        assert_eq!(a.leading_zeros(), 9 * 8 + 6);
        a.0[0] = 1;
        assert_eq!(a.leading_zeros(), 7);
    }

    #[test]
    fn bitslice() {
        let mut a = Id([0; ID_LEN]);
        a.0[3] = 0b11111111;
        a.0[4] = 0b10101010;
        a.0[5] = 0b01010101;
        assert_eq!(a.bitslice(0, 4), 0);
        assert_eq!(a.bitslice(24, 4), 0b1111);
        assert_eq!(a.bitslice(23, 4), 0b0111);

        assert_eq!(a.bitslice(4 * 8 + 1, 7), 0b0101010);
        assert_eq!(a.bitslice(4 * 8 + 1, 8), 0b01010100);
        assert_eq!(a.bitslice(4 * 8 + 2, 8), 0b10101001);
        assert_eq!(a.bitslice(4 * 8 + 3, 8), 0b01010010);

        assert_eq!(a.bitslice(4 * 8 + 3, 1), 0b0);
        assert_eq!(a.bitslice(4 * 8 + 4, 1), 0b1);

        assert_eq!(a.bitslice(3 * 8 + 4, 8), 0b11111010);
    }
}

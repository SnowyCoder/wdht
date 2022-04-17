use std::fmt::Debug;

use itertools::izip;

use crate::consts::ID_LEN;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Id(pub [u8; ID_LEN]);

impl Id {
    pub fn xor(&self, rhs: &Id) -> Self {
        let mut res = Id([0u8; ID_LEN]);
        for (a, b, r) in izip!(&self.0, &rhs.0, &mut res.0) {
            *r = a ^ b;
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xor() {
        let a = Id([1; ID_LEN]);
        let b = Id([0; ID_LEN]);
        assert_eq!(a.xor(&a), b);
        assert_eq!(a.xor(&b), a);
        assert_eq!(b.xor(&a), a);
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

//! The SSE2 / SSSE3 operations of the original, on eight 16-bit lanes with the same wrapping and
//! signedness, and the rANS step.

/// Eight 16-bit lanes (an `__m128i`).
pub(super) type V = [i16; 8];

/// `_mm_set_epi32(e3, e2, e1, e0)`.
pub(super) fn set32(e3: u32, e2: u32, e1: u32, e0: u32) -> V {
    let w = |x: u32, i: u32| ((x >> (16 * i)) & 0xFFFF) as u16 as i16;
    [
        w(e0, 0),
        w(e0, 1),
        w(e1, 0),
        w(e1, 1),
        w(e2, 0),
        w(e2, 1),
        w(e3, 0),
        w(e3, 1),
    ]
}

/// The low 16 bits of `x` in every lane (shuffle / unpack broadcast).
pub(super) fn splat(x: u64) -> V {
    [x as u16 as i16; 8]
}

pub(super) fn cmpgt(a: V, b: V) -> V {
    std::array::from_fn(|i| if a[i] > b[i] { -1 } else { 0 })
}

pub(super) fn and(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i] & b[i])
}

pub(super) fn add(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].wrapping_add(b[i]))
}

pub(super) fn sub(a: V, b: V) -> V {
    std::array::from_fn(|i| a[i].wrapping_sub(b[i]))
}

pub(super) fn srai(a: V, n: u32) -> V {
    std::array::from_fn(|i| a[i] >> n)
}

/// How many lanes of a compare result are set (`popcnt(movemask)` of the packed compare, or of
/// one compare divided by two).
pub(super) fn lanes(a: V) -> u32 {
    a.iter().filter(|&&x| x != 0).count() as u32
}

/// `old + (((target) - old) >> shift)` per lane, where `target = (mask & cmp) + k`.
pub(super) fn adapt(old: V, cmp: V, mask: V, k: V, shift: u32) -> V {
    add(srai(sub(add(and(cmp, mask), k), old), shift), old)
}

/// rANS step: `(x & mask) + (x >> bits) * (c1 - c0) - c0`.
pub(super) fn rans(x: u64, bits: u32, c0: u16, c1: u16) -> u64 {
    let freq = u64::from(u32::from(c1).wrapping_sub(u32::from(c0)));
    (x & ((1u64 << bits) - 1))
        .wrapping_add((x >> bits).wrapping_mul(freq))
        .wrapping_sub(u64::from(c0))
}

pub(super) fn highest_bit(x: u64) -> Option<u32> {
    (x != 0).then(|| 63 - x.leading_zeros())
}

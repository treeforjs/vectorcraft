//! Deterministic noise for the filters that place things at random: hashes of grid cells, so the
//! same art always renders the same.

/// A well-mixed 64-bit hash of a cell and a salt.
pub(super) fn hash(i: i64, j: i64, salt: u64) -> u64 {
    let mut z = (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ salt;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A hash as a number in [0, 1).
pub(super) fn unit(h: u64) -> f64 {
    (h >> 11) as f64 / (1u64 << 53) as f64
}

/// The cell of a grid of `size` that coordinate `v` falls in (saturating far away).
pub(super) fn cell(v: f64, size: f64) -> i64 {
    (v / size).floor() as i64
}

//! Second-order sigma-delta modulator used only to synthesize DSD test signals.

/// Converts samples in [-1, 1] at the DSD rate into a 1-bit stream packed MSB-first.
pub fn sdm2(input: &[f64]) -> Vec<u8> {
    let mut out = vec![0u8; input.len().div_ceil(8)];
    let (mut i1, mut i2, mut y) = (0.0f64, 0.0f64, 0.0f64);
    for (n, &x) in input.iter().enumerate() {
        i1 += x - y;
        i2 += i1 - y;
        y = if i2 >= 0.0 { 1.0 } else { -1.0 };
        if y > 0.0 {
            out[n / 8] |= 0x80 >> (n % 8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dc_input_sets_proportional_density() {
        // x = 0.5 → density of ones = (1 + 0.5) / 2 = 0.75
        let bits = sdm2(&vec![0.5; 80_000]);
        let ones: u32 = bits.iter().map(|b| b.count_ones()).sum();
        let density = ones as f64 / 80_000.0;
        assert!((density - 0.75).abs() < 0.01, "density {density}");
    }
}

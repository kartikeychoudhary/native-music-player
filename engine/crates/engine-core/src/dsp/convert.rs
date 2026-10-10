use crate::output::format::DeviceSample;

/// Bit-perfect path: left-justified i32 → device integer format, by truncating low (zero) bits.
pub fn write_int_passthrough(src: &[i32], dst: DeviceSample, out: &mut Vec<u8>) {
    out.reserve(src.len() * dst.bytes());
    match dst {
        DeviceSample::I16 => src.iter().for_each(|&s| out.extend_from_slice(&((s >> 16) as i16).to_le_bytes())),
        DeviceSample::I24 => src.iter().for_each(|&s| out.extend_from_slice(&(s >> 8).to_le_bytes()[..3])),
        DeviceSample::I32 => src.iter().for_each(|&s| out.extend_from_slice(&s.to_le_bytes())),
        DeviceSample::F32 => panic!("write_int_passthrough needs an integer device format"),
    }
}

pub fn write_f32_passthrough(src: &[f32], out: &mut Vec<u8>) {
    out.reserve(src.len() * 4);
    src.iter().for_each(|&s| out.extend_from_slice(&s.to_le_bytes()));
}

/// xorshift64* — fast, deterministic, good enough for dither.
pub struct Ditherer { state: u64 }

impl Ditherer {
    pub fn new(seed: u64) -> Self { Self { state: seed.max(1) } }
    fn uniform(&mut self) -> f64 {
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let v = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
    /// Triangular PDF in (-1, 1) LSB.
    pub fn tpdf(&mut self) -> f64 { self.uniform() + self.uniform() }
}

pub fn write_f64(src: &[f64], dst: DeviceSample, mut dither: Option<&mut Ditherer>, out: &mut Vec<u8>) {
    out.reserve(src.len() * dst.bytes());
    if dst == DeviceSample::F32 {
        src.iter().for_each(|&s| out.extend_from_slice(&(s as f32).to_le_bytes()));
        return;
    }
    let scale = (1u64 << (dst.bits() - 1)) as f64;
    for &s in src {
        let d = if dst == DeviceSample::I32 { 0.0 } else { dither.as_deref_mut().map_or(0.0, Ditherer::tpdf) };
        let v = (s * scale + d).round().clamp(-scale, scale - 1.0) as i64;
        match dst {
            DeviceSample::I16 => out.extend_from_slice(&(v as i16).to_le_bytes()),
            DeviceSample::I24 => out.extend_from_slice(&(v as i32).to_le_bytes()[..3]),
            DeviceSample::I32 => out.extend_from_slice(&(v as i32).to_le_bytes()),
            DeviceSample::F32 => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_passthrough_is_lossless() {
        let s24 = [0x7FFF_FF00u32 as i32, -256, 0x0012_3400];
        let mut out = vec![];
        write_int_passthrough(&s24, DeviceSample::I24, &mut out);
        assert_eq!(out, vec![0xFF, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0x34, 0x12, 0x00]);
        out.clear();
        write_int_passthrough(&[1 << 16, -(1 << 16)], DeviceSample::I16, &mut out);
        assert_eq!(out, vec![1, 0, 0xFF, 0xFF]);
        out.clear();
        write_int_passthrough(&[0x1234_0000], DeviceSample::I32, &mut out);
        assert_eq!(out, 0x1234_0000i32.to_le_bytes());
    }

    #[test]
    fn f64_to_i16_dithers_within_one_lsb_and_clips() {
        let mut d = Ditherer::new(7);
        let mut out = vec![];
        write_f64(&vec![0.25; 10_000], DeviceSample::I16, Some(&mut d), &mut out);
        let vals: Vec<i16> = out.as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c)).collect();
        assert!(vals.iter().all(|&v| (8191..=8193).contains(&v)));
        let mean = vals.iter().map(|&v| v as f64).sum::<f64>() / vals.len() as f64;
        assert!((mean - 8192.0).abs() < 0.05);
        out.clear();
        write_f64(&[1.5, -1.5], DeviceSample::I16, None, &mut out);
        assert_eq!(out, vec![0xFF, 0x7F, 0x00, 0x80]);
    }

    #[test]
    fn f32_paths_are_exact() {
        let mut out = vec![];
        write_f32_passthrough(&[0.5, -0.25], &mut out);
        assert_eq!(out, [0.5f32.to_le_bytes(), (-0.25f32).to_le_bytes()].concat());
        out.clear();
        write_f64(&[0.125], DeviceSample::F32, None, &mut out);
        assert_eq!(out, 0.125f32.to_le_bytes());
    }
}

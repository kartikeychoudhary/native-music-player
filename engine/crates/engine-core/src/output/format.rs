#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSample { I16, I24, I32, F32 }

impl DeviceSample {
    pub fn bytes(self) -> usize { match self { Self::I16 => 2, Self::I24 => 3, Self::I32 | Self::F32 => 4 } }
    pub fn bits(self) -> u16 { match self { Self::I16 => 16, Self::I24 => 24, Self::I32 | Self::F32 => 32 } }
    pub fn is_int(self) -> bool { !matches!(self, Self::F32) }
    pub fn wire_name(self) -> &'static str { match self { Self::I16 => "s16", Self::I24 => "s24", Self::I32 => "s32", Self::F32 => "f32" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat { pub sample_rate: u32, pub channels: u16, pub sample: DeviceSample }

impl OutputFormat {
    pub fn bytes_per_frame(&self) -> usize { self.sample.bytes() * self.channels as usize }
}

/// Device bytes to f32 (used by callbacks that must hand floats to the OS). Realtime-safe.
/// Exact for 16- and 24-bit integers: f32 has a 24-bit mantissa.
pub fn bytes_to_f32(sample: DeviceSample, src: &[u8], dst: &mut [f32]) {
    match sample {
        DeviceSample::I16 => for (o, c) in dst.iter_mut().zip(src.as_chunks::<2>().0) { *o = i16::from_le_bytes(*c) as f32 / 32_768.0; },
        DeviceSample::I24 => for (o, c) in dst.iter_mut().zip(src.as_chunks::<3>().0) { *o = (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0; },
        DeviceSample::I32 => for (o, c) in dst.iter_mut().zip(src.as_chunks::<4>().0) { *o = (i32::from_le_bytes(*c) as f64 / 2_147_483_648.0) as f32; },
        DeviceSample::F32 => for (o, c) in dst.iter_mut().zip(src.as_chunks::<4>().0) { *o = f32::from_le_bytes(*c); },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i24_round_trips_through_f32_exactly() {
        let vals = [-(1 << 23), -1, 0, 1, 0x12_3456, (1 << 23) - 1];
        let bytes: Vec<u8> = vals.iter().flat_map(|v: &i32| v.to_le_bytes()[..3].to_vec()).collect();
        let mut f = vec![0f32; vals.len()];
        bytes_to_f32(DeviceSample::I24, &bytes, &mut f);
        let back: Vec<i32> = f.iter().map(|&x| (x as f64 * 8_388_608.0) as i32).collect();
        assert_eq!(back, vals);
    }

    #[test]
    fn i16_and_f32() {
        let mut f = [0f32; 2];
        bytes_to_f32(DeviceSample::I16, &[0x00, 0x40, 0x00, 0xC0], &mut f);
        assert_eq!(f, [0.5, -0.5]);
        bytes_to_f32(DeviceSample::F32, &[0.25f32.to_le_bytes(), 1.0f32.to_le_bytes()].concat(), &mut f);
        assert_eq!(f, [0.25, 1.0]);
    }
}

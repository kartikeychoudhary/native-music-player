//! DSD-over-PCM v1.1: each 24-bit sample = marker (0x05/0xFA alternating per frame) + 16 DSD bits.
pub struct DopPacker { marker_hi: bool }

impl DopPacker {
    pub fn new() -> Self { Self { marker_hi: false } }

    /// `input[ch]` holds MSB-first DSD bytes; an odd trailing byte is padded with 0x69 (idle).
    pub fn pack(&mut self, input: &[Vec<u8>], out: &mut Vec<i32>) {
        let len = input.first().map_or(0, Vec::len);
        let frames = len.div_ceil(2);
        out.reserve(frames * input.len());
        for f in 0..frames {
            let marker: u32 = if self.marker_hi { 0xFA } else { 0x05 };
            for ch in input {
                let b0 = ch[2 * f] as u32;
                let b1 = *ch.get(2 * f + 1).unwrap_or(&0x69) as u32;
                out.push(((marker << 24) | (b0 << 16) | (b1 << 8)) as i32);
            }
            self.marker_hi = !self.marker_hi;
        }
    }
}

impl Default for DopPacker { fn default() -> Self { Self::new() } }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_two_bytes_per_frame_with_alternating_markers() {
        let mut p = DopPacker::new();
        let mut out = vec![];
        p.pack(&[vec![0xA1, 0xA2, 0xA3, 0xA4], vec![0xB1, 0xB2, 0xB3, 0xB4]], &mut out);
        assert_eq!(out.len(), 4); // 2 frames × 2 channels
        let w = |v: i32| (v as u32) >> 8;
        assert_eq!(w(out[0]), 0x05A1A2);
        assert_eq!(w(out[1]), 0x05B1B2);
        assert_eq!(w(out[2]), 0xFAA3A4);
        assert_eq!(w(out[3]), 0xFAB3B4);
        let mut more = vec![];
        p.pack(&[vec![0x01, 0x02], vec![0x03, 0x04]], &mut more);
        assert_eq!(w(more[0]) >> 16, 0x05); // the marker keeps alternating across calls
    }
}

//! MPEG-4 AudioSpecificConfig (ISO 14496-3 §1.6.2.1), just enough to detect SBR/PS.
use protocol::Codec;

const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AacConfig { pub object_type: u8, pub sample_rate: u32, pub channels: u8, pub sbr: bool, pub ps: bool, pub output_rate: u32 }

impl AacConfig {
    pub fn codec(&self) -> Codec {
        if self.ps { Codec::HeAacV2 } else if self.sbr { Codec::HeAac } else { Codec::AacLc }
    }
}

struct Bits<'a> { d: &'a [u8], pos: usize }
impl Bits<'_> {
    fn left(&self) -> usize { self.d.len() * 8 - self.pos }
    fn get(&mut self, n: usize) -> Option<u32> {
        if self.left() < n { return None; }
        let mut v = 0u32;
        for _ in 0..n {
            let bit = (self.d[self.pos / 8] >> (7 - self.pos % 8)) & 1;
            v = (v << 1) | bit as u32;
            self.pos += 1;
        }
        Some(v)
    }
    fn aot(&mut self) -> Option<u8> { let a = self.get(5)?; Some(if a == 31 { 32 + self.get(6)? as u8 } else { a as u8 }) }
    fn rate(&mut self) -> Option<u32> { let i = self.get(4)?; if i == 15 { self.get(24) } else { RATES.get(i as usize).copied() } }
}

pub fn parse_asc(asc: &[u8]) -> Option<AacConfig> {
    let mut b = Bits { d: asc, pos: 0 };
    let mut aot = b.aot()?;
    let sample_rate = b.rate()?;
    let channels = b.get(4)? as u8;
    let (mut sbr, mut ps, mut ext_rate) = (false, false, None);
    if aot == 5 || aot == 29 {
        sbr = true;
        ps = aot == 29;
        ext_rate = Some(b.rate()?);
        aot = b.aot()?;
    }
    // GASpecificConfig for the AAC object types we care about (1, 2, 3, 4, 6, 7, 17, 19..23).
    if channels != 0 && matches!(aot, 1..=4 | 6 | 7 | 17 | 19..=23) {
        let _frame_length = b.get(1)?;
        if b.get(1)? == 1 { b.get(14)?; } // dependsOnCoreCoder → coreCoderDelay
        let ext_flag = b.get(1)?;
        if aot == 6 || aot == 20 { b.get(3)?; }
        if ext_flag == 1 && matches!(aot, 17 | 19..=23) { b.get(3)?; }
        // Backward-compatible (implicit) signaling via sync extensions.
        if !sbr && b.left() >= 16 && b.get(11)? == 0x2B7 && b.aot()? == 5 && b.get(1)? == 1 {
            sbr = true;
            ext_rate = Some(b.rate()?);
            if b.left() >= 12 && b.get(11)? == 0x548 { ps = b.get(1)? == 1; }
        }
    }
    let output_rate = ext_rate.unwrap_or(if sbr { sample_rate * 2 } else { sample_rate });
    Some(AacConfig { object_type: aot, sample_rate, channels, sbr, ps, output_rate })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lc_44k_stereo() {
        let c = parse_asc(&[0x12, 0x10]).unwrap();
        assert_eq!((c.object_type, c.sample_rate, c.channels, c.sbr, c.ps, c.output_rate), (2, 44_100, 2, false, false, 44_100));
        assert_eq!(c.codec(), protocol::Codec::AacLc);
    }

    #[test]
    fn explicit_he_aac() {
        let c = parse_asc(&[0x2B, 0x11, 0x88, 0x00]).unwrap();
        assert!(c.sbr && !c.ps);
        assert_eq!((c.sample_rate, c.output_rate), (24_000, 48_000));
        assert_eq!(c.codec(), protocol::Codec::HeAac);
    }

    #[test]
    fn explicit_he_aac_v2() {
        let c = parse_asc(&[0xEB, 0x09, 0x88, 0x00]).unwrap();
        assert!(c.sbr && c.ps);
        assert_eq!(c.codec(), protocol::Codec::HeAacV2);
    }

    #[test]
    fn backward_compatible_sync_extension() {
        let c = parse_asc(&[0x13, 0x10, 0x56, 0xE5, 0x98]).unwrap();
        assert_eq!(c.object_type, 2);
        assert!(c.sbr);
        assert_eq!(c.output_rate, 48_000);
    }

    #[test]
    fn garbage_is_none() { assert!(parse_asc(&[]).is_none()); }
}

mod common;
use common::*;
use engine_core::decode::dsd::{dff, dsf, DsdDecoder, DsdSource};
use engine_core::decode::{DsdOutput, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn read_all(src: &mut dyn DsdSource) -> Vec<Vec<u8>> {
    let mut all = vec![vec![]; src.channels() as usize];
    let mut buf = vec![vec![]; src.channels() as usize];
    while src.read(&mut buf, 4096).unwrap() > 0 { for (a, b) in all.iter_mut().zip(&buf) { a.extend_from_slice(b); } }
    all
}

#[test]
fn dsf_and_dff_carry_identical_bitstreams() {
    let mut s = dsf::open(&fixture("dsd64.dsf")).unwrap();
    let mut d = dff::open(&fixture("dsd64.dff")).unwrap();
    assert_eq!((s.channels(), s.dsd_rate()), (2, 2_822_400));
    assert_eq!((d.channels(), d.dsd_rate()), (2, 2_822_400));
    let (a, b) = (read_all(s.as_mut()), read_all(d.as_mut()));
    assert_eq!(a[0].len() as u64, s.total_bytes_per_channel());
    assert_eq!(a, b, "DSF (LSB-first, reversed on read) must equal DFF (MSB-first)");
}

fn pcm(name: &str) -> (u32, Vec<f32>) {
    let mut dec = DsdDecoder::open(&fixture(name)).unwrap();
    assert_eq!(dec.info().spec.kind, SampleKind::Float);
    let rate = dec.info().spec.sample_rate;
    let mut out = vec![];
    while let Some(b) = dec.next_block().unwrap() { let PcmData::Float(v) = b.data else { panic!() }; out.extend(v); }
    (rate, out)
}

#[test]
fn dsd64_to_pcm_keeps_tones() {
    let (rate, out) = pcm("dsd64.dsf");
    assert_eq!(rate, 88_200);
    let tail = &out[8192..];
    assert_eq!(dominant_freq(tail, 2, 0, rate), 1000.0);
    assert_eq!(dominant_freq(tail, 2, 1, rate), 3000.0);
    let r = rms(&channel(tail, 2, 0));
    assert!((r - 0.25 / 2f64.sqrt()).abs() < 0.02, "rms {r}");
}

#[test]
fn dsd128_decodes_at_176k() {
    let (rate, out) = pcm("dsd128.dsf");
    assert_eq!(rate, 176_400);
    assert_eq!(dominant_freq(&out[16384..], 2, 0, rate), 1000.0);
}

#[test]
fn dff_decodes() {
    let (rate, out) = pcm("dsd64.dff");
    assert_eq!(rate, 88_200);
    assert_eq!(dominant_freq(&out[8192..], 2, 1, rate), 3000.0);
}

#[test]
fn dop_mode_switches_spec_and_carries_source_bits() {
    let mut dec = DsdDecoder::open(&fixture("dsd64.dsf")).unwrap();
    assert!(dec.set_dsd_output(DsdOutput::Dop));
    assert_eq!(dec.info().spec.sample_rate, 176_400);
    assert_eq!(dec.info().spec.kind, SampleKind::Dop);
    let b = dec.next_block().unwrap().unwrap();
    let PcmData::Int(v) = b.data else { panic!() };
    let bits = read_all(dsf::open(&fixture("dsd64.dsf")).unwrap().as_mut());
    let w = |x: i32| (x as u32) >> 8;
    assert_eq!(w(v[0]) >> 16, 0x05);
    assert_eq!(w(v[2]) >> 16, 0xFA);
    assert_eq!(((w(v[0]) >> 8) & 0xFF) as u8, bits[0][0]);
    assert_eq!((w(v[0]) & 0xFF) as u8, bits[0][1]);
    assert_eq!(((w(v[1]) >> 8) & 0xFF) as u8, bits[1][0]);
}

#[test]
fn seek_moves_to_requested_output_frame() {
    let mut dec = DsdDecoder::open(&fixture("dsd64.dsf")).unwrap();
    let total = dec.info().total_frames.unwrap();
    dec.seek(88_200).unwrap();
    let mut n = 0u64;
    while let Some(b) = dec.next_block().unwrap() { n += b.frames() as u64; }
    assert!((n as i64 - (total as i64 - 88_200)).abs() <= 64, "{n} vs {}", total - 88_200);
}

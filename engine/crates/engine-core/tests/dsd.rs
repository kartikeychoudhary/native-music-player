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

// --- malformed headers must yield Err, never a panic or an abort ---

fn patched(src: &str, dst: &str, patch: impl FnOnce(&mut Vec<u8>)) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aurora-dsd-malformed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut bytes = std::fs::read(fixture(src)).unwrap();
    patch(&mut bytes);
    let path = dir.join(dst);
    std::fs::write(&path, bytes).unwrap();
    path
}

// DSF layout: fmt body starts at byte 40; channels at +12, rate at +16, block size at +32.
fn dsf_with(dst: &str, off: usize, value: u32) -> std::path::PathBuf {
    patched("dsd64.dsf", dst, |b| b[40 + off..40 + off + 4].copy_from_slice(&value.to_le_bytes()))
}

fn assert_rejected(path: &std::path::Path) {
    assert!(DsdDecoder::open(path).is_err(), "{} must be rejected", path.display());
}

#[test]
fn dsf_with_zero_rate_is_rejected() { assert_rejected(&dsf_with("rate0.dsf", 16, 0)); }

#[test]
fn dsf_with_non_dsd_rate_is_rejected() { assert_rejected(&dsf_with("rate44100.dsf", 16, 44_100)); }

#[test]
fn dsf_with_huge_block_size_is_rejected() { assert_rejected(&dsf_with("block_max.dsf", 32, 0xFFFF_FFFF)); }

#[test]
fn dsf_with_zero_channels_is_rejected() { assert_rejected(&dsf_with("ch0.dsf", 12, 0)); }

// Overwrites the 8-byte big-endian size that follows the first occurrence of `id`.
fn dff_with_size(dst: &str, id: &[u8; 4], size: u64) -> std::path::PathBuf {
    patched("dsd64.dff", dst, |b| {
        let at = b.windows(4).position(|w| w == id).expect("chunk id in fixture");
        b[at + 4..at + 12].copy_from_slice(&size.to_be_bytes());
    })
}

#[test]
fn dff_with_short_fs_subchunk_is_rejected() { assert_rejected(&dff_with_size("fs_short.dff", b"FS  ", 2)); }

#[test]
fn dff_with_enormous_subchunk_size_is_rejected() {
    assert_rejected(&dff_with_size("fs_huge.dff", b"FS  ", u64::MAX - 8));
    assert_rejected(&dff_with_size("prop_huge.dff", b"PROP", u64::MAX - 8));
}

/// Opening, switching output, seeking and decoding must never panic; errors are fine.
fn exercise(path: &std::path::Path) {
    let Ok(mut dec) = DsdDecoder::open(path) else { return };
    for mode in [DsdOutput::Dop, DsdOutput::Pcm] {
        dec.set_dsd_output(mode);
        let _ = dec.info().total_frames;
        for frame in [0, 88_200, u64::MAX / 2, u64::MAX] {
            if dec.seek(frame).is_ok() { let _ = dec.next_block(); }
        }
    }
}

#[test]
fn dsf_with_huge_sample_count_does_not_panic() {
    // DSF fmt: sample count (u64) at +24.
    let path = patched("dsd64.dsf", "samples_max.dsf", |b| b[40 + 24..40 + 32].copy_from_slice(&u64::MAX.to_le_bytes()));
    exercise(&path);
    let dec = DsdDecoder::open(&path);
    if let Ok(dec) = dec { assert!(dec.info().total_frames.is_some()); }
    // Eight channels maximise the per-block byte offset arithmetic.
    exercise(&patched("dsd64.dsf", "samples_max_8ch.dsf", |b| {
        b[40 + 24..40 + 32].copy_from_slice(&u64::MAX.to_le_bytes());
        b[40 + 12..40 + 16].copy_from_slice(&8u32.to_le_bytes());
    }));
}

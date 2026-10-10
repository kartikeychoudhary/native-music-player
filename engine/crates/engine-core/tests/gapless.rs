mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::PcmData;

const A_FRAMES: usize = 110_583;
const TOTAL_FRAMES: usize = 6 * 44_100;

fn decode_f64(name: &str) -> Vec<f32> {
    let mut d = SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        match b.data {
            PcmData::Float(v) => out.extend(v),
            PcmData::Int(v) => out.extend(v.iter().map(|&s| s as f32 / 2_147_483_648.0)),
        }
    }
    out
}

fn check_pair(ext: &str, min_snr: f64) {
    let a = decode_f64(&format!("gap_a.{ext}"));
    let b = decode_f64(&format!("gap_b.{ext}"));
    assert_eq!(a.len() / 2, A_FRAMES, "{ext}: part A length");
    assert_eq!((a.len() + b.len()) / 2, TOTAL_FRAMES, "{ext}: total length");
    let joined: Vec<f32> = a.into_iter().chain(b).collect();
    let reference = read_f32le("gap_src.f32le");
    // Look at ±2048 frames around the join: inserted silence or offsets wreck the SNR here.
    let s = (A_FRAMES - 2048) * 2;
    let e = (A_FRAMES + 2048) * 2;
    let snr = snr_db(&reference[s..e], &joined[s..e]);
    assert!(snr >= min_snr, "{ext}: boundary SNR {snr:.1} dB");
}

#[test] fn flac_pair_is_seamless() { check_pair("flac", 90.0); }
#[test] fn mp3_pair_is_seamless() { check_pair("mp3", 25.0); }
// 19.0, not 20.0: ffmpeg's own decode of this pair measures 19.2 dB at the same window (encoder edge).
#[test] fn aac_pair_is_seamless() { check_pair("m4a", 19.0); }
#[test] fn vorbis_pair_is_seamless() { check_pair("ogg", 20.0); }

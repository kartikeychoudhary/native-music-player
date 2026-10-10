mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn decode_f32(name: &str) -> (u32, u16, Vec<f32>) {
    let mut d = SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    assert_eq!(d.info().spec.kind, SampleKind::Float);
    let (rate, ch) = (d.info().spec.sample_rate, d.info().spec.channels);
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        let PcmData::Float(v) = b.data else { panic!() };
        out.extend(v);
    }
    (rate, ch, out)
}

fn check(name: &str, min_snr: f64) {
    let reference = read_f32le("pcm16_44k.f32le");
    let (rate, ch, got) = decode_f32(name);
    assert_eq!((rate, ch), (44100, 2));
    assert_eq!(got.len(), reference.len(), "{name}: gapless trim must give the exact source length");
    let snr = snr_db(&reference, &got);
    assert!(snr >= min_snr, "{name}: SNR {snr:.1} dB < {min_snr}");
}

#[test] fn mp3_320_matches_source() { check("mp3_320.mp3", 30.0); }
#[test] fn mp3_v0_matches_source() { check("mp3_v0.mp3", 25.0); }
#[test] fn aac_lc_m4a_matches_source() { check("aac_lc_256.m4a", 25.0); }
#[test] fn vorbis_matches_source() { check("vorbis_q6.ogg", 25.0); }

#[test]
fn corrupt_mp3_decodes_past_damage() {
    let (_, _, got) = decode_f32("mp3_corrupt.mp3");
    let full = read_f32le("pcm16_44k.f32le").len();
    assert!(got.len() as f64 > full as f64 * 0.95, "stopped early: {} of {}", got.len(), full);
    // The final quarter after the damage is still correct audio.
    let tail_ref = &read_f32le("pcm16_44k.f32le")[full * 3 / 4..];
    let tail_got = &got[got.len() - tail_ref.len()..];
    assert!(snr_db(tail_ref, tail_got) > 20.0);
}

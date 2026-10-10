mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::PcmData;

fn open(name: &str) -> SymphoniaDecoder { SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap() }

fn drain(d: &mut SymphoniaDecoder) -> Vec<f32> {
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        match b.data {
            PcmData::Float(v) => out.extend(v),
            PcmData::Int(v) => out.extend(v.iter().map(|&s| s as f32 / 2_147_483_648.0)),
        }
    }
    out
}

/// Seeking must reproduce the decoder's own full decode (isolates seek from codec loss).
fn check_seek(name: &str, frame: u64, min_snr: f64) {
    let full = drain(&mut open(name));
    let mut d = open(name);
    assert_eq!(d.seek(frame).unwrap(), frame);
    let got = drain(&mut d);
    let expected = &full[frame as usize * 2..];
    assert_eq!(got.len(), expected.len(), "{name}: seek({frame}) remaining length");
    let n = 4096 * 2;
    let snr = snr_db(&expected[..n], &got[..n]);
    assert!(snr >= min_snr, "{name}: seek({frame}) SNR {snr:.1} dB < {min_snr}");
}

#[test] fn aac_m4a_seek_zero_matches_full_decode() { check_seek("aac_lc_256.m4a", 0, 60.0); }
#[test] fn aac_m4a_seek_is_sample_accurate() { check_seek("aac_lc_256.m4a", 44_100, 40.0); }
#[test] fn mp3_seek_zero_matches_full_decode() { check_seek("mp3_320.mp3", 0, 60.0); }
#[test] fn mp3_seek_is_sample_accurate() { check_seek("mp3_320.mp3", 44_100, 40.0); }
#[test] fn vorbis_seek_zero_matches_full_decode() { check_seek("vorbis_q6.ogg", 0, 60.0); }
#[test] fn vorbis_seek_is_sample_accurate() { check_seek("vorbis_q6.ogg", 44_100, 40.0); }

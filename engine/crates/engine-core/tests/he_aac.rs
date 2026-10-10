#![cfg(feature = "fdk-aac")]
mod common;
use common::*;
use engine_core::decode::{aac_config::parse_asc, fdk_dec::FdkAacDecoder, symphonia_dec::probe_file, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn drain(d: &mut dyn TrackDecoder) -> Vec<f32> {
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() {
        let PcmData::Int(v) = b.data else { panic!() };
        out.extend(v.iter().map(|&s| s as f32 / 2_147_483_648.0));
    }
    out
}

fn mp4(name: &str) -> FdkAacDecoder {
    let p = probe_file(&fixture(name)).unwrap();
    let cfg = parse_asc(p.params.extra_data.as_deref().unwrap()).unwrap();
    FdkAacDecoder::new_mp4(p, cfg).unwrap()
}

#[test]
fn he_aac_m4a_decodes_at_full_sbr_rate() {
    let mut d = mp4("he_aac.m4a");
    assert_eq!(d.info().codec, protocol::Codec::HeAac);
    assert_eq!(d.info().spec.sample_rate, 48_000);
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 16 });
    assert_eq!(d.info().decoder_name, "fdk-aac");
    let out = drain(&mut d);
    assert!(out.len() / 2 >= 96_000);
    assert_eq!(dominant_freq(&out[9600..], 2, 0, 48_000), 1000.0);
    assert_eq!(dominant_freq(&out[9600..], 2, 1, 48_000), 440.0);
}

#[test]
fn he_aac_v2_m4a_outputs_stereo() {
    let mut d = mp4("he_aac_v2.m4a");
    assert_eq!(d.info().codec, protocol::Codec::HeAacV2);
    assert_eq!(d.info().spec.channels, 2);
    let out = drain(&mut d);
    assert!(rms(&channel(&out, 2, 0)) > 0.05);
}

#[test]
fn raw_adts_decodes_and_seeks() {
    let mut d = FdkAacDecoder::open_adts(&fixture("he_aac.aac")).unwrap();
    assert_eq!(d.info().spec.sample_rate, 48_000);
    let full = drain(&mut d);
    let mut d = FdkAacDecoder::open_adts(&fixture("he_aac.aac")).unwrap();
    d.seek(48_000).unwrap();
    let after = drain(&mut d);
    let diff = (full.len() as i64 / 2 - 48_000) - after.len() as i64 / 2;
    assert!(diff.abs() <= 2048, "seek landed {diff} frames off");
}

#[test]
fn he_aac_m4a_seek_matches_full_decode() {
    let full = drain(&mut mp4("he_aac.m4a"));
    let mut d = mp4("he_aac.m4a");
    assert_eq!(d.seek(48_000).unwrap(), 48_000);
    let after = drain(&mut d);
    let reference = &full[48_000 * 2..];
    assert_eq!(after.len(), reference.len(), "remaining length after seek differs from full decode");
    let n = 4096 * 2;
    let snr = snr_db(&reference[..n], &after[..n]);
    assert!(snr >= 30.0, "SNR over first 4096 frames after seek is {snr:.1} dB");
}

#[test]
fn raw_adts_v2_is_detected_and_decodes_every_frame() {
    let mut d = FdkAacDecoder::open_adts(&fixture("he_aac_v2.aac")).unwrap();
    assert_eq!(d.info().codec, protocol::Codec::HeAacV2);
    let total = d.info().total_frames.unwrap();
    // fdk buffers a few ADTS frames before it starts emitting; they must be drained at EOF.
    assert_eq!(drain(&mut d).len() as u64 / 2, total);
}

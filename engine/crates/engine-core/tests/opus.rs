mod common;
use common::*;
use engine_core::decode::{opus_dec::OpusDecoder, symphonia_dec::probe_file, TrackDecoder};
use engine_core::pcm::PcmData;

fn decode(name: &str) -> (OpusDecoder, Vec<f32>) {
    let mut d = OpusDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap();
    let mut out = vec![];
    while let Some(b) = d.next_block().unwrap() { let PcmData::Float(v) = b.data else { panic!() }; out.extend(v); }
    (d, out)
}

#[test]
fn decodes_at_48k_with_preskip_removed() {
    let (d, out) = decode("opus_160.opus");
    assert_eq!(d.info().spec.sample_rate, 48_000);
    assert_eq!(d.info().decoder_name, "libopus");
    let frames = out.len() / 2;
    assert!((frames as i64 - 96_000).abs() <= 48, "2 s at 48 kHz ±1 ms, got {frames}");
    assert_eq!(dominant_freq(&out, 2, 0, 48_000), 440.0);
    assert_eq!(dominant_freq(&out, 2, 1, 48_000), 1000.0);
    // Pre-skip removed: the signal starts immediately (no ~6.5 ms of leading silence).
    assert!(rms(&channel(&out[..960 * 2], 2, 0)) > 0.02);
}

#[test]
fn seek_lands_on_requested_frame() {
    let (_, full) = decode("opus_160.opus");
    let mut d = OpusDecoder::new(probe_file(&fixture("opus_160.opus")).unwrap()).unwrap();
    d.seek(48_000).unwrap();
    let PcmData::Float(v) = d.next_block().unwrap().unwrap().data else { panic!() };
    let n = v.len().min(1920);
    assert!(snr_db(&full[96_000..96_000 + n], &v[..n]) > 15.0);
}

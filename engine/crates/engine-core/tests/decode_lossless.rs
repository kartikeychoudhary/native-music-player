mod common;
use common::*;
use engine_core::decode::{symphonia_dec::{probe_file, SymphoniaDecoder}, TrackDecoder};
use engine_core::pcm::{PcmData, SampleKind};

fn open(name: &str) -> SymphoniaDecoder { SymphoniaDecoder::new(probe_file(&fixture(name)).unwrap()).unwrap() }

fn decode_all_int(dec: &mut dyn TrackDecoder) -> Vec<i32> {
    let mut out = vec![];
    while let Some(b) = dec.next_block().unwrap() {
        let PcmData::Int(v) = b.data else { panic!("expected integer samples") };
        out.extend(v);
    }
    out
}

#[test]
fn flac16_is_bit_exact() {
    let mut d = open("flac16_44k.flac");
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 16 });
    assert_eq!(d.info().spec.sample_rate, 44100);
    assert_eq!(decode_all_int(&mut d), read_s32le("pcm16_44k.s32le"));
}

#[test]
fn flac24_96k_is_bit_exact() {
    let mut d = open("flac24_96k.flac");
    assert_eq!(d.info().spec.kind, SampleKind::Int { bits: 24 });
    assert_eq!(decode_all_int(&mut d), read_s32le("pcm24_96k.s32le"));
}

#[test]
fn alac_16_and_24_are_bit_exact() {
    assert_eq!(decode_all_int(&mut open("alac16_44k.m4a")), read_s32le("pcm16_44k.s32le"));
    assert_eq!(decode_all_int(&mut open("alac24_96k.m4a")), read_s32le("pcm24_96k.s32le"));
}

#[test]
fn wav_and_aiff_are_bit_exact() {
    assert_eq!(decode_all_int(&mut open("pcm24_96k.wav")), read_s32le("pcm24_96k.s32le"));
    assert_eq!(decode_all_int(&mut open("aiff16_44k.aiff")), read_s32le("pcm16_44k.s32le"));
}

#[test]
fn flac_seek_is_sample_accurate() {
    let reference = read_s32le("pcm16_44k.s32le");
    let mut d = open("flac16_44k.flac");
    assert_eq!(d.seek(44_100).unwrap(), 44_100);
    let got = decode_all_int(&mut d);
    assert_eq!(got, reference[44_100 * 2..]);
    assert_eq!(d.seek(0).unwrap(), 0);
    assert_eq!(decode_all_int(&mut d), reference);
}

#[test]
fn reports_source_info() {
    let d = open("flac24_96k.flac");
    let i = d.info();
    assert_eq!(i.container, "flac");
    assert_eq!(i.codec, protocol::Codec::Flac);
    assert_eq!(i.decoder_name, "symphonia");
    assert_eq!(i.total_frames, Some(192_000));
    assert!(i.bitrate_kbps.unwrap() > 500);
}

mod common;
use common::*;
use engine_core::decode::open;
use protocol::{Codec, ErrorCode};

#[test]
fn routes_each_format_to_the_right_decoder() {
    let cases = [
        ("flac16_44k.flac", Codec::Flac, "symphonia"),
        ("mp3_320.mp3", Codec::Mp3, "symphonia"),
        ("aac_lc_256.m4a", Codec::AacLc, "symphonia"),
        ("alac16_44k.m4a", Codec::Alac, "symphonia"),
        ("vorbis_q6.ogg", Codec::Vorbis, "symphonia"),
        ("opus_160.opus", Codec::Opus, "libopus"),
        ("dsd64.dsf", Codec::Dsd, "dsd"),
        ("dsd64.dff", Codec::Dsd, "dsd"),
    ];
    for (name, codec, dec) in cases {
        let d = open(&fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((d.info().codec, d.info().decoder_name), (codec, dec), "{name}");
    }
}

#[cfg(feature = "fdk-aac")]
#[test]
fn he_aac_goes_to_fdk() {
    assert_eq!(open(&fixture("he_aac.m4a")).unwrap().info().decoder_name, "fdk-aac");
    assert_eq!(open(&fixture("he_aac_v2.m4a")).unwrap().info().codec, Codec::HeAacV2);
    assert_eq!(open(&fixture("he_aac.aac")).unwrap().info().decoder_name, "fdk-aac");
}

#[cfg(not(feature = "fdk-aac"))]
#[test]
fn he_aac_without_fdk_reports_codec_not_in_build() {
    let e = open(&fixture("he_aac.m4a")).err().unwrap();
    assert_eq!(e.code(), ErrorCode::CodecNotInBuild);
}

#[test]
fn error_codes() {
    assert_eq!(open(&fixture("empty.mp3")).err().unwrap().code(), ErrorCode::UnsupportedFormat);
    let missing = std::path::Path::new("definitely/not/here.flac");
    assert_eq!(open(missing).err().unwrap().code(), ErrorCode::FileMissing);
}

mod common;

use engine_core::decode::mp4_gapless::{read_mp4_gapless, Mp4Gapless};
use std::fs::File;

fn gapless(name: &str) -> Option<Mp4Gapless> {
    let mut f = File::open(common::fixture(name)).unwrap();
    read_mp4_gapless(&mut f).unwrap()
}

/// ffmpeg writes the elst duration in movie ticks (1 ms), so the exact valid length comes from mdhd.
#[test]
fn gap_a_exact_length_from_mdhd() {
    assert_eq!(gapless("gap_a.m4a"), Some(Mp4Gapless::EditList { media_time: 1024, duration: 110583, timescale: 44100 }));
}

#[test]
fn aac_lc_256_edit_list_duration() {
    match gapless("aac_lc_256.m4a") {
        Some(Mp4Gapless::EditList { duration, .. }) => assert_eq!(duration, 88200),
        other => panic!("expected an edit list, got {other:?}"),
    }
}

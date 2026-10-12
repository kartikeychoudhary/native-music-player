//! Crossfade maths (spec §3.6): equal-power curve, overlap length, and smart skip.

/// (outgoing gain, incoming gain) at position `t` in 0..=1 of the overlap; their squares sum to 1.
pub fn equal_power(t: f64) -> (f64, f64) {
    let a = t.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2;
    (a.cos(), a.sin())
}

/// The overlap in seconds: the setting, shrunk to half of the shorter track.
pub fn overlap_secs(fade: f64, a_secs: f64, b_secs: f64) -> f64 { fade.min(a_secs / 2.0).min(b_secs / 2.0).max(0.0) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlbumPos { pub album_id: Option<i64>, pub disc: Option<u32>, pub track: Option<u32> }

/// True when `b` directly follows `a` on the same album, so the pair must stay gapless.
pub fn same_album_in_order(a: AlbumPos, b: AlbumPos) -> bool {
    let (Some(album), Some(at), Some(bt)) = (a.album_id, a.track, b.track) else { return false };
    if b.album_id != Some(album) { return false; }
    let (ad, bd) = (a.disc.unwrap_or(1), b.disc.unwrap_or(1));
    (bd == ad && bt == at + 1) || (bd == ad + 1 && bt == 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_power_keeps_total_power_constant() {
        for i in 0..=10 {
            let (o, n) = equal_power(i as f64 / 10.0);
            assert!((o * o + n * n - 1.0).abs() < 1e-12);
        }
        assert_eq!(equal_power(0.0), (1.0, 0.0));
        let (o, n) = equal_power(1.0);
        assert!(o.abs() < 1e-12 && (n - 1.0).abs() < 1e-12);
    }

    #[test]
    fn overlap_shrinks_to_half_the_shorter_track() {
        assert_eq!(overlap_secs(4.0, 200.0, 180.0), 4.0);
        assert_eq!(overlap_secs(4.0, 0.5, 180.0), 0.25);
        assert_eq!(overlap_secs(4.0, 200.0, 3.0), 1.5);
        assert_eq!(overlap_secs(0.0, 200.0, 180.0), 0.0);
    }

    #[test]
    fn smart_skip_matches_consecutive_album_tracks_only() {
        let p = |album: Option<i64>, disc: Option<u32>, track: Option<u32>| AlbumPos { album_id: album, disc, track };
        assert!(same_album_in_order(p(Some(1), Some(1), Some(4)), p(Some(1), Some(1), Some(5))));
        assert!(same_album_in_order(p(Some(1), None, Some(4)), p(Some(1), Some(1), Some(5))), "missing disc = disc 1");
        assert!(same_album_in_order(p(Some(1), Some(1), Some(12)), p(Some(1), Some(2), Some(1))), "next disc starts at 1");
        assert!(!same_album_in_order(p(Some(1), Some(1), Some(4)), p(Some(1), Some(1), Some(6))), "skips a track");
        assert!(!same_album_in_order(p(Some(1), Some(1), Some(5)), p(Some(1), Some(1), Some(4))), "backwards");
        assert!(!same_album_in_order(p(Some(1), Some(1), Some(4)), p(Some(2), Some(1), Some(5))), "other album");
        assert!(!same_album_in_order(p(None, Some(1), Some(4)), p(None, Some(1), Some(5))), "not library albums");
        assert!(!same_album_in_order(p(Some(1), Some(1), None), p(Some(1), Some(1), Some(1))), "unknown order");
    }
}

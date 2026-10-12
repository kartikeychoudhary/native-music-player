//! ReplayGain (spec §3.6): track/album/off from tag values, with peak protection.
use protocol::{ReplayGainInfo, ReplayGainMode};

/// Highest allowed peak after gain, in dBFS.
pub const PEAK_CEILING_DB: f64 = -0.1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RgSource { Track, Album }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RgDecision {
    /// Gain to apply in dB (0 when off or without data).
    pub db: f64,
    /// Which tag values were used; `None` when off or the track has none.
    pub source: Option<RgSource>,
    /// The gain was lowered so the peak stays under `PEAK_CEILING_DB`.
    pub limited: bool,
}

impl RgDecision {
    pub const NONE: RgDecision = RgDecision { db: 0.0, source: None, limited: false };
    pub fn linear(&self) -> f64 { if self.db == 0.0 { 1.0 } else { 10f64.powf(self.db / 20.0) } }
}

pub fn decide(mode: ReplayGainMode, info: &ReplayGainInfo) -> RgDecision {
    let track = info.track_gain.map(|g| (g, info.track_peak, RgSource::Track));
    let album = info.album_gain.map(|g| (g, info.album_peak, RgSource::Album));
    let chosen = match mode {
        ReplayGainMode::Off => None,
        ReplayGainMode::Track => track.or(album),
        ReplayGainMode::Album => album.or(track),
    };
    let Some((gain, peak, source)) = chosen else { return RgDecision::NONE };
    // A missing peak counts as full scale: no boost then, only cuts.
    let peak = peak.filter(|p| *p > 0.0).unwrap_or(1.0) as f64;
    let max_db = PEAK_CEILING_DB - 20.0 * peak.log10();
    let gain = gain as f64;
    if gain > max_db { RgDecision { db: max_db, source: Some(source), limited: true } } else { RgDecision { db: gain, source: Some(source), limited: false } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ReplayGainInfo, ReplayGainMode::*};

    fn info(tg: Option<f32>, tp: Option<f32>, ag: Option<f32>, ap: Option<f32>) -> ReplayGainInfo {
        ReplayGainInfo { track_gain: tg, track_peak: tp, album_gain: ag, album_peak: ap }
    }

    #[test]
    fn picks_the_mode_then_falls_back() {
        let both = info(Some(-6.4), Some(0.5), Some(-8.0), Some(0.5));
        assert_eq!(decide(Track, &both), RgDecision { db: -6.4f32 as f64, source: Some(RgSource::Track), limited: false });
        assert_eq!(decide(Album, &both).source, Some(RgSource::Album));
        assert_eq!(decide(Album, &info(Some(-3.0), Some(0.5), None, None)).source, Some(RgSource::Track), "album falls back to track");
        assert_eq!(decide(Track, &info(None, None, Some(-2.0), Some(0.5))).source, Some(RgSource::Album), "track falls back to album");
        assert_eq!(decide(Off, &both), RgDecision { db: 0.0, source: None, limited: false });
        assert_eq!(decide(Track, &ReplayGainInfo::default()).source, None);
    }

    #[test]
    fn peak_protection_keeps_the_peak_under_minus_0_1_dbfs() {
        // Peak 0.5 (-6.02 dBFS): at most +5.92 dB.
        let d = decide(Track, &info(Some(9.0), Some(0.5), None, None));
        assert!(d.limited);
        assert!((d.db - (-0.1 + 6.0206)).abs() < 1e-3, "{}", d.db);
        // No peak counts as full scale, so no boost at all.
        let d = decide(Track, &info(Some(4.0), None, None, None));
        assert!(d.limited && (d.db - -0.1).abs() < 1e-9);
        // Cuts are never limited.
        assert!(!decide(Track, &info(Some(-12.0), None, None, None)).limited);
        assert!((decide(Track, &info(Some(-6.0206), Some(1.0), None, None)).linear() - 0.5).abs() < 1e-4);
    }
}

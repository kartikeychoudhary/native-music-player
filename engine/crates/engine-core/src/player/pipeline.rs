use protocol::{OutputMode, Transport};

use crate::decode::DsdOutput;
use crate::dsp::channels::ChannelMap;
use crate::output::format::{DeviceSample, OutputFormat};
use crate::output::DeviceCaps;
use crate::pcm::{PcmSpec, SampleKind};

/// Note for a bit-perfect plan that had no exclusive caps (the player may add the reason).
pub const EXCLUSIVE_UNAVAILABLE: &str = "Exclusive mode unavailable — using processed mode";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passthrough { None, Int, Float, Dop }

#[derive(Clone, Debug, PartialEq)]
pub struct PipelinePlan {
    pub format: OutputFormat,
    pub exclusive: bool,
    pub bit_perfect: bool,
    pub passthrough: Passthrough,
    pub resample_from: Option<u32>,
    pub channel_map: ChannelMap,
    pub dither: bool,
    pub dsd: Option<DsdOutput>,
    pub notes: Vec<String>,
}

pub struct PlanInput<'a> {
    pub source: &'a PcmSpec,
    pub dsd_rate: Option<u32>,
    pub mode: OutputMode,
    pub transport: Transport,
    pub shared_caps: &'a DeviceCaps,
    pub exclusive_caps: Option<&'a DeviceCaps>,
    /// Open stream (format, exclusive), for gapless continuation.
    pub current: Option<(OutputFormat, bool)>,
}

fn passthrough(fmt: OutputFormat, exclusive: bool, kind: Passthrough, dsd: Option<DsdOutput>) -> PipelinePlan {
    PipelinePlan { format: fmt, exclusive, bit_perfect: true, passthrough: kind, resample_from: None, channel_map: ChannelMap::Identity, dither: false, dsd, notes: vec![] }
}

/// Lossless passthrough onto `caps` at the source rate, if possible.
fn try_passthrough(i: &PlanInput, caps: &DeviceCaps, exclusive: bool) -> Option<PipelinePlan> {
    let s = i.source;
    if s.channels > caps.max_channels { return None; }
    if let Some(dsd_rate) = i.dsd_rate {
        let dop_rate = dsd_rate / 16;
        let sample = caps.best_int_for(24)?;
        return (exclusive && caps.supports_rate(dop_rate)).then(|| passthrough(OutputFormat { sample_rate: dop_rate, channels: s.channels, sample }, true, Passthrough::Dop, Some(DsdOutput::Dop)));
    }
    if !caps.supports_rate(s.sample_rate) { return None; }
    match s.kind {
        SampleKind::Int { bits } => caps.best_int_for(bits).map(|sample| passthrough(OutputFormat { sample_rate: s.sample_rate, channels: s.channels, sample }, exclusive, Passthrough::Int, None)),
        SampleKind::Float if caps.supports(DeviceSample::F32) => Some(passthrough(OutputFormat { sample_rate: s.sample_rate, channels: s.channels, sample: DeviceSample::F32 }, exclusive, Passthrough::Float, None)),
        _ => None,
    }
}

fn processed(i: &PlanInput, caps: &DeviceCaps, exclusive: bool, keep: Option<OutputFormat>) -> PipelinePlan {
    let s = i.source;
    let format = keep.unwrap_or_else(|| {
        let sample_rate = if caps.supports_rate(s.sample_rate) { s.sample_rate } else if caps.supports_rate(48_000) { 48_000 } else { caps.rates.first().copied().unwrap_or(48_000) };
        let sample = if caps.supports(DeviceSample::F32) { DeviceSample::F32 } else { caps.best_int_for(24).unwrap_or(DeviceSample::I16) };
        let channels = if s.channels == 1 && caps.max_channels >= 2 { 2 } else if s.channels <= caps.max_channels { s.channels } else { 2 };
        OutputFormat { sample_rate, channels, sample }
    });
    PipelinePlan {
        format,
        exclusive,
        bit_perfect: false,
        passthrough: Passthrough::None,
        resample_from: (s.sample_rate != format.sample_rate).then_some(s.sample_rate),
        channel_map: ChannelMap::plan(s.channels, format.channels),
        dither: format.sample.is_int(),
        dsd: i.dsd_rate.map(|_| DsdOutput::Pcm),
        notes: vec![],
    }
}

pub fn plan_output(i: &PlanInput) -> PipelinePlan {
    let s = i.source;
    if i.mode == OutputMode::BitPerfect {
        match i.exclusive_caps {
            Some(ex) => {
                if let Some(p) = try_passthrough(i, ex, true) { return p; }
                if s.kind == SampleKind::Float && i.dsd_rate.is_none() && ex.supports_rate(s.sample_rate) && s.channels <= ex.max_channels {
                    let mut p = processed(i, ex, true, None);
                    p.notes.push("Lossy source converted to integer".into());
                    return p;
                }
                let mut p = processed(i, i.shared_caps, false, None);
                p.notes.push(format!("Device can't play {} Hz / {} ch exclusively — using processed mode", s.sample_rate, s.channels));
                return p;
            }
            None if i.transport == Transport::Bluetooth => {
                if let Some(p) = try_passthrough(i, i.shared_caps, false) { return p; }
                let keep = i.current.filter(|&(_, ex)| !ex).map(|(f, _)| f);
                let mut p = processed(i, i.shared_caps, false, keep);
                p.notes.push(format!("Device can't play {} Hz / {} ch bit-perfect — using processed mode", s.sample_rate, s.channels));
                return p;
            }
            None => {
                let mut p = processed(i, i.shared_caps, false, None);
                p.notes.push(EXCLUSIVE_UNAVAILABLE.into());
                return p;
            }
        }
    }
    let keep = i.current.filter(|&(_, ex)| !ex).map(|(f, _)| f);
    processed(i, i.shared_caps, false, keep)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::format::DeviceSample::*;

    fn caps(exclusive: bool) -> DeviceCaps {
        DeviceCaps { rates: vec![44_100, 48_000, 88_200, 96_000, 176_400, 192_000], max_channels: 2, samples: vec![I16, I24, I32, F32], exclusive }
    }
    fn int(rate: u32, ch: u16, bits: u16) -> PcmSpec { PcmSpec { sample_rate: rate, channels: ch, kind: SampleKind::Int { bits } } }
    fn input<'a>(src: &'a PcmSpec, mode: OutputMode, shared: &'a DeviceCaps, ex: Option<&'a DeviceCaps>) -> PlanInput<'a> {
        PlanInput { source: src, dsd_rate: None, mode, transport: Transport::Usb, shared_caps: shared, exclusive_caps: ex, current: None }
    }

    #[test]
    fn bit_perfect_int_uses_narrowest_lossless_format() {
        let (sh, ex) = (caps(false), caps(true));
        let s = int(96_000, 2, 24);
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(p.bit_perfect && p.exclusive);
        assert_eq!(p.passthrough, Passthrough::Int);
        assert_eq!(p.format, OutputFormat { sample_rate: 96_000, channels: 2, sample: I24 });
        let s16 = int(44_100, 2, 16);
        assert_eq!(plan_output(&input(&s16, OutputMode::BitPerfect, &sh, Some(&ex))).format.sample, I16);
    }

    #[test]
    fn dsd_goes_dop_when_rate_supported() {
        let (sh, ex) = (caps(false), caps(true));
        let s = PcmSpec { sample_rate: 88_200, channels: 2, kind: SampleKind::Float };
        let mut i = input(&s, OutputMode::BitPerfect, &sh, Some(&ex));
        i.dsd_rate = Some(2_822_400);
        let p = plan_output(&i);
        assert_eq!(p.dsd, Some(DsdOutput::Dop));
        assert_eq!(p.passthrough, Passthrough::Dop);
        assert_eq!(p.format.sample_rate, 176_400);
        assert!(p.bit_perfect);
    }

    #[test]
    fn channel_mismatch_falls_back_to_processed() {
        let (sh, ex) = (caps(false), caps(true));
        let s = int(48_000, 6, 16);
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(!p.bit_perfect && !p.exclusive);
        assert_eq!(p.format.channels, 2);
        assert!(!p.channel_map.is_identity());
        assert!(p.notes.iter().any(|n| n.contains("6 ch")), "{:?}", p.notes);
    }

    #[test]
    fn lossy_on_integer_only_exclusive_device_is_not_bit_perfect() {
        let sh = caps(false);
        let ex = DeviceCaps { samples: vec![I16, I24], ..caps(true) };
        let s = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, Some(&ex)));
        assert!(!p.bit_perfect && p.exclusive && p.dither);
        assert!(p.notes.iter().any(|n| n == "Lossy source converted to integer"));
    }

    #[test]
    fn bluetooth_without_exclusive_is_bit_perfect_to_os() {
        let sh = caps(false);
        let s = int(48_000, 2, 16);
        let mut i = input(&s, OutputMode::BitPerfect, &sh, None);
        i.transport = Transport::Bluetooth;
        let p = plan_output(&i);
        assert!(p.bit_perfect && !p.exclusive);
        assert_eq!(p.passthrough, Passthrough::Int);
    }

    #[test]
    fn processed_keeps_current_shared_format_for_gapless() {
        let sh = caps(false);
        let s = int(96_000, 2, 24);
        let mut i = input(&s, OutputMode::Processed, &sh, None);
        i.current = Some((OutputFormat { sample_rate: 44_100, channels: 2, sample: F32 }, false));
        let p = plan_output(&i);
        assert_eq!(p.format.sample_rate, 44_100);
        assert_eq!(p.resample_from, Some(96_000));
        assert_eq!(p.passthrough, Passthrough::None);
    }

    #[test]
    fn bluetooth_bit_perfect_fallback_reports_reason() {
        let sh = DeviceCaps { rates: vec![48_000], ..caps(false) };
        let s = int(44_100, 2, 16);
        let mut i = input(&s, OutputMode::BitPerfect, &sh, None);
        i.transport = Transport::Bluetooth;
        let p = plan_output(&i);
        assert!(!p.bit_perfect && !p.exclusive);
        assert_eq!(p.passthrough, Passthrough::None);
        assert!(p.notes.iter().any(|n| n == "Device can't play 44100 Hz / 2 ch bit-perfect — using processed mode"), "{:?}", p.notes);
    }

    #[test]
    fn bit_perfect_without_exclusive_caps_on_non_bluetooth_is_processed_with_note() {
        let sh = caps(false);
        let s = int(48_000, 2, 16);
        let p = plan_output(&input(&s, OutputMode::BitPerfect, &sh, None));
        assert!(!p.bit_perfect && !p.exclusive);
        assert_eq!(p.passthrough, Passthrough::None);
        assert_eq!(p.notes, vec!["Exclusive mode unavailable — using processed mode".to_string()]);
    }

    #[test]
    fn processed_prefers_source_rate_then_48k() {
        let sh = DeviceCaps { rates: vec![48_000], ..caps(false) };
        let s = int(44_100, 1, 16);
        let p = plan_output(&input(&s, OutputMode::Processed, &sh, None));
        assert_eq!(p.format, OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 });
        assert_eq!(p.channel_map.describe(), "mono → stereo");
    }
}

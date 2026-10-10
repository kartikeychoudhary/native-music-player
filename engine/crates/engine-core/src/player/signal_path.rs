use protocol::*;

use crate::decode::{source_summary, SourceInfo};
use crate::output::StreamInfo;
use crate::player::pipeline::{Passthrough, PipelinePlan};

pub fn dsd_label(rate: u32) -> String { format!("DSD{}", rate / 44_100) }

fn khz(r: u32) -> String {
    let k = r as f64 / 1000.0;
    if (k - k.round()).abs() < 1e-9 { format!("{}", k as u32) } else { format!("{k:.1}") }
}

pub fn build_signal_path(src: &SourceInfo, plan: &PipelinePlan, stream: &StreamInfo, volume: f32, bluetooth: Option<BluetoothInfo>, underruns: u64) -> SignalPath {
    let mut stages = Vec::new();
    if let Some(rate) = src.dsd_rate {
        let detail = match plan.passthrough {
            Passthrough::Dop => format!("{} as DoP @ {} kHz", dsd_label(rate), khz(plan.format.sample_rate)),
            _ => format!("{} → PCM {} kHz (−6 dB)", dsd_label(rate), khz(src.spec.sample_rate)),
        };
        stages.push(Stage { kind: StageKind::Dsd, active: true, detail });
    }
    stages.push(Stage { kind: StageKind::Channels, active: !plan.channel_map.is_identity(), detail: plan.channel_map.describe() });
    stages.push(Stage {
        kind: StageKind::Resampler,
        active: plan.resample_from.is_some(),
        detail: plan.resample_from.map_or("rate matched".into(), |from| format!("{} → {} kHz (sinc)", khz(from), khz(plan.format.sample_rate))),
    });
    let processed = plan.passthrough == Passthrough::None;
    stages.push(Stage {
        kind: StageKind::Volume,
        active: processed && volume < 1.0,
        detail: if processed { format!("{}%", (volume * 100.0).round() as u32) } else { "bypassed (bit-perfect)".into() },
    });
    stages.push(Stage {
        kind: StageKind::Dither,
        active: plan.dither,
        detail: if plan.dither { format!("TPDF → {}-bit", plan.format.sample.bits()) } else { "off".into() },
    });

    let is_bt = stream.transport == Transport::Bluetooth;
    let scope = plan.bit_perfect.then_some(if plan.exclusive && !is_bt { BitPerfectScope::Device } else { BitPerfectScope::Os });
    SignalPath {
        source: source_summary(src),
        decoder: DecoderSummary { name: src.decoder_name.into(), gapless: src.gapless.as_ref().map(|g| GaplessSummary { delay: g.delay, padding: g.padding }) },
        stages,
        output: OutputSummary {
            device_id: stream.device_id.clone(), name: stream.device_name.clone(), transport: stream.transport,
            mode: stream.share, sample_rate: stream.format.sample_rate, format: stream.format.sample.wire_name().into(),
        },
        bluetooth: is_bt.then(|| bluetooth.unwrap_or_default()),
        bit_perfect: plan.bit_perfect,
        bit_perfect_scope: scope,
        underruns,
        notes: plan.notes.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::SourceInfo;
    use crate::dsp::channels::ChannelMap;
    use crate::output::format::{DeviceSample, OutputFormat};
    use crate::pcm::{PcmSpec, SampleKind};
    use crate::player::pipeline::Passthrough;

    fn src() -> SourceInfo {
        SourceInfo { container: "flac".into(), codec: Codec::Flac, bitrate_kbps: Some(2400), spec: PcmSpec { sample_rate: 96_000, channels: 2, kind: SampleKind::Int { bits: 24 } }, total_frames: Some(1), gapless: None, decoder_name: "symphonia", dsd_rate: None }
    }
    fn plan(bp: bool, ex: bool, resample: Option<u32>) -> PipelinePlan {
        PipelinePlan { format: OutputFormat { sample_rate: 96_000, channels: 2, sample: DeviceSample::I24 }, exclusive: ex, bit_perfect: bp, passthrough: if bp { Passthrough::Int } else { Passthrough::None }, resample_from: resample, channel_map: ChannelMap::Identity, dither: !bp, dsd: None, notes: vec!["n".into()] }
    }
    fn stream(t: Transport, share: ShareMode) -> StreamInfo {
        StreamInfo { device_id: "d".into(), device_name: "DAC".into(), transport: t, share, format: OutputFormat { sample_rate: 96_000, channels: 2, sample: DeviceSample::I24 } }
    }

    #[test]
    fn wired_exclusive_bit_perfect_is_device_scope() {
        let sp = build_signal_path(&src(), &plan(true, true, None), &stream(Transport::Usb, ShareMode::Exclusive), 0.5, None, 0);
        assert!(sp.bit_perfect);
        assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Device));
        assert!(sp.bluetooth.is_none());
        let vol = sp.stages.iter().find(|s| s.kind == StageKind::Volume).unwrap();
        assert!(!vol.active);
        assert_eq!(vol.detail, "bypassed (bit-perfect)");
        assert_eq!(sp.output.format, "s24");
        assert_eq!(sp.notes, vec!["n".to_string()]);
    }

    #[test]
    fn bluetooth_is_os_scope_and_always_has_bluetooth_block() {
        let sp = build_signal_path(&src(), &plan(true, true, None), &stream(Transport::Bluetooth, ShareMode::Exclusive), 1.0, None, 3);
        assert_eq!(sp.bit_perfect_scope, Some(BitPerfectScope::Os));
        assert_eq!(sp.bluetooth, Some(BluetoothInfo::default()));
        assert_eq!(sp.underruns, 3);
    }

    #[test]
    fn processed_shows_resampler_and_volume() {
        let sp = build_signal_path(&src(), &plan(false, false, Some(44_100)), &stream(Transport::Builtin, ShareMode::Shared), 0.82, None, 0);
        assert!(!sp.bit_perfect && sp.bit_perfect_scope.is_none());
        let rs = sp.stages.iter().find(|s| s.kind == StageKind::Resampler).unwrap();
        assert!(rs.active);
        assert_eq!(rs.detail, "44.1 → 96 kHz (sinc)");
        assert_eq!(sp.stages.iter().find(|s| s.kind == StageKind::Volume).unwrap().detail, "82%");
    }

    #[test]
    fn dsd_labels() { assert_eq!(dsd_label(2_822_400), "DSD64"); assert_eq!(dsd_label(11_289_600), "DSD256"); }
}

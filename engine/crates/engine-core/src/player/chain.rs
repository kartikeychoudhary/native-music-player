use crate::dsp::convert::{write_f32_passthrough, write_f64, write_int_passthrough, Ditherer};
use crate::dsp::resample::StreamResampler;
use crate::pcm::{PcmBlock, PcmData, PcmSpec};
use crate::player::pipeline::{Passthrough, PipelinePlan};

/// Turns decoded blocks into device-format bytes according to a `PipelinePlan`.
/// Volume isn't applied here: the ring applies gain in the callback so changes are instant.
pub struct ProcessChain {
    plan: PipelinePlan,
    src: PcmSpec,
    resampler: Option<StreamResampler>,
    dither: Ditherer,
    f: Vec<f64>,
    mapped: Vec<f64>,
    resampled: Vec<f64>,
    gain: f64,
}

impl ProcessChain {
    pub fn new(plan: PipelinePlan, src: PcmSpec, reuse: Option<StreamResampler>) -> Self {
        let resampler = plan.resample_from.map(|from| {
            let want = (from, plan.format.sample_rate, plan.format.channels as usize);
            match reuse {
                Some(r) if r.config() == want => r,
                _ => StreamResampler::new(want.0, want.1, want.2).expect("resampler config from plan_output must be valid"),
            }
        });
        Self { plan, src, resampler, dither: Ditherer::new(0x5EED), f: vec![], mapped: vec![], resampled: vec![], gain: 1.0 }
    }

    pub fn plan(&self) -> &PipelinePlan { &self.plan }
    pub fn take_resampler(&mut self) -> Option<StreamResampler> { self.resampler.take() }

    /// Linear gain (ReplayGain) applied to processed samples after resampling; passthrough ignores it.
    pub fn set_gain(&mut self, linear: f64) { self.gain = linear; }

    pub fn process(&mut self, block: &PcmBlock, out: &mut Vec<u8>) {
        debug_assert_eq!(block.spec.channels, self.src.channels);
        match (self.plan.passthrough, &block.data) {
            (Passthrough::Int | Passthrough::Dop, PcmData::Int(v)) => return write_int_passthrough(v, self.plan.format.sample, out),
            (Passthrough::Float, PcmData::Float(v)) => return write_f32_passthrough(v, out),
            _ => {}
        }
        self.f.clear();
        block.to_f64(&mut self.f);
        self.mapped.clear();
        self.plan.channel_map.apply(&self.f, &mut self.mapped);
        let samples: &mut Vec<f64> = match self.resampler.as_mut() {
            Some(r) => { self.resampled.clear(); r.process(&self.mapped, &mut self.resampled); &mut self.resampled }
            None => &mut self.mapped,
        };
        if self.gain != 1.0 { for s in samples.iter_mut() { *s *= self.gain; } }
        let dither = self.plan.dither.then_some(&mut self.dither);
        write_f64(samples, self.plan.format.sample, dither, out);
    }

    pub fn flush(&mut self, out: &mut Vec<u8>) {
        if let Some(r) = self.resampler.as_mut() {
            self.resampled.clear();
            r.flush(&mut self.resampled);
            if self.gain != 1.0 { for s in self.resampled.iter_mut() { *s *= self.gain; } }
            let dither = self.plan.dither.then_some(&mut self.dither);
            write_f64(&self.resampled, self.plan.format.sample, dither, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::channels::ChannelMap;
    use crate::output::format::DeviceSample::*;
    use crate::output::format::OutputFormat;
    use crate::pcm::*;
    use crate::player::pipeline::Passthrough;

    fn plan(pass: Passthrough, fmt: OutputFormat, resample_from: Option<u32>, map: ChannelMap) -> PipelinePlan {
        PipelinePlan { format: fmt, exclusive: false, bit_perfect: pass != Passthrough::None, passthrough: pass, resample_from, channel_map: map, dither: false, dsd: None, notes: vec![] }
    }

    #[test]
    fn int_passthrough_writes_exact_bytes() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Int { bits: 16 } };
        let mut c = ProcessChain::new(plan(Passthrough::Int, OutputFormat { sample_rate: 44_100, channels: 2, sample: I32 }, None, ChannelMap::Identity), src, None);
        let mut out = vec![];
        c.process(&PcmBlock { spec: src, data: PcmData::Int(vec![1 << 16, -(1 << 16)]) }, &mut out);
        assert_eq!(out, [(1i32 << 16).to_le_bytes(), (-(1i32 << 16)).to_le_bytes()].concat());
    }

    #[test]
    fn processed_maps_channels_and_resamples() {
        let src = PcmSpec { sample_rate: 44_100, channels: 1, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let mut c = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::plan(1, 2)), src, None);
        let mut out = vec![];
        c.process(&PcmBlock { spec: src, data: PcmData::Float(vec![0.25; 44_100]) }, &mut out);
        c.flush(&mut out);
        let frames = out.len() / 8;
        assert!((frames as i64 - 48_000).abs() <= 2);
    }

    fn sine_stereo(start: usize, frames: usize) -> Vec<f32> {
        (start..start + frames).flat_map(|i| { let s = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin() as f32 * 0.5; [s, s] }).collect()
    }

    #[test]
    fn resampler_handoff_preserves_buffered_latency_across_tracks() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let mut a = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::Identity), src, None);
        let mut out_a = vec![];
        a.process(&PcmBlock { spec: src, data: PcmData::Float(sine_stereo(0, 22_050)) }, &mut out_a);
        let r = a.take_resampler().unwrap();
        assert_eq!(r.config(), (44_100, 48_000, 2));
        let mut b = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::Identity), src, Some(r));
        let mut out_b = vec![];
        b.process(&PcmBlock { spec: src, data: PcmData::Float(sine_stereo(22_050, 22_050)) }, &mut out_b);
        b.flush(&mut out_b);
        let frames = (out_a.len() + out_b.len()) / fmt.bytes_per_frame();
        assert_eq!(frames, 48_000);
    }

    #[test]
    fn mismatched_reused_resampler_is_rebuilt_from_the_plan() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let wrong = StreamResampler::new(44_100, 96_000, 2).unwrap();
        let mut c = ProcessChain::new(plan(Passthrough::None, fmt, Some(44_100), ChannelMap::Identity), src, Some(wrong));
        let mut out = vec![];
        c.process(&PcmBlock { spec: src, data: PcmData::Float(sine_stereo(0, 44_100)) }, &mut out);
        c.flush(&mut out);
        assert_eq!(out.len() / fmt.bytes_per_frame(), 48_000);
        assert_eq!(c.take_resampler().unwrap().config(), (44_100, 48_000, 2));
    }

    #[test]
    #[should_panic(expected = "resampler config from plan_output must be valid")]
    fn invalid_resampler_config_fails_loudly() {
        let src = PcmSpec { sample_rate: 44_100, channels: 2, kind: SampleKind::Float };
        let fmt = OutputFormat { sample_rate: 48_000, channels: 2, sample: F32 };
        let _ = ProcessChain::new(plan(Passthrough::None, fmt, Some(0), ChannelMap::Identity), src, None);
    }
}

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};

/// Streaming high-quality sinc resampler over interleaved f64.
/// Removes the resampler's own delay at the start and pads/truncates at `flush` so that
/// output length = round(input length × ratio).
pub struct StreamResampler {
    r: Async<f64>,
    from: u32,
    to: u32,
    ch: usize,
    pending: Vec<f64>,
    out_buf: Vec<f64>,
    to_drop: usize,
    in_total: u64,
    out_total: u64,
}

impl StreamResampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Result<Self, String> {
        let params = SincInterpolationParameters::new(128, WindowFunction::Blackman2)
            .oversampling_factor(256)
            .interpolation(SincInterpolationType::Cubic);
        let r = Async::<f64>::new_sinc(to as f64 / from as f64, 1.0, &params, 1024, channels, FixedAsync::Input).map_err(|e| e.to_string())?;
        let to_drop = r.output_delay();
        let out_buf = vec![0.0; r.output_frames_max() * channels];
        Ok(Self { r, from, to, ch: channels, pending: Vec::new(), out_buf, to_drop, in_total: 0, out_total: 0 })
    }

    pub fn config(&self) -> (u32, u32, usize) { (self.from, self.to, self.ch) }

    fn run(&mut self, frames: usize, partial: Option<usize>, out: &mut Vec<f64>) -> usize {
        let ch = self.ch;
        let (n_in, n_out) = {
            let inp = InterleavedSlice::new(&self.pending[..frames * ch], ch, frames).expect("input adapter");
            let cap = self.out_buf.len() / ch;
            let mut o = InterleavedSlice::new_mut(&mut self.out_buf, ch, cap).expect("output adapter");
            let mut idx = Indexing::new();
            idx.partial_len = partial;
            self.r.process_into_buffer(&inp, &mut o, Some(&idx)).expect("resample")
        };
        let drop = self.to_drop.min(n_out);
        self.to_drop -= drop;
        out.extend_from_slice(&self.out_buf[drop * ch..n_out * ch]);
        self.out_total += (n_out - drop) as u64;
        n_in.min(frames)
    }

    pub fn process(&mut self, input: &[f64], out: &mut Vec<f64>) {
        self.pending.extend_from_slice(input);
        self.in_total += (input.len() / self.ch) as u64;
        loop {
            let need = self.r.input_frames_next();
            if self.pending.len() / self.ch < need { break; }
            let used = self.run(need, None, out);
            self.pending.drain(..used * self.ch);
        }
    }

    pub fn flush(&mut self, out: &mut Vec<f64>) {
        let expected = (self.in_total as f64 * self.to as f64 / self.from as f64).round() as u64;
        let start_len = out.len();
        let start_total = self.out_total;
        loop {
            let have = self.pending.len() / self.ch;
            let need = self.r.input_frames_next();
            if self.pending.len() < need * self.ch { self.pending.resize(need * self.ch, 0.0); }
            self.run(need, Some(have.min(need)), out);
            self.pending.drain(..need * self.ch);
            if self.out_total >= expected { break; }
        }
        let keep = (expected - start_total) as usize * self.ch;
        out.truncate(start_len + keep);
        self.out_total = expected;
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsamples_44k1_to_48k_keeping_length_and_pitch() {
        let mut r = StreamResampler::new(44_100, 48_000, 2).unwrap();
        let input: Vec<f64> = (0..44_100).flat_map(|i| { let s = (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / 44_100.0).sin() * 0.5; [s, s] }).collect();
        let mut out = vec![];
        for chunk in input.chunks(2 * 777) { r.process(chunk, &mut out); }
        r.flush(&mut out);
        let frames = out.len() / 2;
        assert!((frames as i64 - 48_000).abs() <= 2, "{frames}");
        // Zero crossings over the middle 0.5 s ≈ 2 per cycle → 1000 Hz.
        let left: Vec<f64> = out.iter().step_by(2).copied().collect();
        let mid = &left[12_000..36_000];
        let crossings = mid.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((crossings as i64 - 1000).abs() <= 2, "{crossings}");
    }
}

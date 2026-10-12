//! The outgoing track's tail during a crossfade. Only its last `keep` frames can fall inside the
//! overlap, so anything older is handed back by `push` to be written unmixed at once. This bounds
//! memory to the overlap however long the track really is (M3 final review F4).

/// Interleaved f64 frames, at most `keep` of them (plus nothing: excess is returned by `push`).
pub struct TailBuffer { ch: usize, keep: usize, buf: Vec<f64> }

impl TailBuffer {
    pub fn new(channels: usize, keep_frames: usize) -> Self { Self { ch: channels.max(1), keep: keep_frames, buf: Vec::new() } }

    /// Appends rendered samples and returns the frames that can no longer be part of the overlap.
    pub fn push(&mut self, samples: &[f64]) -> Vec<f64> {
        self.buf.extend_from_slice(samples);
        let frames = self.buf.len() / self.ch;
        if frames <= self.keep { return Vec::new(); }
        self.buf.drain(..(frames - self.keep) * self.ch).collect()
    }

    pub fn frames(&self) -> usize { self.buf.len() / self.ch }
    pub fn samples(&self) -> &[f64] { &self.buf }
}

#[cfg(test)]
mod tests {
    use super::TailBuffer;

    fn ramp(from: usize, frames: usize, ch: usize) -> Vec<f64> {
        (from * ch..(from + frames) * ch).map(|i| i as f64).collect()
    }

    #[test]
    fn never_holds_more_than_the_overlap_and_loses_nothing() {
        let (ch, keep, block) = (2, 100, 64);
        let mut t = TailBuffer::new(ch, keep);
        let mut out = Vec::new();
        for b in 0..50 {
            out.extend(t.push(&ramp(b * block, block, ch)));
            assert!(t.frames() <= keep, "block {b}: {} frames held", t.frames());
        }
        out.extend_from_slice(t.samples());
        assert_eq!(out, ramp(0, 50 * block, ch), "every sample, in order, exactly once");
        assert_eq!(t.frames(), keep);
    }

    #[test]
    fn a_tail_shorter_than_the_overlap_is_kept_whole() {
        let mut t = TailBuffer::new(2, 100);
        assert!(t.push(&ramp(0, 30, 2)).is_empty());
        assert_eq!(t.frames(), 30);
        assert_eq!(t.samples(), &ramp(0, 30, 2)[..]);
    }

    #[test]
    fn returns_whole_frames_only() {
        let mut t = TailBuffer::new(3, 4);
        let early = t.push(&ramp(0, 10, 3));
        assert_eq!(early.len() % 3, 0);
        assert_eq!(early, ramp(0, 6, 3));
    }
}

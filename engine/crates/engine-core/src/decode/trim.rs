use crate::pcm::PcmBlock;

/// Track-level gapless trimming for containers whose demuxer doesn't trim (MP4).
/// Coordinates are output frames from the start of the decoded stream.
pub struct Trimmer {
    skip: u64,
    remaining: Option<u64>,
    valid: Option<u64>,
}

impl Trimmer {
    pub fn new(delay: u64, valid: Option<u64>) -> Self {
        Self { skip: delay, remaining: valid, valid }
    }

    pub fn apply(&mut self, block: &mut PcmBlock) {
        let frames = block.frames() as u64;
        let front = self.skip.min(frames);
        self.skip -= front;
        let mut keep = frames - front;
        if let Some(rem) = self.remaining.as_mut() {
            keep = keep.min(*rem);
            *rem -= keep;
        }
        let back = frames - front - keep;
        block.trim(front as usize, back as usize);
    }

    /// `target` and `landed` are positions in trimmed (user-visible) frames. The decoder has been
    /// repositioned so the next decoded frame is `landed + delay` in raw coordinates.
    /// Frames skipped to reach `target` count against the valid length, so `remaining` is
    /// measured from `target`, not `landed`.
    pub fn seek_to(&mut self, target: u64, landed: u64) {
        self.skip = target.saturating_sub(landed);
        self.remaining = self.valid.map(|v| v.saturating_sub(target));
    }

    /// Like `seek_to`, but the caller supplies the number of raw frames to discard from the
    /// repositioned decoder (pre-roll plus any priming it landed inside); `target` is the
    /// user-visible frame the stream resumes at, so `remaining` is measured from it.
    pub fn seek_raw(&mut self, skip: u64, target: u64) {
        self.skip = skip;
        self.remaining = self.valid.map(|v| v.saturating_sub(target));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::*;

    fn block(frames: usize) -> PcmBlock {
        PcmBlock {
            spec: PcmSpec { sample_rate: 44100, channels: 2, kind: SampleKind::Int { bits: 16 } },
            data: PcmData::Int((0..frames as i32 * 2).collect()),
        }
    }

    #[test]
    fn seek_raw_skips_given_frames_and_measures_remaining_from_target() {
        let mut t = Trimmer::new(1500, Some(2000));
        t.seek_raw(1100, 500);
        let mut a = block(1024);
        t.apply(&mut a);
        assert_eq!(a.frames(), 0);
        let mut b = block(1024);
        t.apply(&mut b);
        assert_eq!(b.frames(), 1024 - 76); // 2048 - 1100 skipped = 948 left
        let mut c = block(1024);
        t.apply(&mut c);
        assert_eq!(c.frames(), 1500 - 948); // remaining 2000 - 500 = 1500 in total
    }

    #[test]
    fn drops_delay_across_blocks_and_stops_at_valid_length() {
        let mut t = Trimmer::new(1500, Some(2000));
        let mut a = block(1024);
        t.apply(&mut a);
        assert_eq!(a.frames(), 0);
        let mut b = block(1024);
        t.apply(&mut b);
        assert_eq!(b.frames(), 548); // 2048 - 1500
        let PcmData::Int(v) = &b.data else { panic!() };
        assert_eq!(v[0], 476 * 2); // first kept frame is frame 476 of block b
        let mut c = block(4096);
        t.apply(&mut c);
        assert_eq!(c.frames(), 1452); // 2000 - 548
        let mut d = block(10);
        t.apply(&mut d);
        assert!(d.is_empty());
    }

    #[test]
    fn seek_resets_skip_and_remaining() {
        let mut t = Trimmer::new(100, Some(1000));
        t.seek_to(500, 480); // demuxer landed 20 frames early (in trimmed coordinates)
        let mut a = block(64);
        t.apply(&mut a);
        assert_eq!(a.frames(), 44);
        let mut b = block(1000);
        t.apply(&mut b);
        assert_eq!(b.frames(), 1000 - 500 - 44); // total emitted after seek = valid - target = 500
    }

    #[test]
    fn seek_landing_exactly_on_target_has_no_skip() {
        let mut t = Trimmer::new(100, Some(1000));
        t.seek_to(500, 500);
        let mut a = block(1000);
        t.apply(&mut a);
        assert_eq!(a.frames(), 500); // remaining = valid - target
        let mut b = block(1000);
        t.apply(&mut b);
        assert!(b.is_empty());
    }

    #[test]
    fn seek_past_valid_length_emits_nothing() {
        let mut t = Trimmer::new(100, Some(1000));
        t.seek_to(1200, 480); // target beyond the end of the valid region
        let mut a = block(64);
        t.apply(&mut a);
        assert!(a.is_empty());
        let mut b = block(1000);
        t.apply(&mut b);
        assert!(b.is_empty());
    }

    #[test]
    fn pcm_trim_and_to_f64() {
        let mut b = block(4);
        b.trim(1, 1);
        assert_eq!(b.frames(), 2);
        let mut f = vec![];
        let fb = PcmBlock { spec: PcmSpec { sample_rate: 1, channels: 1, kind: SampleKind::Int { bits: 16 } }, data: PcmData::Int(vec![i32::MIN, 1 << 30]) };
        fb.to_f64(&mut f);
        assert_eq!(f, vec![-1.0, 0.5]);
    }
}

//! Channel layout adaptation. Input layouts follow WAV/FLAC order.
const H: f64 = std::f64::consts::FRAC_1_SQRT_2;

#[derive(Clone, Debug, PartialEq)]
pub enum ChannelMap { Identity, Matrix { from: u16, to: u16, m: Vec<f64> } }

#[derive(Clone, Copy)]
enum Pos { FL, FR, FC, Lfe, BL, BR, BC, SL, SR }

/// (left, right) weight for input channel `i` of an `n`-channel source.
fn lr_weight(n: u16, i: u16) -> (f64, f64) {
    use Pos::*;
    let layout: &[Pos] = match n {
        3 => &[FL, FR, FC],
        4 => &[FL, FR, BL, BR],
        5 => &[FL, FR, FC, BL, BR],
        6 => &[FL, FR, FC, Lfe, BL, BR],
        7 => &[FL, FR, FC, Lfe, BC, SL, SR],
        _ => &[FL, FR, FC, Lfe, BL, BR, SL, SR],
    };
    match layout.get(i as usize).copied().unwrap_or(Lfe) {
        FL => (1.0, 0.0), FR => (0.0, 1.0), FC => (H, H), Lfe => (0.0, 0.0),
        BL | SL => (H, 0.0), BR | SR => (0.0, H), BC => (0.5, 0.5),
    }
}

impl ChannelMap {
    pub fn plan(from: u16, to: u16) -> Self {
        if from == to { return Self::Identity; }
        let (f, t) = (from as usize, to as usize);
        let mut m = vec![0.0; f * t];
        if from == 1 {
            for o in 0..t { m[o * f] = 1.0; }
        } else if to == 2 {
            for i in 0..from { let (l, r) = lr_weight(from, i); m[i as usize] = l; m[f + i as usize] = r; }
            for o in 0..2 {
                let sum: f64 = m[o * f..(o + 1) * f].iter().sum();
                if sum > 0.0 { m[o * f..(o + 1) * f].iter_mut().for_each(|v| *v /= sum); }
            }
        } else if to == 1 {
            m.iter_mut().for_each(|v| *v = 1.0 / f as f64);
        } else {
            for o in 0..t.min(f) { m[o * f + o] = 1.0; }
        }
        Self::Matrix { from, to, m }
    }

    pub fn is_identity(&self) -> bool { matches!(self, Self::Identity) }

    pub fn apply(&self, input: &[f64], out: &mut Vec<f64>) {
        match self {
            Self::Identity => out.extend_from_slice(input),
            Self::Matrix { from, to, m } => {
                let (f, t) = (*from as usize, *to as usize);
                out.reserve(input.len() / f * t);
                for frame in input.chunks_exact(f) {
                    for o in 0..t {
                        out.push(frame.iter().zip(&m[o * f..(o + 1) * f]).map(|(x, w)| x * w).sum());
                    }
                }
            }
        }
    }

    pub fn describe(&self) -> String {
        let name = |n: u16| match n { 1 => "mono".to_string(), 2 => "stereo".into(), 6 => "5.1".into(), 8 => "7.1".into(), n => format!("{n} ch") };
        match self {
            Self::Identity => "unchanged".into(),
            Self::Matrix { from: 1, to, .. } => format!("mono → {}", name(*to)),
            Self::Matrix { from, to, .. } if from > to => format!("{} → {} downmix", name(*from), name(*to)),
            Self::Matrix { from, to, .. } => format!("{} → {}", name(*from), name(*to)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_passes_through() {
        let m = ChannelMap::plan(2, 2);
        assert!(m.is_identity());
        let mut out = vec![];
        m.apply(&[0.1, 0.2], &mut out);
        assert_eq!(out, vec![0.1, 0.2]);
    }

    #[test]
    fn mono_duplicates_to_stereo() {
        let m = ChannelMap::plan(1, 2);
        let mut out = vec![];
        m.apply(&[0.5, -0.5], &mut out);
        assert_eq!(out, vec![0.5, 0.5, -0.5, -0.5]);
        assert_eq!(m.describe(), "mono → stereo");
    }

    #[test]
    fn downmix_51_never_clips() {
        let m = ChannelMap::plan(6, 2);
        let mut out = vec![];
        m.apply(&[1.0; 6], &mut out);
        assert!(out.iter().all(|&v| v <= 1.0 + 1e-12 && v > 0.99), "{out:?}");
        out.clear();
        m.apply(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], &mut out); // FL only → left only
        assert!(out[0] > 0.3 && out[1] == 0.0);
        assert_eq!(m.describe(), "5.1 → stereo downmix");
    }
}

//! Album-art palette: dominant, vibrant and muted colours (spec §3.9, used by §5.5 dynamic colour).
use protocol::Palette;

const K: usize = 5;
const ITERATIONS: usize = 10;

fn luma(p: [f32; 3]) -> f32 { 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2] }

fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 { (0..3).map(|i| (a[i] - b[i]).powi(2)).sum() }

fn nearest(centers: &[[f32; 3]], p: [f32; 3]) -> usize {
    (0..centers.len()).min_by(|&a, &b| dist2(centers[a], p).total_cmp(&dist2(centers[b], p))).unwrap_or(0)
}

/// HSL saturation and lightness, both in 0..=1.
fn sat_light(p: [f32; 3]) -> (f32, f32) {
    let (r, g, b) = (p[0] / 255.0, p[1] / 255.0, p[2] / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 { 0.0 } else { d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-6) };
    (s.min(1.0), l)
}

fn vibrancy(p: [f32; 3]) -> f32 { let (s, l) = sat_light(p); s * (1.0 - (l - 0.5).abs() * 2.0) }

/// Lower is more "muted": low saturation and a slightly dark lightness (it blends into `--bg`).
fn muted_score(p: [f32; 3]) -> f32 { let (s, l) = sat_light(p); s + (l - 0.4).abs() }

fn hex(p: [f32; 3]) -> String {
    let c = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", c(p[0]), c(p[1]), c(p[2]))
}

/// Palette of packed RGB bytes via k-means (k = 5, 10 iterations, seeded deterministically from
/// pixels spread evenly through luma order). Clusters under 5 % of pixels can't be vibrant/muted.
pub fn extract(rgb: &[u8]) -> Palette {
    let px: Vec<[f32; 3]> = rgb.as_chunks::<3>().0.iter().map(|c| [c[0] as f32, c[1] as f32, c[2] as f32]).collect();
    if px.is_empty() {
        return Palette { dominant: "#000000".into(), vibrant: "#000000".into(), muted: "#000000".into() };
    }
    let k = K.min(px.len());
    let mut order: Vec<usize> = (0..px.len()).collect();
    order.sort_by(|&a, &b| luma(px[a]).total_cmp(&luma(px[b])));
    let mut centers: Vec<[f32; 3]> = (0..k).map(|i| px[order[(2 * i + 1) * order.len() / (2 * k)]]).collect();
    let mut counts = vec![0usize; k];
    for _ in 0..ITERATIONS {
        let mut sums = vec![[0f32; 3]; k];
        counts.iter_mut().for_each(|c| *c = 0);
        for &p in &px {
            let i = nearest(&centers, p);
            counts[i] += 1;
            for c in 0..3 { sums[i][c] += p[c]; }
        }
        for i in 0..k {
            if counts[i] > 0 { for c in 0..3 { centers[i][c] = sums[i][c] / counts[i] as f32; } }
        }
    }
    let clusters: Vec<([f32; 3], usize)> = centers.into_iter().zip(counts).filter(|(_, n)| *n > 0).collect();
    let dominant = clusters.iter().max_by_key(|(_, n)| *n).map(|c| c.0).unwrap_or([0.0; 3]);
    let significant: Vec<&([f32; 3], usize)> = clusters.iter().filter(|(_, n)| n * 20 >= px.len()).collect();
    let vibrant = significant.iter().max_by(|a, b| vibrancy(a.0).total_cmp(&vibrancy(b.0))).map_or(dominant, |c| c.0);
    let muted = significant.iter().min_by(|a, b| muted_score(a.0).total_cmp(&muted_score(b.0))).map_or(dominant, |c| c.0);
    Palette { dominant: hex(dominant), vibrant: hex(vibrant), muted: hex(muted) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(parts: &[([u8; 3], usize)]) -> Vec<u8> {
        parts.iter().flat_map(|(c, n)| std::iter::repeat_n(*c, *n)).flatten().collect()
    }

    #[test]
    fn solid_colour_fills_every_role() {
        let p = extract(&image(&[([200, 50, 50], 100)]));
        assert_eq!((p.dominant.as_str(), p.vibrant.as_str(), p.muted.as_str()), ("#c83232", "#c83232", "#c83232"));
    }

    #[test]
    fn grey_dominant_red_vibrant() {
        let p = extract(&image(&[([128, 128, 128], 700), ([220, 30, 30], 300)]));
        assert_eq!(p.dominant, "#808080");
        assert_eq!(p.vibrant, "#dc1e1e");
        assert_eq!(p.muted, "#808080");
    }

    #[test]
    fn tiny_specks_never_win_vibrant() {
        let p = extract(&image(&[([60, 60, 70], 990), ([255, 0, 255], 10)]));
        assert_ne!(p.vibrant, "#ff00ff");
    }

    #[test]
    fn empty_input_is_black() {
        assert_eq!(extract(&[]).dominant, "#000000");
    }
}

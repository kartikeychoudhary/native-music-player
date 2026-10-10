/// Perceptual volume curve: gain = v².
pub fn volume_gain(v: f32) -> f64 {
    let v = v.clamp(0.0, 1.0) as f64;
    v * v
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curve() {
        assert_eq!(volume_gain(1.0), 1.0);
        assert_eq!(volume_gain(0.5), 0.25);
        assert_eq!(volume_gain(0.0), 0.0);
        assert_eq!(volume_gain(3.0), 1.0);
    }
}

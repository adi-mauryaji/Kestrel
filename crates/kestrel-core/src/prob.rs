//! Probability helpers shared by the tiers that output class probabilities (Tier 3 head,
//! Tier 4 label scores). Here rather than in the router's calibration module (blueprint A.3) so
//! tier crates depend on core only (Ch. 24.2).

/// Softmax of `logits / temperature`. T = 1 is the raw softmax; T > 1 softens, T < 1 sharpens;
/// the arg-max never changes. The caller guarantees a finite `temperature > 0` (heads check it
/// when they load). Subtracting the max first keeps `exp` from overflowing.
pub fn softmax(logits: &[f32], temperature: f32) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits
        .iter()
        .map(|&z| ((z - max) / temperature).exp())
        .collect();
    let sum: f32 = exps.iter().sum();
    exps.into_iter().map(|e| e / sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argmax(v: &[f32]) -> usize {
        (0..v.len()).fold(0, |best, i| if v[i] > v[best] { i } else { best })
    }

    #[test]
    fn sums_to_one_and_keeps_the_argmax() {
        let z = [0.5f32, -1.0, 2.0, 1.5];
        for t in [0.5f32, 1.0, 3.0] {
            let p = softmax(&z, t);
            assert!((p.iter().sum::<f32>() - 1.0).abs() < 1e-6);
            assert_eq!(argmax(&p), 2);
        }
    }

    #[test]
    fn higher_temperature_softens_the_top_probability() {
        let z = [1.0f32, 2.0, 3.0];
        assert!(softmax(&z, 3.0)[2] < softmax(&z, 1.0)[2]);
        assert!(softmax(&z, 0.5)[2] > softmax(&z, 1.0)[2]);
    }

    #[test]
    fn huge_logits_do_not_overflow() {
        let p = softmax(&[1000.0, 0.0], 1.0);
        assert!(p.iter().all(|x| x.is_finite()));
        assert!((p[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn known_values() {
        // softmax([0, ln 3]) = [1/4, 3/4]; with T = 2: [1/(1+sqrt 3), sqrt 3/(1+sqrt 3)].
        let p = softmax(&[0.0, 3f32.ln()], 1.0);
        assert!((p[0] - 0.25).abs() < 1e-6 && (p[1] - 0.75).abs() < 1e-6);
        let p = softmax(&[0.0, 3f32.ln()], 2.0);
        let s3 = 3f32.sqrt();
        assert!((p[1] - s3 / (1.0 + s3)).abs() < 1e-6);
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(softmax(&[], 1.0).is_empty());
    }
}

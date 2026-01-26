use anyhow::Context;
use rubato::Resampler;

/// Resample mono f32 audio to a target sample rate.
///
/// Input is expected to be PCM samples in [-1, 1] with a known sample rate.
#[allow(dead_code)]
pub fn resample_mono_f32(
    input_samples: &[f32],
    input_sample_rate_hz: u32,
    target_sample_rate_hz: u32,
) -> anyhow::Result<Vec<f32>> {
    if input_sample_rate_hz == target_sample_rate_hz {
        return Ok(input_samples.to_vec());
    }

    let input_sample_rate_hz: usize = input_sample_rate_hz
        .try_into()
        .context("invalid input sample rate")?;
    let target_sample_rate_hz: usize = target_sample_rate_hz
        .try_into()
        .context("invalid target sample rate")?;

    let params = rubato::SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: rubato::SincInterpolationType::Cubic,
        oversampling_factor: 256,
        window: rubato::WindowFunction::BlackmanHarris2,
    };

    let mut resampler = rubato::SincFixedIn::<f32>::new(
        target_sample_rate_hz as f64 / input_sample_rate_hz as f64,
        2.0,
        params,
        input_samples.len(),
        1,
    )
    .context("create resampler")?;

    let input = vec![input_samples.to_vec()];
    let out = resampler.process(&input, None).context("resample")?;
    Ok(out.into_iter().next().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_identity_returns_same() {
        let x = vec![0.0, 0.5, -0.5, 0.25];
        let y = resample_mono_f32(&x, 16_000, 16_000).unwrap();
        assert_eq!(x, y);
    }
}

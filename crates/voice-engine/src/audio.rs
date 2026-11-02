//! Utilidades de audio PCM: mezcla a mono, remuestreo y conversión de formatos.

/// Promedia canales entrelazados a mono.
pub fn downmix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// Remuestreo lineal. Suficiente para voz (p. ej. 48 kHz → 16 kHz).
pub fn resample_linear(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() || from_hz == 0 || to_hz == 0 {
        return input.to_vec();
    }
    let ratio = f64::from(from_hz) / f64::from(to_hz);
    let out_len = ((input.len() as f64) / ratio).floor() as usize;
    let last = input.len() - 1;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = (pos - idx as f64) as f32;
            let a = input[idx.min(last)];
            let b = input[(idx + 1).min(last)];
            a + (b - a) * frac
        })
        .collect()
}

/// Convierte PCM `i16` little-endian a `f32` en `[-1, 1)`.
///
/// Un byte final suelto (muestra incompleta) se ignora.
pub fn s16le_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
    }

    #[test]
    fn resample_changes_length_proportionally() {
        let input = vec![0.0; 48_000];
        assert_eq!(resample_linear(&input, 48_000, 16_000).len(), 16_000);
        assert_eq!(resample_linear(&input, 16_000, 16_000).len(), 48_000);
    }

    #[test]
    fn resample_interpolates() {
        let out = resample_linear(&[0.0, 1.0], 1, 2);
        assert_eq!(out, vec![0.0, 0.5, 1.0, 1.0]);
    }

    #[test]
    fn s16_conversion_ignores_odd_byte() {
        let out = s16le_to_f32(&[0x00, 0x40, 0xFF]);
        assert_eq!(out, vec![0.5]);
    }
}

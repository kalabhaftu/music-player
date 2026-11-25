//! Perceived loudness volume curve and utilities.
//!
//! Mirrors C# `VolumeController` and `PlaybackService` volume mechanics,
//! specifically the quadratic perceived loudness curve `Math.Pow(volume / 100.0, 2.0)`
//! ensuring smooth low-volume control without precipitous drop-offs.

/// Calculates the normalized perceived loudness power curve (0.0 to 1.0)
/// matching C# `Math.Pow(volume / 100.0, 2.0)`.
#[inline]
pub fn quadratic_volume_curve(volume: f64) -> f64 {
    let normalized = (volume / 100.0).clamp(0.0, 1.0);
    normalized.powf(2.0)
}

/// Alias for `quadratic_volume_curve`: converts 0..100 volume slider value
/// to a normalized amplitude gain factor (0.0 ..= 1.0).
#[inline]
pub fn perceived_loudness_to_amplitude(volume: f64) -> f64 {
    quadratic_volume_curve(volume)
}

/// Inverts the quadratic loudness curve: maps an amplitude factor (0.0 ..= 1.0)
/// back to a volume slider value (0.0 ..= 100.0).
#[inline]
pub fn amplitude_to_perceived_loudness(amplitude: f64) -> f64 {
    amplitude.clamp(0.0, 1.0).sqrt() * 100.0
}

/// Maps a slider volume (0..100) using the quadratic perceived loudness curve
/// to an integer volume scale (0..100).
#[inline]
pub fn perceived_to_linear_volume(volume: u32) -> u32 {
    (quadratic_volume_curve(volume as f64) * 100.0).round() as u32
}

/// Inverts an integer linear volume scale (0..100) back to the slider volume (0..100).
#[inline]
pub fn linear_to_perceived_volume(linear_vol: u32) -> u32 {
    let norm = (linear_vol as f64 / 100.0).clamp(0.0, 1.0);
    (norm.sqrt() * 100.0).round() as u32
}

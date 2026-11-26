use serde::{Deserialize, Serialize};

/// 10-band equalizer ISO center frequencies in Hz:
/// 32Hz, 64Hz, 125Hz, 250Hz, 500Hz, 1kHz, 2kHz, 4kHz, 8kHz, 16kHz
/// matching C# `EQUALIZER_BANDS`.
pub const EQUALIZER_BANDS: [u32; 10] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

pub const EQUALIZER_BAND_FREQUENCIES_HZ: [f32; 10] =
    [32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0];

pub const EQUALIZER_BAND_LABELS: [&str; 10] =
    ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

/// VLC's one-pass equalizer filter attenuates dry input by 0.25, so a neutral
/// curve must be attached with this preamp to pass audio through untouched.
/// Mirrors `PlaybackService.NeutralEqualizerPreamp` (12.0412 dB).
pub const NEUTRAL_EQUALIZER_PREAMP: f32 = 12.0412;

/// Minimum preamp gain in dB.
pub const EQUALIZER_PREAMP_MIN: f32 = -20.0;

/// Maximum preamp gain in dB.
pub const EQUALIZER_PREAMP_MAX: f32 = 20.0;

/// Legacy alias for preamp limit (+-20.0 dB).
pub const EQUALIZER_PREAMP_LIMIT: f32 = 20.0;

/// Minimum band gain in dB.
pub const EQUALIZER_BAND_GAIN_MIN: f32 = -20.0;

/// Maximum band gain in dB.
pub const EQUALIZER_BAND_GAIN_MAX: f32 = 20.0;

/// Canonical 10-band equalizer presets matching the canonical 10-band presets
/// (32Hz, 64Hz, 125Hz, 250Hz, 500Hz, 1kHz, 2kHz, 4kHz, 8kHz, 16kHz) with exact gain values.
pub const CANONICAL_PRESETS: &[(&str, [f32; 10])] = &[
    ("Acoustic", [5.0, 5.0, 4.0, 1.0, 1.5, 1.5, 3.5, 4.0, 3.5, 2.0]),
    ("Bass Boost", [6.0, 5.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Bass Reducer", [-5.5, -4.5, -4.0, -2.5, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Classical", [5.0, 4.0, 3.0, 2.5, -1.5, -1.5, 0.0, 2.0, 3.0, 3.5]),
    ("Electronic", [5.0, 4.0, 2.0, 0.0, -2.0, 2.0, 1.0, 2.0, 4.0, 5.0]),
    ("Flat", [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
    ("Hip-Hop", [5.0, 4.0, 1.5, 3.0, -1.0, -1.0, 1.5, -0.5, 2.0, 3.0]),
    ("Jazz", [4.0, 3.0, 1.5, 2.0, -1.5, -1.5, 0.0, 1.5, 3.0, 3.5]),
    ("Latin", [4.5, 3.0, 0.0, 0.0, -1.5, -1.5, -1.5, 0.0, 3.0, 4.5]),
    ("Loudness", [6.0, 4.0, 0.0, 0.0, -2.0, 0.0, -1.0, -5.0, 5.0, 1.0]),
    ("Lounge", [-3.0, -1.5, -0.5, 1.5, 4.0, 2.5, 0.0, -1.5, 2.0, 1.0]),
    ("Piano", [3.0, 2.0, 0.0, 2.5, 3.0, 1.5, 3.5, 4.5, 3.0, 3.5]),
    ("Pop", [-1.5, -1.0, 0.0, 2.0, 4.0, 4.0, 2.0, 0.0, -1.0, -1.5]),
    ("R&B", [2.5, 7.0, 5.5, 1.5, -2.5, -1.5, 2.5, 3.0, 3.0, 4.0]),
    ("Rock", [4.5, 3.0, 2.0, 0.0, -1.0, -0.5, 1.5, 3.0, 4.0, 4.5]),
    ("Small Speakers", [5.5, 4.0, 3.5, 2.5, 1.0, 0.0, -1.0, -2.0, -3.5, -4.0]),
    ("Spoken Word", [-3.5, -0.5, 0.0, 0.5, 3.5, 4.5, 5.0, 4.5, 2.5, 0.0]),
    ("Treble Boost", [0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 3.5, 4.5, 5.5]),
    ("Treble Reducer", [0.0, 0.0, 0.0, 0.0, 0.0, -1.0, -2.5, -3.5, -4.5, -5.5]),
    ("Vocal Boost", [-1.5, -2.0, -1.0, 1.0, 3.5, 3.5, 2.0, 1.0, 0.0, -1.0]),
];

/// Returns the list of all 20 canonical preset names.
pub fn canonical_preset_names() -> &'static [&'static str] {
    &[
        "Acoustic",
        "Bass Boost",
        "Bass Reducer",
        "Classical",
        "Electronic",
        "Flat",
        "Hip-Hop",
        "Jazz",
        "Latin",
        "Loudness",
        "Lounge",
        "Piano",
        "Pop",
        "R&B",
        "Rock",
        "Small Speakers",
        "Spoken Word",
        "Treble Boost",
        "Treble Reducer",
        "Vocal Boost",
    ]
}

/// Look up band gains by canonical preset name (case-insensitive, handling common aliases).
pub fn canonical_preset_by_name(name: &str) -> Option<[f32; 10]> {
    let trimmed = name.trim();
    for &(preset_name, gains) in CANONICAL_PRESETS {
        if preset_name.eq_ignore_ascii_case(trimmed) {
            return Some(gains);
        }
    }
    if trimmed.eq_ignore_ascii_case("Normal") {
        return canonical_preset_by_name("Flat");
    }
    // Handle aliases like "Bass Booster", "Treble Booster", "Vocal Booster", "Vocal", "RnB"
    if trimmed.eq_ignore_ascii_case("Bass Booster") {
        return canonical_preset_by_name("Bass Boost");
    }
    if trimmed.eq_ignore_ascii_case("Treble Booster") {
        return canonical_preset_by_name("Treble Boost");
    }
    if trimmed.eq_ignore_ascii_case("Vocal Booster") || trimmed.eq_ignore_ascii_case("Vocal") {
        return canonical_preset_by_name("Vocal Boost");
    }
    if trimmed.eq_ignore_ascii_case("RnB") || trimmed.eq_ignore_ascii_case("R & B") {
        return canonical_preset_by_name("R&B");
    }
    None
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EqualizerState {
    pub enabled: bool,
    pub preamp_db: f32,
    pub band_gains_db: [f32; 10],
}

impl Default for EqualizerState {
    fn default() -> Self {
        Self {
            enabled: false,
            preamp_db: 0.0,
            band_gains_db: [0.0; 10],
        }
    }
}

impl EqualizerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the real-time preamp gain in dB, clamped to [-20.0, 20.0].
    pub fn set_preamp(&mut self, preamp_db: f32) {
        self.preamp_db = preamp_db.clamp(EQUALIZER_PREAMP_MIN, EQUALIZER_PREAMP_MAX);
    }

    /// Sets a specific band gain in dB, clamped to [-20.0, 20.0].
    pub fn set_band_gain(&mut self, band_index: usize, gain_db: f32) {
        if band_index < 10 {
            self.band_gains_db[band_index] =
                gain_db.clamp(EQUALIZER_BAND_GAIN_MIN, EQUALIZER_BAND_GAIN_MAX);
        }
    }

    /// Constructs an `EqualizerState` from a preset name.
    pub fn from_preset(name: &str) -> Option<Self> {
        canonical_preset_by_name(name).map(|gains| Self {
            enabled: true,
            preamp_db: 0.0,
            band_gains_db: gains,
        })
    }

    pub fn preset_acoustic() -> Self {
        Self::from_preset("Acoustic").unwrap()
    }

    pub fn preset_bass_boost() -> Self {
        Self::from_preset("Bass Boost").unwrap()
    }

    pub fn preset_bass_reducer() -> Self {
        Self::from_preset("Bass Reducer").unwrap()
    }

    pub fn preset_classical() -> Self {
        Self::from_preset("Classical").unwrap()
    }

    pub fn preset_electronic() -> Self {
        Self::from_preset("Electronic").unwrap()
    }

    pub fn preset_flat() -> Self {
        Self::default()
    }

    pub fn preset_hip_hop() -> Self {
        Self::from_preset("Hip-Hop").unwrap()
    }

    pub fn preset_jazz() -> Self {
        Self::from_preset("Jazz").unwrap()
    }

    pub fn preset_latin() -> Self {
        Self::from_preset("Latin").unwrap()
    }

    pub fn preset_loudness() -> Self {
        Self::from_preset("Loudness").unwrap()
    }

    pub fn preset_lounge() -> Self {
        Self::from_preset("Lounge").unwrap()
    }

    pub fn preset_piano() -> Self {
        Self::from_preset("Piano").unwrap()
    }

    pub fn preset_pop() -> Self {
        Self::from_preset("Pop").unwrap()
    }

    pub fn preset_rnb() -> Self {
        Self::from_preset("R&B").unwrap()
    }

    pub fn preset_rock() -> Self {
        Self::from_preset("Rock").unwrap()
    }

    pub fn preset_small_speakers() -> Self {
        Self::from_preset("Small Speakers").unwrap()
    }

    pub fn preset_spoken_word() -> Self {
        Self::from_preset("Spoken Word").unwrap()
    }

    pub fn preset_treble_boost() -> Self {
        Self::from_preset("Treble Boost").unwrap()
    }

    pub fn preset_treble_reducer() -> Self {
        Self::from_preset("Treble Reducer").unwrap()
    }

    pub fn preset_vocal_boost() -> Self {
        Self::from_preset("Vocal Boost").unwrap()
    }

    /// Legacy alias for `preset_vocal_boost`.
    pub fn preset_vocal() -> Self {
        Self::preset_vocal_boost()
    }
}

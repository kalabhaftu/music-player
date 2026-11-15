use serde::{Deserialize, Serialize};

pub const EQUALIZER_BANDS: [u32; 10] = [31, 62, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];

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
    pub fn set_band_gain(&mut self, band_index: usize, gain_db: f32) {
        if band_index < 10 {
            self.band_gains_db[band_index] = gain_db.clamp(-12.0, 12.0);
        }
    }

    pub fn preset_flat() -> Self {
        Self::default()
    }

    pub fn preset_bass_boost() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            band_gains_db: [6.0, 5.0, 4.0, 2.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        }
    }

    pub fn preset_rock() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            band_gains_db: [4.5, 3.0, 2.0, 0.0, -1.0, -0.5, 1.5, 3.0, 4.0, 4.5],
        }
    }

    pub fn preset_vocal() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            band_gains_db: [-1.5, -2.0, -1.0, 1.0, 3.5, 3.5, 2.0, 1.0, 0.0, -1.0],
        }
    }

    pub fn preset_electronic() -> Self {
        Self {
            enabled: true,
            preamp_db: 0.0,
            band_gains_db: [5.0, 4.0, 2.0, 0.0, -2.0, 2.0, 1.0, 2.0, 4.0, 5.0],
        }
    }
}

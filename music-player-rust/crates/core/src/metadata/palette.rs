use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl RgbColor {
    pub fn hex(&self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }

    pub fn luminance(&self) -> f64 {
        let linear = |v: u8| -> f64 {
            let val = v as f64 / 255.0;
            if val <= 0.04045 {
                val / 12.92
            } else {
                ((val + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    pub fn contrast_ratio(&self, other: RgbColor) -> f64 {
        let l1 = self.luminance();
        let l2 = other.luminance();
        let (lighter, darker) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (lighter + 0.05) / (darker + 0.05)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtworkPalette {
    pub background: RgbColor,
    pub sidebar: RgbColor,
    pub main: RgbColor,
    pub panel: RgbColor,
    pub raised: RgbColor,
    pub hover: RgbColor,
    pub control: RgbColor,
    pub accent: RgbColor,
}

impl ArtworkPalette {
    pub fn default_dark() -> Self {
        Self {
            background: RgbColor { r: 7, g: 8, b: 7 },
            sidebar: RgbColor { r: 9, g: 10, b: 9 },
            main: RgbColor { r: 7, g: 8, b: 7 },
            panel: RgbColor { r: 14, g: 16, b: 14 },
            raised: RgbColor { r: 23, g: 25, b: 23 },
            hover: RgbColor { r: 33, g: 36, b: 33 },
            control: RgbColor { r: 36, g: 39, b: 36 },
            accent: RgbColor { r: 183, g: 255, b: 45 },
        }
    }
}

pub fn extract_palette(image_path: &Path) -> ArtworkPalette {
    let img = match image::open(image_path) {
        Ok(i) => i.to_rgba8(),
        Err(_) => return ArtworkPalette::default_dark(),
    };
    // The C# shell decodes artwork at 32x32 before building the palette; doing
    // the same keeps track changes cheap and produces the same dominant hues.
    let img = image::imageops::resize(&img, 32, 32, image::imageops::FilterType::Triangle);

    let pixels = img.as_raw();
    let mut bins = [(0.0f64, 0.0f64, 0.0f64, 0.0f64); 19];
    let mut visible = 0.0f64;

    let mut i = 0;
    while i + 3 < pixels.len() {
        let alpha = pixels[i + 3] as f64 / 255.0;
        if alpha >= 0.1 {
            let r = pixels[i] as f64 / 255.0;
            let g = pixels[i + 1] as f64 / 255.0;
            let b = pixels[i + 2] as f64 / 255.0;

            let max = r.max(g.max(b));
            let min = r.min(g.min(b));
            let chroma = max - min;
            let saturation = if max == 0.0 { 0.0 } else { chroma / max };
            let hue = if chroma == 0.0 {
                0.0
            } else if max == r {
                ((g - b) / chroma + 6.0) % 6.0
            } else if max == g {
                (b - r) / chroma + 2.0
            } else {
                (r - g) / chroma + 4.0
            };

            let bin_idx = if saturation >= 0.18 && max >= 0.08 {
                ((hue * 3.0).floor() as usize) % 18
            } else {
                18
            };

            bins[bin_idx].0 += alpha;
            bins[bin_idx].1 += pixels[i] as f64 * alpha;
            bins[bin_idx].2 += pixels[i + 1] as f64 * alpha;
            bins[bin_idx].3 += pixels[i + 2] as f64 * alpha;
            visible += alpha;
        }
        i += 4;
    }

    let mut selected = 18;
    for idx in 0..18 {
        if bins[idx].0 >= visible * 0.05 && (selected == 18 || bins[idx].0 > bins[selected].0) {
            selected = idx;
        }
    }

    let chosen = bins[selected];
    let clamp_byte = |v: f64| -> u8 { v.round().clamp(0.0, 255.0) as u8 };

    let mut seed = if chosen.0 > 0.0 {
        RgbColor {
            r: clamp_byte(chosen.1 / chosen.0),
            g: clamp_byte(chosen.2 / chosen.0),
            b: clamp_byte(chosen.3 / chosen.0),
        }
    } else {
        RgbColor { r: 183, g: 255, b: 45 }
    };

    if selected == 18 {
        let gray = clamp_byte(0.2126 * seed.r as f64 + 0.7152 * seed.g as f64 + 0.0722 * seed.b as f64);
        seed = RgbColor { r: gray, g: gray, b: gray };
    }

    let maximum = 1.0f64.max(seed.r.max(seed.g.max(seed.b)) as f64);
    let tone = |shade: f64| -> RgbColor {
        RgbColor {
            r: clamp_byte(10.0 + (seed.r as f64 / maximum) * shade),
            g: clamp_byte(10.0 + (seed.g as f64 / maximum) * shade),
            b: clamp_byte(10.0 + (seed.b as f64 / maximum) * shade),
        }
    };

    let background = tone(38.0);
    let sidebar = tone(34.0);
    let main = tone(42.0);
    let raised = tone(50.0);
    let hover = tone(58.0);
    let control = tone(64.0);

    let lighten = |c: RgbColor, amt: f64| -> RgbColor {
        RgbColor {
            r: clamp_byte(c.r as f64 + (255.0 - c.r as f64) * amt),
            g: clamp_byte(c.g as f64 + (255.0 - c.g as f64) * amt),
            b: clamp_byte(c.b as f64 + (255.0 - c.b as f64) * amt),
        }
    };

    let mut accent = seed;
    for step in 0..=100 {
        if (accent.luminance() + 0.05) / (control.luminance() + 0.05) >= 4.5 {
            break;
        }
        accent = lighten(seed, step as f64 / 100.0);
    }

    ArtworkPalette {
        background,
        sidebar,
        main,
        panel: background,
        raised,
        hover,
        control,
        accent,
    }
}

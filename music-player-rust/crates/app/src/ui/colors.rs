use slint::Color;

/// FNV-1a slot 1..=6, mirroring the WebUI `cover()` helper in app.js:
/// hash = 2166136261; hash = Math.imul(hash ^ ch.charCodeAt(0), 16777619);
/// cls = `c${Math.abs(hash >>> 0) % 6 + 1}`
pub fn gradient_slot(seed: &str) -> i32 {
    let mut hash: u32 = 2166136261;
    for ch in seed.encode_utf16() {
        hash = (hash ^ (ch as u32)).wrapping_mul(16777619);
    }
    ((hash % 6) + 1) as i32
}

pub fn badge_color(slot: i32) -> Color {
    let (r, g, b) = match slot {
        1 => (0x2a, 0x2f, 0x3f),
        2 => (0x21, 0x33, 0x37),
        3 => (0x38, 0x27, 0x2b),
        4 => (0x24, 0x33, 0x28),
        5 => (0x35, 0x30, 0x25),
        6 => (0x2a, 0x2a, 0x32),
        _ => (0x2a, 0x2f, 0x3f),
    };
    Color::from_rgb_u8(r, g, b)
}

pub fn parse_hex_color(s: &str) -> Color {
    let s = s.trim().trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(183);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(45);
        Color::from_rgb_u8(r, g, b)
    } else {
        Color::from_rgb_u8(183, 255, 45)
    }
}

pub fn contrasting_ink(hex: &str) -> Color {
    let value = hex.trim_start_matches('#');
    let Some((r, g, b)) = (|| {
        Some((
            u8::from_str_radix(value.get(0..2)?, 16).ok()?,
            u8::from_str_radix(value.get(2..4)?, 16).ok()?,
            u8::from_str_radix(value.get(4..6)?, 16).ok()?,
        ))
    })() else {
        return parse_hex_color("#101210");
    };
    let linear = |channel: u8| {
        let value = channel as f64 / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    let luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    if (luminance + 0.05) / 0.05 >= 1.05 / (luminance + 0.05) {
        parse_hex_color("#101210")
    } else {
        parse_hex_color("#ffffff")
    }
}

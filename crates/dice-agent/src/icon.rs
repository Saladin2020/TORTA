//! App / tray icons (high-contrast for Windows system tray).

use tray_icon::Icon;

/// Build a crisp tray icon (32×32 RGBA). Prefers embedded PNG, else draws a dice mark.
pub fn tray_icon() -> Icon {
    if let Some(icon) = load_png_icon(include_bytes!("../assets/icon-32.png"), 32) {
        return icon;
    }
    Icon::from_rgba(draw_dice_rgba(32), 32, 32).expect("tray icon")
}

/// egui window icon data.
pub fn window_icon() -> egui::IconData {
    if let Ok(img) = image::load_from_memory(include_bytes!("../assets/icon-48.png")) {
        let rgba = img.to_rgba8();
        return egui::IconData {
            rgba: rgba.as_raw().to_vec(),
            width: rgba.width(),
            height: rgba.height(),
        };
    }
    egui::IconData {
        rgba: draw_dice_rgba(48),
        width: 48,
        height: 48,
    }
}

fn load_png_icon(bytes: &[u8], size: u32) -> Option<Icon> {
    let img = image::load_from_memory(bytes).ok()?;
    let rgba = img
        .resize_exact(size, size, image::imageops::FilterType::Triangle)
        .to_rgba8();
    Icon::from_rgba(rgba.into_raw(), size, size).ok()
}

/// Procedural black tile + white dice (always readable in the overflow tray).
fn draw_dice_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let s = size as i32;
    let r = (size as f32 * 0.18) as i32; // corner radius

    for y in 0..s {
        for x in 0..s {
            let i = ((y * s + x) * 4) as usize;
            let inside = rounded_rect_contains(x, y, s, r);
            if inside {
                // near-black fill
                rgba[i] = 12;
                rgba[i + 1] = 12;
                rgba[i + 2] = 12;
                rgba[i + 3] = 255;
            } else {
                rgba[i + 3] = 0; // transparent outside
            }
        }
    }

    // White cube (isometric-ish square)
    let m = (size as f32 * 0.28) as i32;
    let cube = m..(s - m);
    for y in cube.clone() {
        for x in cube.clone() {
            let i = ((y * s + x) * 4) as usize;
            rgba[i] = 255;
            rgba[i + 1] = 255;
            rgba[i + 2] = 255;
            rgba[i + 3] = 255;
        }
    }

    // Three pips on the die face
    let pip_r = (size as f32 * 0.045).max(1.0) as i32;
    let cx = s / 2;
    let cy = s / 2;
    let d = (size as f32 * 0.12) as i32;
    for (px, py) in [(cx - d, cy - d), (cx, cy), (cx + d, cy + d)] {
        fill_circle(&mut rgba, s, px, py, pip_r, [12, 12, 12, 255]);
    }

    rgba
}

fn rounded_rect_contains(x: i32, y: i32, s: i32, r: i32) -> bool {
    let inset = 1;
    let x0 = inset + r;
    let y0 = inset + r;
    let x1 = s - 1 - inset - r;
    let y1 = s - 1 - inset - r;
    if x >= x0 && x <= x1 && y >= inset && y <= s - 1 - inset {
        return true;
    }
    if y >= y0 && y <= y1 && x >= inset && x <= s - 1 - inset {
        return true;
    }
    // corners
    let corners = [
        (x0, y0),
        (x1, y0),
        (x0, y1),
        (x1, y1),
    ];
    for (cx, cy) in corners {
        let dx = x - cx;
        let dy = y - cy;
        if dx * dx + dy * dy <= r * r {
            return true;
        }
    }
    false
}

fn fill_circle(rgba: &mut [u8], s: i32, cx: i32, cy: i32, rad: i32, color: [u8; 4]) {
    for y in (cy - rad)..=(cy + rad) {
        for x in (cx - rad)..=(cx + rad) {
            if x < 0 || y < 0 || x >= s || y >= s {
                continue;
            }
            let dx = x - cx;
            let dy = y - cy;
            if dx * dx + dy * dy <= rad * rad {
                let i = ((y * s + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&color);
            }
        }
    }
}

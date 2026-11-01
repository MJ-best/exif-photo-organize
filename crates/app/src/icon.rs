//! Procedurally drawn app icon (so the binary needs no image assets).

use eframe::egui::IconData;

fn smooth(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

pub fn icon() -> IconData {
    let n = 256usize;
    let mut rgba = vec![0u8; n * n * 4];
    let (r, pad) = (52.0f32, 10.0f32);
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // Rounded square SDF.
            let half = n as f32 / 2.0 - pad;
            let (cx, cy) = (fx - n as f32 / 2.0, fy - n as f32 / 2.0);
            let qx = cx.abs() - (half - r);
            let qy = cy.abs() - (half - r);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            let a = smooth(outside);
            if a <= 0.0 {
                continue;
            }
            // Background gradient: system blue -> indigo.
            let t = fy / n as f32;
            let mut c = [10.0 + 60.0 * t, 132.0 - 60.0 * t, 255.0 - 30.0 * t];
            // Sun.
            let sd = ((fx - 168.0).powi(2) + (fy - 92.0).powi(2)).sqrt() - 30.0;
            let sa = smooth(sd);
            for (i, v) in [255.0, 236.0, 170.0].iter().enumerate() {
                c[i] = c[i] * (1.0 - sa) + v * sa;
            }
            // Two mountain layers.
            for (peak_x, peak_y, slope, col) in
                [(96.0f32, 120.0f32, 0.95f32, [235.0f32, 240.0, 255.0]), (176.0, 150.0, 0.8, [255.0, 255.0, 255.0])]
            {
                let ridge = peak_y + (fx - peak_x).abs() * slope;
                let ma = smooth(ridge - fy);
                for i in 0..3 {
                    c[i] = c[i] * (1.0 - ma) + col[i] * ma;
                }
            }
            let o = (y * n + x) * 4;
            rgba[o] = c[0].clamp(0.0, 255.0) as u8;
            rgba[o + 1] = c[1].clamp(0.0, 255.0) as u8;
            rgba[o + 2] = c[2].clamp(0.0, 255.0) as u8;
            rgba[o + 3] = (a * 255.0) as u8;
        }
    }
    IconData { rgba, width: n as u32, height: n as u32 }
}

#[cfg(test)]
mod tests {
    #[test]
    fn icon_renders() {
        let i = super::icon();
        assert_eq!(i.rgba.len(), 256 * 256 * 4);
        // Corners transparent, centre opaque.
        assert_eq!(i.rgba[3], 0);
        assert_eq!(i.rgba[(128 * 256 + 128) * 4 + 3], 255);
        if let Ok(p) = std::env::var("ICON_OUT") {
            let img = photoorg_core::image::RgbaImage::from_raw(256, 256, i.rgba).unwrap();
            img.save(p).unwrap();
        }
    }
}

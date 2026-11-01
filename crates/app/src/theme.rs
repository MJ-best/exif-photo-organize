//! Colours and egui style tuned to look like macOS Photos.

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    pub sidebar_bg: Color32,
    pub content_bg: Color32,
    pub toolbar_bg: Color32,
    pub viewer_bg: Color32,
    pub panel_bg: Color32,
    pub text: Color32,
    pub text2: Color32,
    pub text3: Color32,
    pub separator: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub sidebar_sel: Color32,
    pub hover: Color32,
    pub control_bg: Color32,
    pub control_sel: Color32,
    pub field_bg: Color32,
    pub star: Color32,
    pub placeholder: Color32,
    pub danger: Color32,
    pub success: Color32,
    /// B-cut candidates.
    pub warning: Color32,
    pub shadow: Color32,
}

impl Palette {
    pub fn dark() -> Palette {
        Palette {
            dark: true,
            sidebar_bg: Color32::from_rgb(0x28, 0x28, 0x2A),
            content_bg: Color32::from_rgb(0x1E, 0x1E, 0x1E),
            toolbar_bg: Color32::from_rgb(0x26, 0x26, 0x28),
            viewer_bg: Color32::from_rgb(0x12, 0x12, 0x12),
            panel_bg: Color32::from_rgb(0x2A, 0x2A, 0x2C),
            text: Color32::from_rgb(0xF2, 0xF2, 0xF7),
            text2: Color32::from_rgb(0x98, 0x98, 0x9D),
            text3: Color32::from_rgb(0x63, 0x63, 0x66),
            separator: Color32::from_rgb(0x3A, 0x3A, 0x3C),
            accent: Color32::from_rgb(0x0A, 0x84, 0xFF),
            accent_text: Color32::WHITE,
            sidebar_sel: Color32::from_rgb(0x46, 0x46, 0x4A),
            hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
            control_bg: Color32::from_rgb(0x3A, 0x3A, 0x3C),
            control_sel: Color32::from_rgb(0x63, 0x63, 0x66),
            field_bg: Color32::from_rgb(0x34, 0x34, 0x36),
            star: Color32::from_rgb(0xFF, 0xD6, 0x0A),
            placeholder: Color32::from_rgb(0x2C, 0x2C, 0x2E),
            danger: Color32::from_rgb(0xFF, 0x45, 0x3A),
            success: Color32::from_rgb(0x30, 0xD1, 0x58),
            warning: Color32::from_rgb(0xFF, 0x9F, 0x0A),
            shadow: Color32::from_black_alpha(110),
        }
    }

    pub fn light() -> Palette {
        Palette {
            dark: false,
            sidebar_bg: Color32::from_rgb(0xEE, 0xEE, 0xF0),
            content_bg: Color32::WHITE,
            toolbar_bg: Color32::from_rgb(0xF7, 0xF7, 0xF8),
            viewer_bg: Color32::from_rgb(0xF5, 0xF5, 0xF7),
            panel_bg: Color32::from_rgb(0xF7, 0xF7, 0xF8),
            text: Color32::from_rgb(0x1D, 0x1D, 0x1F),
            text2: Color32::from_rgb(0x6E, 0x6E, 0x73),
            text3: Color32::from_rgb(0xAE, 0xAE, 0xB2),
            separator: Color32::from_rgb(0xD8, 0xD8, 0xDC),
            accent: Color32::from_rgb(0x00, 0x7A, 0xFF),
            accent_text: Color32::WHITE,
            sidebar_sel: Color32::from_rgb(0xD9, 0xD9, 0xDE),
            hover: Color32::from_rgba_unmultiplied(0, 0, 0, 10),
            control_bg: Color32::from_rgb(0xE3, 0xE3, 0xE8),
            control_sel: Color32::WHITE,
            field_bg: Color32::from_rgb(0xE8, 0xE8, 0xED),
            star: Color32::from_rgb(0xFF, 0x9F, 0x0A),
            placeholder: Color32::from_rgb(0xEC, 0xEC, 0xF0),
            danger: Color32::from_rgb(0xFF, 0x3B, 0x30),
            success: Color32::from_rgb(0x34, 0xC7, 0x59),
            warning: Color32::from_rgb(0xFF, 0x95, 0x00),
            shadow: Color32::from_black_alpha(40),
        }
    }
}

pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(crate::fonts::BOLD.into()))
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.content_bg;
    v.window_fill = p.panel_bg;
    v.extreme_bg_color = p.field_bg;
    v.faint_bg_color = p.hover;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, p.accent_text);
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(1.0, p.separator);
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: p.shadow };
    v.popup_shadow = egui::Shadow { offset: [0, 6], blur: 20, spread: 0, color: p.shadow };
    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.weak_bg_fill = p.control_bg;
    v.widgets.inactive.bg_fill = p.control_bg;
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.weak_bg_fill = p.control_sel;
    v.widgets.hovered.bg_fill = p.control_sel;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.active.weak_bg_fill = p.accent;
    v.widgets.active.bg_fill = p.accent;
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.accent_text);
    v.widgets.open.weak_bg_fill = p.control_sel;
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.text_styles.insert(TextStyle::Body, font(13.0));
        s.text_styles.insert(TextStyle::Button, font(13.0));
        s.text_styles.insert(TextStyle::Small, font(11.0));
        s.text_styles.insert(TextStyle::Heading, bold(17.0));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
        s.spacing.scroll.bar_width = 8.0;
        s.spacing.scroll.floating = true;
        s.interaction.selectable_labels = false;
    });
}

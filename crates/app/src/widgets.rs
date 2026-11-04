//! Small custom widgets in the style of macOS: sidebar rows, segmented control, pill buttons,
//! star ratings, switches and vector icons.

use crate::theme::{bold, font, Palette};
use eframe::egui::{
    self, pos2, vec2, Align2, Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Photos,
    Star,
    Clock,
    Video,
    Raw,
    Camera,
    Lens,
    Card,
    Folder,
    Eject,
    Import,
    Gear,
    Info,
    Filter,
    Search,
    Left,
    Right,
    Close,
    Check,
    Library,
    Plus,
    Minus,
    /// Pick (A-cut) flag.
    Flag,
    /// Reject (B-cut).
    Reject,
    /// Burst stack.
    Stack,
    /// B-cut candidate (eye with a question).
    Suspect,
    /// A-cut candidate (magnifier with an A).
    Keeper,
    /// Scene (mountains).
    Scene,
    /// Project (작품): an open photo book.
    Album,
    /// Theme: a tag.
    Tag,
}

/// Paint an icon centred in `rect` (designed on a 16×16 grid).
pub fn paint_icon(p: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 16.0;
    let c = rect.center();
    let at = |x: f32, y: f32| pos2(c.x + (x - 8.0) * s, c.y + (y - 8.0) * s);
    let st = Stroke::new((1.4 * s).max(1.0), color);
    let line = |pts: &[(f32, f32)]| {
        p.add(Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), st));
    };
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| Rect::from_min_max(at(x0, y0), at(x1, y1));
    match icon {
        Icon::Photos => {
            p.rect_stroke(r(2.0, 3.0, 14.0, 13.0), 2.0 * s, st, StrokeKind::Middle);
            line(&[(2.5, 11.5), (6.0, 8.0), (9.0, 10.5), (11.0, 8.5), (13.5, 11.0)]);
            p.circle_filled(at(10.5, 6.0), 1.2 * s, color);
        }
        Icon::Library => {
            p.rect_stroke(r(1.5, 4.5, 11.5, 14.0), 1.5 * s, st, StrokeKind::Middle);
            line(&[(4.0, 2.5), (14.0, 2.5), (14.0, 11.5)]);
            line(&[(2.0, 12.0), (5.0, 9.0), (7.5, 11.0), (11.0, 8.0)]);
        }
        Icon::Star => {
            star_shape(p, c, 7.0 * s, Some(color), None);
        }
        Icon::Clock => {
            p.circle_stroke(c, 6.0 * s, st);
            line(&[(8.0, 4.5), (8.0, 8.0), (10.5, 9.5)]);
        }
        Icon::Video => {
            p.rect_stroke(r(1.5, 4.5, 10.5, 11.5), 1.5 * s, st, StrokeKind::Middle);
            line(&[(10.5, 7.0), (14.5, 4.8), (14.5, 11.2), (10.5, 9.0)]);
        }
        Icon::Raw => {
            line(&[(4.0, 1.5), (10.0, 1.5), (13.0, 4.5), (13.0, 14.5), (4.0, 14.5), (4.0, 1.5)]);
            line(&[(10.0, 1.5), (10.0, 4.5), (13.0, 4.5)]);
            p.text(at(8.5, 10.0), Align2::CENTER_CENTER, "R", bold(7.0 * s), color);
        }
        Icon::Camera => {
            p.rect_stroke(r(1.5, 4.5, 14.5, 13.5), 2.0 * s, st, StrokeKind::Middle);
            line(&[(5.0, 4.5), (6.0, 2.5), (10.0, 2.5), (11.0, 4.5)]);
            p.circle_stroke(at(8.0, 9.0), 2.6 * s, st);
        }
        Icon::Lens => {
            p.circle_stroke(c, 6.2 * s, st);
            p.circle_stroke(c, 3.2 * s, st);
            p.circle_filled(at(9.2, 6.8), 0.8 * s, color);
        }
        Icon::Card => {
            line(&[(4.0, 1.5), (11.0, 1.5), (13.0, 3.5), (13.0, 14.5), (3.0, 14.5), (3.0, 2.5), (4.0, 1.5)]);
            for x in [5.5, 7.5, 9.5] {
                line(&[(x, 3.5), (x, 6.0)]);
            }
        }
        Icon::Folder => {
            line(&[(1.5, 4.0), (1.5, 13.0), (14.5, 13.0), (14.5, 5.5), (7.5, 5.5), (6.0, 3.5), (1.5, 3.5), (1.5, 4.0)]);
        }
        Icon::Eject => {
            p.add(Shape::convex_polygon(vec![at(8.0, 3.0), at(13.0, 9.0), at(3.0, 9.0)], color, Stroke::NONE));
            p.rect_filled(r(3.0, 11.0, 13.0, 13.0), 0.5 * s, color);
        }
        Icon::Import => {
            line(&[(8.0, 1.5), (8.0, 10.0)]);
            line(&[(4.5, 6.5), (8.0, 10.0), (11.5, 6.5)]);
            line(&[(2.0, 10.5), (2.0, 14.0), (14.0, 14.0), (14.0, 10.5)]);
        }
        Icon::Gear => {
            p.circle_stroke(c, 2.4 * s, st);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([c + d * 4.6 * s, c + d * 6.6 * s], Stroke::new(2.0 * s, color));
            }
            p.circle_stroke(c, 4.8 * s, st);
        }
        Icon::Info => {
            p.circle_stroke(c, 6.5 * s, st);
            p.circle_filled(at(8.0, 4.8), 1.0 * s, color);
            line(&[(8.0, 7.0), (8.0, 11.8)]);
        }
        Icon::Filter => {
            line(&[(1.5, 3.0), (14.5, 3.0), (9.5, 8.5), (9.5, 13.5), (6.5, 12.0), (6.5, 8.5), (1.5, 3.0)]);
        }
        Icon::Search => {
            p.circle_stroke(at(6.8, 6.8), 4.6 * s, st);
            line(&[(10.2, 10.2), (14.0, 14.0)]);
        }
        Icon::Left => line(&[(10.5, 2.5), (5.0, 8.0), (10.5, 13.5)]),
        Icon::Right => line(&[(5.5, 2.5), (11.0, 8.0), (5.5, 13.5)]),
        Icon::Close => {
            line(&[(4.0, 4.0), (12.0, 12.0)]);
            line(&[(12.0, 4.0), (4.0, 12.0)]);
        }
        Icon::Check => line(&[(3.0, 8.5), (6.5, 12.0), (13.0, 4.5)]),
        Icon::Plus => {
            line(&[(8.0, 3.0), (8.0, 13.0)]);
            line(&[(3.0, 8.0), (13.0, 8.0)]);
        }
        Icon::Minus => line(&[(3.0, 8.0), (13.0, 8.0)]),
        Icon::Flag => {
            line(&[(4.0, 14.5), (4.0, 1.5)]);
            p.add(Shape::convex_polygon(
                vec![at(4.0, 2.0), at(13.0, 2.0), at(10.5, 5.0), at(13.0, 8.0), at(4.0, 8.0)],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Reject => {
            p.circle_stroke(c, 6.2 * s, st);
            line(&[(3.6, 12.4), (12.4, 3.6)]);
        }
        Icon::Stack => {
            p.rect_stroke(r(1.5, 5.5, 11.5, 14.5), 1.5 * s, st, StrokeKind::Middle);
            line(&[(4.0, 3.5), (13.5, 3.5), (13.5, 12.0)]);
            line(&[(6.5, 1.5), (14.5, 1.5)]);
        }
        Icon::Suspect => {
            p.circle_stroke(at(7.0, 7.0), 5.0 * s, st);
            line(&[(10.6, 10.6), (14.5, 14.5)]);
            p.text(at(7.0, 7.2), Align2::CENTER_CENTER, "B", bold(6.5 * s), color);
        }
        Icon::Keeper => {
            p.circle_stroke(at(7.0, 7.0), 5.0 * s, st);
            line(&[(10.6, 10.6), (14.5, 14.5)]);
            p.text(at(7.0, 7.2), Align2::CENTER_CENTER, "A", bold(6.5 * s), color);
        }
        Icon::Scene => {
            line(&[(1.5, 13.5), (6.0, 6.0), (9.0, 10.0), (11.0, 7.5), (14.5, 13.5), (1.5, 13.5)]);
            p.circle_filled(at(11.5, 3.5), 1.5 * s, color);
        }
        Icon::Album => {
            // Two pages of an open book.
            line(&[(8.0, 4.0), (8.0, 14.0)]);
            line(&[(8.0, 4.0), (5.0, 3.0), (1.5, 3.0), (1.5, 13.0), (5.0, 13.0), (8.0, 14.0)]);
            line(&[(8.0, 4.0), (11.0, 3.0), (14.5, 3.0), (14.5, 13.0), (11.0, 13.0), (8.0, 14.0)]);
        }
        Icon::Tag => {
            line(&[(2.0, 2.5), (8.5, 2.5), (14.0, 8.0), (8.0, 14.0), (2.0, 8.5), (2.0, 2.5)]);
            p.circle_filled(at(5.5, 6.0), 1.3 * s, color);
        }
    }
}

/// Five-pointed star.
pub fn star_shape(p: &egui::Painter, c: Pos2, r: f32, fill: Option<Color32>, stroke: Option<Stroke>) {
    let mut pts = Vec::with_capacity(10);
    for i in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        let rr = if i % 2 == 0 { r } else { r * 0.45 };
        pts.push(pos2(c.x + a.cos() * rr, c.y + a.sin() * rr + r * 0.08));
    }
    if let Some(f) = fill {
        // Non-convex: fan from the centre.
        for i in 0..10 {
            let a = pts[i];
            let b = pts[(i + 1) % 10];
            p.add(Shape::convex_polygon(vec![c + vec2(0.0, r * 0.08), a, b], f, Stroke::NONE));
        }
    }
    if let Some(s) = stroke {
        let mut closed = pts.clone();
        closed.push(pts[0]);
        p.add(Shape::line(closed, s));
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, active: bool, pal: &Palette, tooltip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 6.0, pal.control_sel);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, pal.hover);
    }
    let col = if active { pal.text } else { pal.text2 };
    paint_icon(p, Rect::from_center_size(rect.center(), Vec2::splat(16.0)), icon, col);
    if tooltip.is_empty() {
        resp
    } else {
        resp.on_hover_text(tooltip)
    }
}

/// macOS sidebar row.
pub fn sidebar_item(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    count: Option<usize>,
    selected: bool,
    pal: &Palette,
) -> Response {
    sidebar_item_indented(ui, icon, label, count, selected, pal, 0.0)
}

/// A sidebar row shifted right by `indent` (nested items such as the themes of a project).
pub fn sidebar_item_indented(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    count: Option<usize>,
    selected: bool,
    pal: &Palette,
    indent: f32,
) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 28.0), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, 6.0, pal.sidebar_sel);
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, pal.hover);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 18.0 + indent, rect.center().y), Vec2::splat(16.0));
    paint_icon(p, icon_rect, icon, if selected { pal.accent } else { pal.accent.gamma_multiply(0.9) });
    let mut right = rect.right() - 10.0;
    if let Some(n) = count {
        let g = p.layout_no_wrap(photoorg_core::format::count(n), font(11.5), pal.text2);
        p.galley(pos2(right - g.size().x, rect.center().y - g.size().y / 2.0), g.clone(), pal.text2);
        right -= g.size().x + 8.0;
    }
    let left = rect.left() + 34.0 + indent;
    let max_w = (right - left).max(10.0);
    let g = p.layout_no_wrap(label.to_owned(), font(13.0), pal.text);
    let clip = Rect::from_min_max(pos2(left, rect.top()), pos2(left + max_w, rect.bottom()));
    p.with_clip_rect(clip).galley(pos2(left, rect.center().y - g.size().y / 2.0), g.clone(), pal.text);
    if g.size().x > max_w {
        resp.on_hover_text(label)
    } else {
        resp
    }
}

pub fn section_title(ui: &mut Ui, text: &str, pal: &Palette) {
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    ui.painter().text(pos2(rect.left() + 8.0, rect.center().y), Align2::LEFT_CENTER, text, bold(11.0), pal.text2);
}

/// Segmented control. Returns true if the value changed.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)], pal: &Palette) -> bool {
    let seg_w: Vec<f32> = options
        .iter()
        .map(|(_, l)| ui.painter().layout_no_wrap(l.to_string(), font(12.5), pal.text).size().x + 24.0)
        .collect();
    let total: f32 = seg_w.iter().sum::<f32>() + 4.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 26.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 7.0, pal.control_bg);
    let mut x = rect.left() + 2.0;
    let mut changed = false;
    for (i, (v, label)) in options.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, rect.top() + 2.0), vec2(seg_w[i], rect.height() - 4.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", i, label)), Sense::click());
        let sel = *value == *v;
        if sel {
            p.rect_filled(r.expand(0.5), 5.5, pal.shadow.gamma_multiply(0.4));
            p.rect_filled(r, 5.0, pal.control_sel);
        } else if resp.hovered() {
            p.rect_filled(r, 5.0, pal.hover);
        }
        p.text(r.center(), Align2::CENTER_CENTER, *label, if sel { bold(12.5) } else { font(12.5) }, pal.text);
        if resp.clicked() && !sel {
            *value = *v;
            changed = true;
        }
        x += seg_w[i];
    }
    changed
}

/// Rounded "pill" button used for filter tokens.
pub fn pill(ui: &mut Ui, text: &str, active: bool, pal: &Palette) -> Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), font(12.5), pal.text);
    let w = g.size().x + 36.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 26.0), Sense::click());
    let p = ui.painter();
    let (bg, fg) = if active {
        (pal.accent, pal.accent_text)
    } else if resp.hovered() {
        (pal.control_sel, pal.text)
    } else {
        (pal.control_bg, pal.text)
    };
    p.rect_filled(rect, 13.0, bg);
    p.galley(pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0), g, fg);
    // chevron
    let cx = rect.right() - 13.0;
    let cy = rect.center().y;
    p.line_segment([pos2(cx - 3.0, cy - 1.5), pos2(cx, cy + 1.5)], Stroke::new(1.3, fg));
    p.line_segment([pos2(cx, cy + 1.5), pos2(cx + 3.0, cy - 1.5)], Stroke::new(1.3, fg));
    resp
}

pub fn primary_button(ui: &mut Ui, text: &str, pal: &Palette, enabled: bool) -> Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), bold(13.0), pal.accent_text);
    let (rect, resp) =
        ui.allocate_exact_size(vec2(g.size().x + 28.0, 28.0), if enabled { Sense::click() } else { Sense::hover() });
    let bg = if !enabled {
        pal.control_bg
    } else if resp.is_pointer_button_down_on() {
        pal.accent.gamma_multiply(0.8)
    } else if resp.hovered() {
        pal.accent.gamma_multiply(1.1)
    } else {
        pal.accent
    };
    ui.painter().rect_filled(rect, 7.0, bg);
    let fg = if enabled { pal.accent_text } else { pal.text3 };
    ui.painter().galley(rect.center() - g.size() / 2.0, g, fg);
    resp
}

pub fn secondary_button(ui: &mut Ui, text: &str, pal: &Palette) -> Response {
    let g = ui.painter().layout_no_wrap(text.to_owned(), font(13.0), pal.text);
    let (rect, resp) = ui.allocate_exact_size(vec2(g.size().x + 26.0, 28.0), Sense::click());
    let bg = if resp.hovered() { pal.control_sel } else { pal.control_bg };
    ui.painter().rect_filled(rect, 7.0, bg);
    ui.painter().galley(rect.center() - g.size() / 2.0, g, pal.text);
    resp
}

/// Interactive 5-star rating. Returns the new rating when clicked (clicking the current
/// rating clears it).
pub fn stars(ui: &mut Ui, rating: u8, size: f32, pal: &Palette) -> Option<u8> {
    let gap = size * 0.25;
    let (rect, resp) = ui.allocate_exact_size(vec2(size * 5.0 + gap * 4.0, size), Sense::click());
    let hover_n = resp.hover_pos().map(|p| (((p.x - rect.left()) / (size + gap)).floor() as i32 + 1).clamp(1, 5) as u8);
    let shown = hover_n.unwrap_or(rating);
    let painter = ui.painter();
    for i in 0..5u8 {
        let c = pos2(rect.left() + size / 2.0 + i as f32 * (size + gap), rect.center().y);
        if i < shown {
            let col = if hover_n.is_some() && i >= rating { pal.star.gamma_multiply(0.6) } else { pal.star };
            star_shape(painter, c, size * 0.5, Some(col), None);
        } else {
            star_shape(painter, c, size * 0.5, None, Some(Stroke::new(1.0, pal.text3)));
        }
    }
    if resp.clicked() {
        let n = hover_n.unwrap_or(0);
        return Some(if n == rating { 0 } else { n });
    }
    None
}

/// Paint small read-only stars (e.g. on thumbnails).
pub fn paint_stars(p: &egui::Painter, left_bottom: Pos2, rating: u8, size: f32, color: Color32, shadow: bool) {
    for i in 0..rating.min(5) {
        let c = pos2(left_bottom.x + size / 2.0 + i as f32 * (size * 1.1), left_bottom.y - size / 2.0);
        if shadow {
            star_shape(p, c + vec2(0.0, 0.8), size * 0.55, Some(Color32::from_black_alpha(140)), None);
        }
        star_shape(p, c, size * 0.5, Some(color), None);
    }
}

/// Apple-style switch.
pub fn switch(ui: &mut Ui, on: &mut bool, label: &str, pal: &Palette) -> Response {
    let g = ui.painter().layout_no_wrap(label.to_owned(), font(13.0), pal.text);
    let w = 36.0 + 8.0 + g.size().x;
    let (rect, mut resp) = ui.allocate_exact_size(vec2(w, 22.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);
    let track = Rect::from_min_size(pos2(rect.left(), rect.center().y - 10.0), vec2(36.0, 20.0));
    let p = ui.painter();
    p.rect_filled(track, 10.0, lerp_color(pal.control_bg, pal.accent, t));
    let knob_x = egui::lerp(track.left() + 10.0..=track.right() - 10.0, t);
    p.circle_filled(pos2(knob_x, track.center().y + 0.5), 8.5, Color32::from_black_alpha(50));
    p.circle_filled(pos2(knob_x, track.center().y), 8.0, Color32::WHITE);
    p.galley(pos2(track.right() + 8.0, rect.center().y - g.size().y / 2.0), g, pal.text);
    resp
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Small rounded badge with text; returns its rect.
pub fn badge(p: &egui::Painter, anchor: Pos2, align: Align2, text: &str, size: f32, bg: Color32, fg: Color32) -> Rect {
    let g = p.layout_no_wrap(text.to_owned(), bold(size), fg);
    let sz = g.size() + vec2(size * 0.9, size * 0.35);
    let rect = align.anchor_size(anchor, sz);
    p.rect_filled(rect, CornerRadius::same((sz.y / 2.0) as u8), bg);
    p.galley(rect.center() - g.size() / 2.0, g, fg);
    rect
}

/// Search field with a magnifier glyph. Returns the response of the text edit.
pub fn search_field(ui: &mut Ui, text: &mut String, width: f32, pal: &Palette, id: &str) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    ui.painter().rect_filled(rect, 7.0, pal.field_bg);
    paint_icon(
        ui.painter(),
        Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(13.0)),
        Icon::Search,
        pal.text2,
    );
    let inner =
        Rect::from_min_max(pos2(rect.left() + 26.0, rect.top() + 1.0), pos2(rect.right() - 22.0, rect.bottom() - 1.0));
    let mut child =
        ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let resp = child.add(
        egui::TextEdit::singleline(text)
            .id_salt(id)
            .frame(egui::Frame::NONE)
            .hint_text("검색")
            .font(font(13.0))
            .align(egui::Align2::LEFT_CENTER)
            .desired_width(inner.width()),
    );
    if !text.is_empty() {
        let xr = Rect::from_center_size(pos2(rect.right() - 12.0, rect.center().y), Vec2::splat(14.0));
        let xresp = ui.interact(xr, ui.scope_id().with((id, "clear")), Sense::click());
        ui.painter().circle_filled(xr.center(), 7.0, pal.text3);
        paint_icon(ui.painter(), xr.shrink(3.0), Icon::Close, pal.field_bg);
        if xresp.clicked() {
            text.clear();
        }
    }
    resp
}

pub fn separator_h(ui: &Ui, rect: Rect, pal: &Palette) {
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, pal.separator));
}

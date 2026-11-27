//! Marbeat look: dark violet theme, cards, pill tabs, buttons.
use eframe::egui::{self, Color32, FontId, Margin, RichText, Rounding, Stroke, Vec2};

pub const BG: Color32 = Color32::from_rgb(11, 11, 24);
pub const BG2: Color32 = Color32::from_rgb(16, 16, 34);
pub const CARD: Color32 = Color32::from_rgb(22, 22, 46);
pub const CARD2: Color32 = Color32::from_rgb(31, 31, 62);
pub const EDGE: Color32 = Color32::from_rgb(44, 44, 84);
pub const ACCENT: Color32 = Color32::from_rgb(124, 92, 255);
pub const ACCENT_HI: Color32 = Color32::from_rgb(150, 124, 255);
pub const MINT: Color32 = Color32::from_rgb(77, 255, 176);
pub const WARN: Color32 = Color32::from_rgb(255, 196, 77);
pub const BAD: Color32 = Color32::from_rgb(255, 106, 106);
pub const TEXT: Color32 = Color32::from_rgb(236, 236, 250);
pub const MUTED: Color32 = Color32::from_rgb(150, 150, 190);

pub fn theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = BG2;
    v.extreme_bg_color = Color32::from_rgb(8, 8, 18);
    v.faint_bg_color = CARD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = ACCENT_HI;
    let r = Rounding::same(9.0);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] { w.rounding = r; }
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, EDGE);
    v.widgets.inactive.bg_fill = CARD2;
    v.widgets.inactive.weak_bg_fill = CARD2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, EDGE);
    v.widgets.hovered.bg_fill = Color32::from_rgb(48, 44, 100);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(48, 44, 100);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.bg_fill = ACCENT;
    v.widgets.active.weak_bg_fill = ACCENT;
    v.window_rounding = Rounding::same(14.0);
    ctx.set_visuals(v);
    let mut st = (*ctx.style()).clone();
    st.spacing.item_spacing = Vec2::new(10.0, 9.0);
    st.spacing.button_padding = Vec2::new(14.0, 7.0);
    st.spacing.interact_size.y = 30.0;
    st.spacing.slider_width = 170.0;
    st.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
    st.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
    st.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(22.0));
    st.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
    ctx.set_style(st);
}

pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::none().fill(CARD).rounding(12.0).stroke(Stroke::new(1.0, EDGE)).inner_margin(Margin::same(14.0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if !title.is_empty() { ui.label(RichText::new(title.to_uppercase()).size(11.5).strong().color(MUTED)); ui.add_space(2.0); }
        add(ui)
    }).inner
}

pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).strong().color(Color32::WHITE)).fill(ACCENT).rounding(9.0).min_size(Vec2::new(ui.available_width().min(260.0), 36.0)))
}
pub fn primary_w(ui: &mut egui::Ui, text: &str, w: f32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).strong().color(Color32::WHITE)).fill(ACCENT).rounding(9.0).min_size(Vec2::new(w, 36.0)))
}
pub fn ghost(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(TEXT)).rounding(9.0).min_size(Vec2::new(0.0, 32.0)))
}
pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) { ui.label(RichText::new(text.into()).size(12.5).color(MUTED)); }
pub fn good(ui: &mut egui::Ui, text: impl Into<String>) { ui.label(RichText::new(text.into()).color(MINT)); }
pub fn row_label(ui: &mut egui::Ui, text: &str) { ui.label(RichText::new(text).color(MUTED)); }

/// pill tab, returns true when clicked
pub fn tab(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    let b = egui::Button::new(RichText::new(text).strong().color(if selected { Color32::WHITE } else { MUTED })).fill(if selected { ACCENT } else { Color32::TRANSPARENT }).stroke(Stroke::NONE).rounding(18.0).min_size(Vec2::new(0.0, 32.0));
    ui.add(b).clicked()
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none().fill(color.gamma_multiply(0.18)).stroke(Stroke::new(1.0, color.gamma_multiply(0.6))).rounding(10.0).inner_margin(Margin::symmetric(8.0, 2.0)).show(ui, |ui| { ui.label(RichText::new(text).size(12.0).color(color)); });
}

pub fn fmt_time(t: f64) -> String { let t = t.max(0.0) as u64; format!("{}:{:02}", t / 60, t % 60) }

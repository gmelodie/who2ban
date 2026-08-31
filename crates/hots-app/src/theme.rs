use eframe::egui::{self, Color32, CornerRadius, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x28, 0x28, 0x28);
pub const PANEL: Color32 = Color32::from_rgb(0x32, 0x30, 0x2f);
pub const RAISE: Color32 = Color32::from_rgb(0x3c, 0x38, 0x36);
pub const LINE: Color32 = Color32::from_rgb(0x50, 0x49, 0x45);
pub const TEXT: Color32 = Color32::from_rgb(0xeb, 0xdb, 0xb2);
pub const DIM: Color32 = Color32::from_rgb(0xa8, 0x99, 0x84);
pub const GREEN: Color32 = Color32::from_rgb(0xb8, 0xbb, 0x26);
pub const RED: Color32 = Color32::from_rgb(0xfb, 0x49, 0x34);
pub const YELLOW: Color32 = Color32::from_rgb(0xfa, 0xbd, 0x2f);
pub const BLUE: Color32 = Color32::from_rgb(0x83, 0xa5, 0x98);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(paint);
}

fn paint(style: &mut egui::Style) {
    let radius = CornerRadius::same(5);
    let visuals = &mut style.visuals;
    visuals.dark_mode = true;
    visuals.panel_fill = BG;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = Color32::from_rgb(0x1d, 0x20, 0x21);
    visuals.faint_bg_color = PANEL;
    visuals.code_bg_color = PANEL;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = BLUE;
    visuals.warn_fg_color = YELLOW;
    visuals.error_fg_color = RED;
    visuals.window_stroke = Stroke::new(1.0, LINE);
    visuals.window_corner_radius = radius;
    visuals.selection.bg_fill = BLUE.gamma_multiply(0.4);
    visuals.selection.stroke = Stroke::new(1.0, TEXT);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = radius;
        widget.bg_fill = RAISE;
        widget.weak_bg_fill = RAISE;
        widget.bg_stroke = Stroke::new(1.0, LINE);
        widget.fg_stroke = Stroke::new(1.0, TEXT);
    }
    visuals.widgets.noninteractive.bg_fill = PANEL;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, DIM);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, BLUE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, BLUE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, YELLOW);

    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 4.0);
}

/// The colour of a winrate, and grey until enough games make it mean anything.
pub fn winrate_color(games: u32, rate: f64, min_games: u32) -> Color32 {
    if games < min_games {
        return DIM;
    }
    if rate >= 0.6 {
        return GREEN;
    }
    if rate <= 0.4 {
        return RED;
    }
    YELLOW
}

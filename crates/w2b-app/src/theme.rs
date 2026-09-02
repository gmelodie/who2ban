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

/// The two colours the game gives the teams, taken off the draft screen's own banners:
/// your side is blue and theirs is red, and a card is quicker to place by colour than by
/// reading which heading it sits under.
pub const ALLY: Color32 = Color32::from_rgb(0x5a, 0x8f, 0xd0);
pub const ENEMY: Color32 = Color32::from_rgb(0xe0, 0x6c, 0x75);

/// Neither, for a lobby that does not say which side is yours.
pub const NEUTRAL: Color32 = LINE;

/// Which side of a lobby a player is on, as far as this client can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ally,
    Enemy,
    /// The configured battletag is not in the lobby, so no side is anybody's.
    Unknown,
}

impl Side {
    pub fn color(self) -> Color32 {
        match self {
            Side::Ally => ALLY,
            Side::Enemy => ENEMY,
            Side::Unknown => NEUTRAL,
        }
    }

    /// Faint enough to read a card against, strong enough to tell the two apart at a
    /// glance across a window of ten of them.
    pub fn wash(self) -> Color32 {
        match self {
            Side::Unknown => PANEL,
            side => tint(PANEL, side.color(), 0.14),
        }
    }
}

/// `base` moved `amount` of the way towards `with`.
pub fn tint(base: Color32, with: Color32, amount: f32) -> Color32 {
    let mix = |a: u8, b: u8| {
        (a as f32 * (1.0 - amount) + b as f32 * amount).round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(
        mix(base.r(), with.r()),
        mix(base.g(), with.g()),
        mix(base.b(), with.b()),
    )
}

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

    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.spacing.interact_size = egui::vec2(48.0, 28.0);
    style.spacing.text_edit_width = 420.0;

    for (text, font) in style.text_styles.iter_mut() {
        font.size = match text {
            egui::TextStyle::Small => 12.0,
            egui::TextStyle::Heading => 19.0,
            _ => 15.0,
        };
    }
}

/// egui gives a panel 8 points of margin, which reads as none against a window edge.
pub fn panel(style: &egui::Style) -> egui::Frame {
    egui::Frame::side_top_panel(style).inner_margin(egui::Margin::symmetric(18, 10))
}

pub fn central(style: &egui::Style) -> egui::Frame {
    egui::Frame::central_panel(style).inner_margin(egui::Margin::symmetric(18, 12))
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

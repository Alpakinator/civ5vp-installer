//! The palette, the typeface, and the widget styling.
//!
//! Every value here is generated from `docs/ui-reference/visual-language.md` — the prose is
//! the source, this file is the transcription. Change the prose first, then this.
//!
//! Presentation only: nothing in this module knows what any screen means.

use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};

// ── Navy grounds ─────────────────────────────────────────────────────────────

/// The window background.
pub const PAGE_NAVY: Color32 = Color32::from_rgb(0x0E, 0x1A, 0x28);
/// Fill of framed panels — one step up from the page.
pub const PANEL_NAVY: Color32 = Color32::from_rgb(0x15, 0x26, 0x38);
/// Interactive surfaces at rest: buttons, hovered rows.
pub const RAISED_NAVY: Color32 = Color32::from_rgb(0x1C, 0x33, 0x50);
/// Sunken troughs: text-entry fields, the progress trough.
pub const WELL_NAVY: Color32 = Color32::from_rgb(0x0A, 0x14, 0x20);

// ── Parchment inks ───────────────────────────────────────────────────────────

/// Body text — the default ink on navy.
pub const PARCHMENT: Color32 = Color32::from_rgb(0xE6, 0xD8, 0xB0);
/// Headings and button labels.
pub const PARCHMENT_BRIGHT: Color32 = Color32::from_rgb(0xF4, 0xEA, 0xCC);
/// Secondary text: quiet status lines, activity-log detail.
pub const PARCHMENT_DIM: Color32 = Color32::from_rgb(0xAC, 0x9F, 0x7C);

// ── Gold trim ────────────────────────────────────────────────────────────────

/// Emphasis lines: hovered borders, panel captions, progress highlights.
pub const GOLD_BRIGHT: Color32 = Color32::from_rgb(0xE7, 0xC5, 0x65);
/// The standard trim line: outer frames, progress fill, divider diamonds.
pub const GOLD: Color32 = Color32::from_rgb(0xC3, 0x96, 0x3C);
/// Recessed trim: inner frame lines, resting widget borders.
pub const GOLD_DARK: Color32 = Color32::from_rgb(0x7E, 0x62, 0x26);

// ── Outcome accents ──────────────────────────────────────────────────────────

/// Success — text and notice borders only, never large fills.
pub const LAUREL: Color32 = Color32::from_rgb(0x9D, 0xBB, 0x61);
/// Failure and refusal — the same restraint.
pub const EMBER: Color32 = Color32::from_rgb(0xC9, 0x6A, 0x45);

/// The type scale, as named in the visual language. `Display` is egui's `Heading`;
/// `Caption` is drawn ad hoc by the deco header, so it has no `TextStyle` of its own.
pub const CAPTION_SIZE: f32 = 17.0;

/// Install the skin on a context: the embedded Jost (ADR-0003) and the widget styling.
/// Idempotent, but rebuilding the font atlas is not free — call once per context.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // One skin, not one per system theme: the installer is night-blue and parchment
    // everywhere, so the OS light/dark preference is deliberately overridden.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style());
}

/// Jost for every piece of UI text (ADR-0003: an OFL-licensed geometric sans in the same
/// Futura lineage as the game's Tw Cen MT — embeddable and redistributable without a
/// licensing cloud; `assets/fonts/OFL.txt` accompanies it as the license requires).
/// It is put first for both families rather than replacing them, so anything the face
/// lacks — box-drawing, non-Latin — still falls back to egui's defaults instead of tofu.
fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "jost".to_owned(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Jost-400-Book.ttf"
        ))),
    );
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "jost".to_owned());
    }
    fonts
}

fn style() -> egui::Style {
    let mut style = egui::Style {
        // The type scale from the visual language. One face; hierarchy is size and colour.
        text_styles: [
            (TextStyle::Heading, FontId::proportional(26.0)),
            (TextStyle::Body, FontId::proportional(16.0)),
            (TextStyle::Button, FontId::proportional(17.0)),
            (TextStyle::Small, FontId::proportional(13.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
        ]
        .into(),
        visuals: visuals(),
        ..Default::default()
    };
    style.spacing.item_spacing = egui::vec2(8.0, 5.0);
    style.spacing.button_padding = egui::vec2(14.0, 5.0);
    style
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(PARCHMENT);
    visuals.panel_fill = PAGE_NAVY;
    visuals.window_fill = PAGE_NAVY;
    visuals.window_stroke = Stroke::new(1.0, GOLD_DARK);
    visuals.faint_bg_color = PANEL_NAVY;
    // Sunken troughs: text-entry fields and other wells.
    visuals.extreme_bg_color = WELL_NAVY;
    visuals.text_edit_bg_color = Some(WELL_NAVY);
    visuals.hyperlink_color = GOLD_BRIGHT;
    // Text selection: gold as trim, parchment stays the ink.
    visuals.selection.bg_fill = GOLD_DARK;
    visuals.selection.stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);

    // Native widgets take a 2 px corner radius — at widget size a small radius and a small
    // chamfer read the same, and the frame stays crisp (visual language, "Corner treatments").
    let corner = CornerRadius::same(2);

    // Labels, separators, and other things that only sit there.
    visuals.widgets.noninteractive.bg_fill = PANEL_NAVY;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL_NAVY;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, GOLD_DARK);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, PARCHMENT);
    visuals.widgets.noninteractive.corner_radius = corner;

    // At rest: raised navy, a quiet gold-dark border, bright parchment lettering.
    visuals.widgets.inactive.bg_fill = WELL_NAVY;
    visuals.widgets.inactive.weak_bg_fill = RAISED_NAVY;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, GOLD_DARK);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);
    visuals.widgets.inactive.corner_radius = corner;

    // Hover: the border lifts to bright gold at 2 px, the fill lightens one step.
    visuals.widgets.hovered.bg_fill = RAISED_NAVY;
    visuals.widgets.hovered.weak_bg_fill = RAISED_NAVY;
    visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, GOLD_BRIGHT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, PARCHMENT_BRIGHT);
    visuals.widgets.hovered.corner_radius = corner;

    // Pressed: the one moment gold is a fill, and the lettering flips to navy.
    visuals.widgets.active.bg_fill = GOLD;
    visuals.widgets.active.weak_bg_fill = GOLD;
    visuals.widgets.active.bg_stroke = Stroke::new(2.0, GOLD_BRIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5, PAGE_NAVY);
    visuals.widgets.active.corner_radius = corner;

    visuals.widgets.open.bg_fill = RAISED_NAVY;
    visuals.widgets.open.weak_bg_fill = RAISED_NAVY;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, GOLD);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);
    visuals.widgets.open.corner_radius = corner;

    visuals
}

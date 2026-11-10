//! The palette, the typeface, and the widget styling: the Vox Populi Civilopedia's Civ V style
//! (`web/src/civ5.css` there), carried over to egui.
//!
//! Every value here is generated from `docs/ui-reference/visual-language.md` - the prose is
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
/// Fill of framed panels - one step up from the page.
pub const PANEL_NAVY: Color32 = Color32::from_rgb(0x15, 0x26, 0x38);
/// Interactive surfaces at rest: buttons.
pub const RAISED_NAVY: Color32 = Color32::from_rgb(0x1C, 0x33, 0x50);
/// A button under the pointer, held down or open; a hovered row in a list.
pub const RAISED_HOVER: Color32 = Color32::from_rgb(0x23, 0x40, 0x68);
/// Sunken troughs: text-entry fields, the progress trough.
pub const WELL_NAVY: Color32 = Color32::from_rgb(0x0A, 0x14, 0x20);

/// How much of the page a panel hides: the panels, and the top bar, are see-through to the
/// game's painting behind the page (the Civilopedia owner's 92%, chosen in steps of one).
pub const PANEL_ALPHA: f32 = 0.92;

/// A panel's see-through fill.
pub fn panel_glass() -> Color32 {
    PANEL_NAVY.gamma_multiply(PANEL_ALPHA)
}

/// The top bar's see-through fill: page navy, as the Civilopedia's masthead is.
pub fn chrome_glass() -> Color32 {
    PAGE_NAVY.gamma_multiply(PANEL_ALPHA)
}

// ── Parchment inks ───────────────────────────────────────────────────────────

/// Body text - the default ink on navy.
pub const PARCHMENT: Color32 = Color32::from_rgb(0xE6, 0xD8, 0xB0);
/// Headings and button labels.
pub const PARCHMENT_BRIGHT: Color32 = Color32::from_rgb(0xF4, 0xEA, 0xCC);
/// Secondary text: quiet status lines, activity-log detail.
pub const PARCHMENT_DIM: Color32 = Color32::from_rgb(0xAC, 0x9F, 0x7C);

// ── Gold trim ────────────────────────────────────────────────────────────────

/// Emphasis lines: hovered borders, panel captions, progress highlights.
pub const GOLD_BRIGHT: Color32 = Color32::from_rgb(0xE7, 0xC5, 0x65);
/// The standard trim line: outer frames, progress fill, the lit span of a rule.
pub const GOLD: Color32 = Color32::from_rgb(0xC3, 0x96, 0x3C);
/// Recessed trim: inner frame lines, resting widget borders.
pub const GOLD_DARK: Color32 = Color32::from_rgb(0x7E, 0x62, 0x26);

/// A section heading's warm gold, brighter than the gold of the trim.
pub const HEADING: Color32 = Color32::from_rgb(0xF8, 0xE7, 0xB0);
/// The glow round a heading, as the game's titles have.
pub const HEADING_GLOW: Color32 = Color32::from_rgb(0xE7, 0xB4, 0x50);

// ── The rims ─────────────────────────────────────────────────────────────────
//
// A rim is three tones, top to bottom: lit from straight above (`deco::Tones`). Two strengths,
// so the eye lands on the content and on what is active rather than on a grid of equal
// outlines.

/// A panel's rim, and a button's at rest: its top, sides and bottom.
pub const RIM_TOP: Color32 = Color32::from_rgb(0xC2, 0xA6, 0x6B);
pub const RIM_SIDE: Color32 = Color32::from_rgb(0x8A, 0x6F, 0x42);
pub const RIM_BOTTOM: Color32 = Color32::from_rgb(0x5A, 0x44, 0x22);
/// The quiet rim, dimmer and lower in contrast: checkboxes, radio buttons, what can't be used.
pub const QUIET_TOP: Color32 = Color32::from_rgb(0x85, 0x73, 0x50);
pub const QUIET_SIDE: Color32 = Color32::from_rgb(0x64, 0x55, 0x3A);
pub const QUIET_BOTTOM: Color32 = Color32::from_rgb(0x46, 0x3A, 0x28);
/// Hovered, pressed or open: the top tone brightens.
pub const LIT_TOP: Color32 = Color32::from_rgb(0xD9, 0xBF, 0x84);
/// Keyboard focus: a brighter rim instead of an outline.
pub const FOCUS_TOP: Color32 = Color32::from_rgb(0xEA, 0xD2, 0x9A);
pub const FOCUS_SIDE: Color32 = Color32::from_rgb(0xB4, 0x95, 0x5A);

// ── Outcome accents ──────────────────────────────────────────────────────────

/// Success - text and notice borders only, never large fills.
pub const LAUREL: Color32 = Color32::from_rgb(0x9D, 0xBB, 0x61);
/// Failure and refusal - the same restraint.
pub const EMBER: Color32 = Color32::from_rgb(0xC9, 0x6A, 0x45);

/// The type scale, as named in the visual language. `Display` is egui's `Heading`; `Caption`,
/// a section heading, is drawn by `deco`, so it has no `TextStyle` of its own.
pub const CAPTION_SIZE: f32 = 19.0;
/// The primary button's label.
pub const BUTTON_LARGE_SIZE: f32 = 17.0;

/// Install the skin on a context: the embedded Jost (ADR-0003) and the widget styling.
/// Idempotent, but rebuilding the font atlas is not free - call once per context.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    // One skin, not one per system theme: the installer is night-blue and parchment
    // everywhere, so the OS light/dark preference is deliberately overridden.
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style());
}

/// The VP logo, decoded for the window: the title bar, the taskbar and the Alt-Tab list.
/// The same artwork is embedded into the Windows executable itself by `build.rs`, so
/// Explorer shows it before the process ever runs.
///
/// Best-effort by construction: a window without an icon is a cosmetic loss, never a
/// reason to fail startup, so an undecodable asset simply yields `None`.
pub fn window_icon() -> Option<egui::IconData> {
    let logo = image::load_from_memory_with_format(
        include_bytes!("../../../assets/icon/VP_logo.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();
    let (width, height) = logo.dimensions();
    Some(egui::IconData {
        rgba: logo.into_raw(),
        width,
        height,
    })
}

/// Jost for every piece of UI text (ADR-0003: an OFL-licensed geometric sans in the same
/// Futura lineage as the game's Tw Cen MT - embeddable and redistributable without a
/// licensing cloud; `assets/fonts/OFL.txt` accompanies it as the license requires).
/// It is put first for both families rather than replacing them, so anything the face
/// lacks - box-drawing, non-Latin - still falls back to egui's defaults instead of tofu.
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
    style.spacing.scroll = scroll();
    style
}

/// The scroll bar, centred in the page margin that `deco::page` leaves it.
///
/// egui right-aligns the bar against the edge of the scrolling area, so the outer margin is
/// what centres it: half the page margin, less half the bar. The width is held constant -
/// the default bar swells when the pointer is near it, which would slide a centred bar
/// sideways every time it was touched.
fn scroll() -> egui::style::ScrollStyle {
    let bar_width = 8.0;
    egui::style::ScrollStyle {
        bar_width,
        floating_width: bar_width,
        bar_outer_margin: (f32::from(crate::deco::PAGE_MARGIN) - bar_width) / 2.0,
        ..egui::style::ScrollStyle::floating()
    }
}

fn visuals() -> egui::Visuals {
    let mut visuals = egui::Visuals::dark();

    visuals.override_text_color = Some(PARCHMENT);
    visuals.panel_fill = PAGE_NAVY;
    visuals.faint_bg_color = PANEL_NAVY;
    // Sunken troughs: text-entry fields and other wells.
    visuals.extreme_bg_color = WELL_NAVY;
    visuals.text_edit_bg_color = Some(WELL_NAVY);
    visuals.hyperlink_color = GOLD_BRIGHT;
    // Text selection: gold, parchment stays the ink.
    visuals.selection.bg_fill = GOLD_DARK;
    visuals.selection.stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);

    // Every box is rounded to the same radius, the Civilopedia's 6 px. The shell paints its own
    // widgets' boxes (`deco::Plate`), for the three-tone rim; this is the radius egui uses for
    // the boxes it draws itself - a dropdown's rows, the file browser's buttons.
    let corner = CornerRadius::same(6);

    // Labels, separators, and other things that only sit there.
    visuals.widgets.noninteractive.bg_fill = PANEL_NAVY;
    visuals.widgets.noninteractive.weak_bg_fill = PANEL_NAVY;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, RIM_SIDE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, PARCHMENT);
    visuals.widgets.noninteractive.corner_radius = corner;

    // At rest: raised navy, the quiet rim, bright parchment lettering.
    visuals.widgets.inactive.bg_fill = WELL_NAVY;
    visuals.widgets.inactive.weak_bg_fill = RAISED_NAVY;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, QUIET_SIDE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);
    visuals.widgets.inactive.corner_radius = corner;

    // Hover: the fill lightens one step and the rim takes its lit tone.
    //
    // Every state's border is the same width, 1 px: egui sizes a button's padding as
    // `button_padding` less the border width of the state it is in, so a wider border on hover
    // would shrink the button by a pixel a side and move its label every time the pointer
    // crossed it.
    visuals.widgets.hovered.bg_fill = RAISED_HOVER;
    visuals.widgets.hovered.weak_bg_fill = RAISED_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, LIT_TOP);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.5, PARCHMENT_BRIGHT);
    visuals.widgets.hovered.corner_radius = corner;

    // Pressed looks as hovered, as on the Civilopedia.
    visuals.widgets.active = visuals.widgets.hovered;

    visuals.widgets.open.bg_fill = RAISED_HOVER;
    visuals.widgets.open.weak_bg_fill = RAISED_HOVER;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0, LIT_TOP);
    visuals.widgets.open.fg_stroke = Stroke::new(1.0, PARCHMENT_BRIGHT);
    visuals.widgets.open.corner_radius = corner;

    // The frames egui draws for itself: windows (the file browser is one), dropdown lists and
    // tooltips. Opaque panel navy, as what floats over the page is on the Civilopedia, in the
    // rim's side tone. Their shadows fall down and to the right, the way the game's do.
    visuals.window_fill = PANEL_NAVY;
    visuals.window_stroke = Stroke::new(2.0, RIM_SIDE);
    visuals.window_corner_radius = corner;
    visuals.menu_corner_radius = corner;
    let shadow = Color32::from_rgba_unmultiplied(0x0A, 0x14, 0x20, 0xB3);
    visuals.window_shadow = egui::Shadow {
        offset: [12, 12],
        blur: 16,
        spread: 0,
        color: shadow,
    };
    visuals.popup_shadow = egui::Shadow {
        offset: [6, 6],
        blur: 5,
        spread: 0,
        color: shadow,
    };

    visuals
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window icon is decoded from an embedded asset, so a bad or missing file is a
    /// build-time fact, not something a user should discover as a blank title bar.
    #[test]
    fn the_window_icon_decodes() {
        let Some(icon) = window_icon() else {
            unreachable!("the embedded VP logo decodes")
        };
        assert_eq!(icon.width, icon.height, "the logo is square");
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
    }
}

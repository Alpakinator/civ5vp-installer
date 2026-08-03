//! The painted art-deco components: page, header, panels, divider, progress.
//!
//! Everything here is drawn from the recipes in `docs/ui-reference/visual-language.md` —
//! the four motifs (stepped frame, sunburst fan, diamond rule, hatched fill), the line
//! weights, and the chamfered corners. All of it is original artwork painted in code; no
//! game asset is embedded or traced (ADR-0003).
//!
//! Presentation only (rule 3): these functions lay out and paint, and decide nothing.

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::theme;

/// Page margin on every side of the window (visual language, "Proportions").
const PAGE_MARGIN: i8 = 20;
/// Internal padding of a framed panel.
const PANEL_PADDING: i8 = 10;
/// Chamfer of a panel's outer line; the inner line follows at its inset with a smaller cut.
const PANEL_CHAMFER: f32 = 8.0;
/// How far the inner frame line sits inside the outer one.
const FRAME_INSET: f32 = 3.0;

/// The window surface: page-navy ground, page margin all round.
pub fn page(style: &egui::Style) -> egui::Frame {
    egui::Frame::central_panel(style)
        .fill(theme::PAGE_NAVY)
        .inner_margin(egui::Margin::same(PAGE_MARGIN))
}

/// The eight corners of a rectangle with its corners cut at 45°.
fn chamfered(rect: Rect, cut: f32) -> Vec<Pos2> {
    vec![
        pos2(rect.left() + cut, rect.top()),
        pos2(rect.right() - cut, rect.top()),
        pos2(rect.right(), rect.top() + cut),
        pos2(rect.right(), rect.bottom() - cut),
        pos2(rect.right() - cut, rect.bottom()),
        pos2(rect.left() + cut, rect.bottom()),
        pos2(rect.left(), rect.bottom() - cut),
        pos2(rect.left(), rect.top() + cut),
    ]
}

/// The stepped frame: a filled chamfered octagon, a 1 px gold outer line on its edge, and a
/// 1 px gold-dark inner line inset 3 px, stepping around each corner in parallel cuts.
fn stepped_frame(rect: Rect, fill: Color32, outer_line: Color32) -> Shape {
    let outer = chamfered(rect, PANEL_CHAMFER);
    let inner = chamfered(rect.shrink(FRAME_INSET), PANEL_CHAMFER - 2.0);
    Shape::Vec(vec![
        Shape::convex_polygon(outer, fill, Stroke::new(1.0, outer_line)),
        Shape::closed_line(inner, Stroke::new(1.0, theme::GOLD_DARK)),
    ])
}

/// A framed panel: panel-navy ground under the content, stepped frame around it, an
/// optional caption in gold at the top. Spans the full content width.
pub fn panel<R>(
    ui: &mut egui::Ui,
    caption: Option<&str>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    framed(ui, theme::PANEL_NAVY, theme::GOLD, caption, contents)
}

/// The notice variant of the panel: the border takes an outcome accent and the ground warms
/// very slightly toward it; the text stays parchment — the border carries the tone.
pub fn notice<R>(
    ui: &mut egui::Ui,
    accent: Color32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let warmed = Color32::from_rgb(
        theme::PANEL_NAVY.r().saturating_add(accent.r() / 24),
        theme::PANEL_NAVY.g().saturating_add(accent.g() / 24),
        theme::PANEL_NAVY.b().saturating_add(accent.b() / 24),
    );
    framed(ui, warmed, accent, None, contents)
}

fn framed<R>(
    ui: &mut egui::Ui,
    fill: Color32,
    outer_line: Color32,
    caption: Option<&str>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    // The ground has to be painted before the content but sized after it, so a placeholder
    // shape is reserved now and replaced once the frame knows its rectangle.
    let ground = ui.painter().add(Shape::Noop);
    let inner = egui::Frame::new()
        .inner_margin(egui::Margin::same(PANEL_PADDING))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            if let Some(caption) = caption {
                ui.label(
                    egui::RichText::new(caption)
                        .size(theme::CAPTION_SIZE)
                        .color(theme::GOLD_BRIGHT),
                );
                ui.add_space(4.0);
            }
            contents(ui)
        });
    ui.painter()
        .set(ground, stepped_frame(inner.response.rect, fill, outer_line));
    inner.inner
}

/// The header: the display title centred, the sunburst fan behind it, the diamond rule
/// below. Rendered once per window.
pub fn header(ui: &mut egui::Ui, title: &str) {
    let fan = ui.painter().add(Shape::Noop);
    let laid_out = ui.vertical_centered(|ui| {
        ui.add_space(3.0);
        ui.heading(egui::RichText::new(title).color(theme::PARCHMENT_BRIGHT));
        ui.add_space(3.0);
    });
    ui.painter().set(fan, sunburst(laid_out.response.rect));
    diamond_rule(ui);
    ui.add_space(4.0);
}

/// The sunburst fan: straight rays fanning symmetrically from a low centre point — 1.5 px
/// gold at low opacity, one ray every 10° across ≈ 150°, inner radius 14, outer radius 48.
/// Ornament behind text; it must never fight the lettering.
fn sunburst(header_rect: Rect) -> Shape {
    let centre = pos2(header_rect.center().x, header_rect.bottom() - 4.0);
    let ray = Stroke::new(1.5, theme::GOLD.gamma_multiply(0.22));
    let mut rays = Vec::new();
    let mut degrees = 15.0_f32;
    while degrees <= 165.0 {
        let direction = vec2(
            degrees.to_radians().cos(),
            // Screen y grows downward; the fan opens upward.
            -degrees.to_radians().sin(),
        );
        rays.push(Shape::line_segment(
            [centre + direction * 14.0, centre + direction * 48.0],
            ray,
        ));
        degrees += 10.0;
    }
    Shape::Vec(rays)
}

/// The diamond rule: a gold-dark hairline across the full width, over-struck for its
/// central stretch by a gold line, with a small solid gold diamond at the exact centre.
pub fn diamond_rule(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 9.0), egui::Sense::hover());
    let painter = ui.painter();
    let y = rect.center().y;

    painter.hline(rect.x_range(), y, Stroke::new(1.0, theme::GOLD_DARK));
    let lit = 80.0_f32.min(width / 2.0);
    painter.hline(
        egui::Rangef::new(rect.center().x - lit, rect.center().x + lit),
        y,
        Stroke::new(1.0, theme::GOLD),
    );

    let c = rect.center();
    let half = 4.0;
    painter.add(Shape::convex_polygon(
        vec![
            pos2(c.x, y - half),
            pos2(c.x + half, y),
            pos2(c.x, y + half),
            pos2(c.x - half, y),
        ],
        theme::GOLD,
        Stroke::NONE,
    ));
}

/// The progress bar: a well-navy trough with its own stepped frame. `Some(fraction)` draws
/// the gold fill from the left with a bright highlight along its top edge; `None` draws the
/// hatched-fill motif across the whole trough — working, but with nothing to measure.
/// Never animated, so a still render is a faithful picture of it.
pub fn progress(ui: &mut egui::Ui, fraction: Option<f32>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), egui::Sense::hover());
    let painter = ui.painter();

    painter.add(Shape::convex_polygon(
        chamfered(rect, 3.0),
        theme::WELL_NAVY,
        Stroke::new(1.0, theme::GOLD_DARK),
    ));

    let trough = rect.shrink(FRAME_INSET);
    match fraction {
        Some(fraction) => {
            let filled = fraction.clamp(0.0, 1.0) * trough.width();
            if filled >= 1.0 {
                let fill = Rect::from_min_size(trough.min, vec2(filled, trough.height()));
                painter.rect_filled(fill, 0.0, theme::GOLD);
                painter.hline(
                    fill.x_range(),
                    fill.top() + 0.5,
                    Stroke::new(1.0, theme::GOLD_BRIGHT),
                );
            }
        }
        None => {
            // 45° gold stripes, 3 px wide and 9 px apart, clipped to the trough.
            let hatch = painter.with_clip_rect(trough);
            let stripe = Stroke::new(3.0, theme::GOLD.gamma_multiply(0.55));
            let mut x = trough.left() - trough.height();
            while x < trough.right() {
                hatch.line_segment(
                    [
                        pos2(x, trough.bottom()),
                        pos2(x + trough.height(), trough.top()),
                    ],
                    stripe,
                );
                x += 9.0;
            }
        }
    }
}

/// The primary action: minimum 150 × 34 px, label centred, styling from the theme's widget
/// visuals. A plain egui button underneath, so AccessKit still sees a button with a label.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::CAPTION_SIZE)
                .color(if enabled {
                    theme::PARCHMENT_BRIGHT
                } else {
                    theme::PARCHMENT_DIM
                }),
        )
        .min_size(vec2(150.0, 30.0))
        .stroke(Stroke::new(
            1.0,
            if enabled {
                theme::GOLD
            } else {
                theme::GOLD_DARK
            },
        )),
    )
}

/// A separator drawn inside a panel: a single recessed hairline.
pub fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 5.0), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0, theme::GOLD_DARK),
    );
}

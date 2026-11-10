//! The painted components: backdrop, top bar, panels, headings, rules, progress, and the
//! widgets egui would otherwise draw for itself.
//!
//! The look is the Vox Populi Civilopedia's Civ V style (`docs/ui-reference/visual-language.md`):
//! rounded boxes in a 2 px gold rim lit from straight above, see-through navy panels over the
//! game's own main-menu painting, a corner fan in each panel's top corners, and section
//! headings that glow warm gold with a rule running on from the name. The painting and the
//! fans are the game's, read from the player's installation at runtime (`game_art`,
//! ADR-0012); everything else here is painted in code.
//!
//! Presentation only: these functions lay out and paint, and decide nothing.

use std::sync::Arc;

use egui::emath::GuiRounding as _;
use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::theme;

/// Page margin either side of the window, and below the last panel.
pub const PAGE_MARGIN: i8 = 20;
/// Internal padding of a framed panel.
const PANEL_PADDING: i8 = 10;
/// How wide a rim is.
pub const RIM: f32 = 2.0;
/// How far down a rim passes from its top tone to its side tone, and up from its bottom tone.
pub const RIM_PASS: f32 = 10.0;
/// A corner fan's size, from the rim's outer corner.
pub const FAN: f32 = 24.0;
/// The corner radius of every box: panels, buttons, fields.
const RADIUS: f32 = 6.0;
/// A checkbox's corner radius: its box is a third the height of a button's.
const CHECKBOX_RADIUS: f32 = 3.0;
/// The top bar's height, its rule included.
pub const TOP_BAR: f32 = 46.0;
/// The gap between the top bar and the first thing on the page.
pub const BELOW_TOP_BAR: f32 = 10.0;

// ── The game's art ───────────────────────────────────────────────────────────

/// The game's pictures, uploaded as textures, which the painting below reaches for.
#[derive(Clone)]
struct Art {
    backdrop: Option<egui::TextureHandle>,
    fans: Option<[egui::TextureHandle; 2]>,
}

fn art_id() -> egui::Id {
    egui::Id::new("civ5vp-game-art")
}

/// Hand the skin the game's pictures. Kept on the context, so every painter below finds them
/// without each screen having to pass them down.
pub fn install_art(ctx: &egui::Context, art: crate::game_art::GameArt) {
    let backdrop = art
        .backdrop
        .map(|picture| ctx.load_texture("backdrop", picture, egui::TextureOptions::LINEAR));
    // Drawn at a quarter of their size and less: without mipmaps a fan sparkles.
    let small = egui::TextureOptions {
        mipmap_mode: Some(egui::TextureFilter::Linear),
        ..egui::TextureOptions::LINEAR
    };
    let fans = art.fans.map(|[left, right]| {
        [
            ctx.load_texture("fan-left", left, small),
            ctx.load_texture("fan-right", right, small),
        ]
    });
    ctx.data_mut(|data| data.insert_temp(art_id(), Art { backdrop, fans }));
}

/// Go without the game's pictures: the game folder is gone or was never found.
pub fn remove_art(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<Art>(art_id()));
}

fn art(ctx: &egui::Context) -> Option<Art> {
    ctx.data(|data| data.get_temp::<Art>(art_id()))
}

/// Whether the game's painting is behind the page. While it is, text set straight on the page
/// needs its shadow to stay readable over the painting's light patches.
fn has_backdrop(ctx: &egui::Context) -> bool {
    art(ctx).is_some_and(|art| art.backdrop.is_some())
}

/// The window's ground: page navy, and over it the game's main-menu painting, covering the
/// window as the Civilopedia's backdrop does - scaled to fill it, centred, cropped at the
/// edges, and fixed to the window while the page scrolls over it.
pub fn backdrop(ui: &egui::Ui) {
    let window = ui.ctx().content_rect();
    let painter = ui.ctx().layer_painter(egui::LayerId::background());
    painter.rect_filled(window, 0.0, theme::PAGE_NAVY);
    let Some(texture) = art(ui.ctx()).and_then(|art| art.backdrop) else {
        return;
    };
    let size = texture.size_vec2();
    let scale = (window.width() / size.x).max(window.height() / size.y);
    painter.image(
        texture.id(),
        Rect::from_center_size(window.center(), size * scale),
        Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
        Color32::WHITE,
    );
}

/// A note laid straight on the page, set on a small plate of see-through black so it reads over
/// any part of the painting: Parchment bright, the plate shrink-wrapped to the text with a
/// little air, its corners rounded as every box is.
pub fn page_note(ui: &mut egui::Ui, text: egui::RichText) -> egui::Response {
    let spoken = text.text().to_owned();
    let galley = egui::WidgetText::from(text.color(theme::PARCHMENT_BRIGHT)).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        ui.available_width() - 2.0 * NOTE_PADDING.x,
        egui::TextStyle::Body,
    );
    let (rect, response) =
        ui.allocate_exact_size(galley.size() + 2.0 * NOTE_PADDING, egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, 4.0, Color32::from_black_alpha(NOTE_PLATE_ALPHA));
        painter.galley(rect.min + NOTE_PADDING, galley, theme::PARCHMENT_BRIGHT);
    }
    response
}

/// The air between a page note and the edge of its plate.
const NOTE_PADDING: egui::Vec2 = vec2(8.0, 3.0);
/// How much of the painting a page note's plate hides.
const NOTE_PLATE_ALPHA: u8 = 150;

/// The page under the top bar: no fill of its own, as the backdrop is the ground.
///
/// No margin on either side: both are held *inside* the scrolling area instead, as its content
/// margin. egui hands a wheel event to a scroll area only while the pointer is within that
/// area's own rectangle, so a side margin left out here is a strip of the window where the wheel
/// does nothing - which is how the left edge behaved until 0.1.5. None at the top either: the
/// top bar runs to the window's top edge.
pub fn page(style: &egui::Style) -> egui::Frame {
    egui::Frame::central_panel(style)
        .fill(Color32::TRANSPARENT)
        .inner_margin(egui::Margin {
            bottom: PAGE_MARGIN,
            ..egui::Margin::ZERO
        })
}

// ── The rim ──────────────────────────────────────────────────────────────────

/// A rim's three tones, top to bottom. It holds its top tone along the top, passes into the
/// side tone over [`RIM_PASS`] down the sides, and into the bottom tone along the bottom: lit
/// from straight above, as the game lights its frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tones {
    pub top: Color32,
    pub side: Color32,
    pub bottom: Color32,
}

impl Tones {
    /// A panel's rim, and a widget's at rest.
    pub const PANEL: Self = Self {
        top: theme::RIM_TOP,
        side: theme::RIM_SIDE,
        bottom: theme::RIM_BOTTOM,
    };
    /// The quieter rim: dimmer and lower in contrast, for the small marks - checkboxes, radio
    /// buttons - and for anything that cannot be used.
    pub const QUIET: Self = Self {
        top: theme::QUIET_TOP,
        side: theme::QUIET_SIDE,
        bottom: theme::QUIET_BOTTOM,
    };
    /// Hovered, pressed or open: the top tone brightens.
    pub const LIT: Self = Self {
        top: theme::LIT_TOP,
        ..Self::PANEL
    };
    /// Keyboard focus: a brighter rim, in the box's own shape, instead of an outline.
    pub const FOCUS: Self = Self {
        top: theme::FOCUS_TOP,
        side: theme::FOCUS_SIDE,
        bottom: theme::RIM_BOTTOM,
    };

    /// A rim in an outcome's colour, for a notice: its accent down the sides, a little
    /// brighter along the top, deeper along the bottom.
    fn accent(accent: Color32) -> Self {
        Self {
            top: lerp(accent, theme::PARCHMENT_BRIGHT, 0.25),
            side: accent,
            bottom: lerp(accent, Color32::BLACK, 0.45),
        }
    }

    /// The tone at height `y` of a rim running from `top` to `bottom`.
    fn at(self, y: f32, top: f32, bottom: f32) -> Color32 {
        let pass = RIM_PASS.min((bottom - top) / 2.0).max(f32::EPSILON);
        if y - top < pass {
            lerp(self.top, self.side, ((y - top) / pass).clamp(0.0, 1.0))
        } else if bottom - y < pass {
            lerp(
                self.bottom,
                self.side,
                ((bottom - y) / pass).clamp(0.0, 1.0),
            )
        } else {
            self.side
        }
    }
}

/// A box's state, as its rim and fill answer it.
fn tones_for(response: &egui::Response) -> Tones {
    if !response.enabled() {
        Tones::QUIET
    } else if response.has_focus() && !response.hovered() {
        Tones::FOCUS
    } else if response.hovered() || response.is_pointer_button_down_on() || response.has_focus() {
        Tones::LIT
    } else {
        Tones::PANEL
    }
}

fn lerp(from: Color32, to: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
        mix(from.a(), to.a()),
    )
}

/// The outline of a rectangle with its corners rounded to `radius`, clockwise from the top
/// of its left side, as a polygon fine enough that no facet shows at `pixels_per_point`.
fn rounded(rect: Rect, radius: f32, pixels_per_point: f32) -> Vec<Pos2> {
    let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if radius <= 0.0 {
        return vec![
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
    }
    let steps = ((radius * pixels_per_point).ceil() as usize).clamp(3, 24);
    let corners = [
        (pos2(rect.left() + radius, rect.top() + radius), 180.0_f32),
        (pos2(rect.right() - radius, rect.top() + radius), 270.0),
        (pos2(rect.right() - radius, rect.bottom() - radius), 0.0),
        (pos2(rect.left() + radius, rect.bottom() - radius), 90.0),
    ];
    let mut points: Vec<Pos2> = Vec::with_capacity(4 * (steps + 1));
    for (centre, start) in corners {
        for step in 0..=steps {
            let angle = (start + 90.0 * step as f32 / steps as f32).to_radians();
            let point = centre + radius * vec2(angle.cos(), angle.sin());
            if points.last().is_none_or(|last| last.distance(point) > 1e-3) {
                points.push(point);
            }
        }
    }
    if points.len() > 1 && points[0].distance(points[points.len() - 1]) <= 1e-3 {
        points.pop();
    }
    points
}

/// The six corners of a pointy-top hexagon - the game's map tile - with `radius` from the
/// centre to its top and bottom points.
fn hexagon(centre: Pos2, radius: f32) -> Vec<Pos2> {
    let side = radius * HEX_WIDTH_RATIO;
    vec![
        pos2(centre.x, centre.y - radius),
        pos2(centre.x + side, centre.y - radius / 2.0),
        pos2(centre.x + side, centre.y + radius / 2.0),
        pos2(centre.x, centre.y + radius),
        pos2(centre.x - side, centre.y + radius / 2.0),
        pos2(centre.x - side, centre.y - radius / 2.0),
    ]
}

/// Width of a pointy-top hexagon over its height: √3 / 2.
const HEX_WIDTH_RATIO: f32 = 0.866_025_4;

/// A closed outline with a vertex added wherever it crosses `y`, so a colour pinned to the
/// vertices changes there and not somewhere between two far-apart corners.
fn split_at(outline: &[Pos2], y: f32) -> Vec<Pos2> {
    let mut split = Vec::with_capacity(outline.len() + 2);
    for (i, &a) in outline.iter().enumerate() {
        split.push(a);
        let b = outline[(i + 1) % outline.len()];
        if (a.y - y) * (b.y - y) < 0.0 {
            let t = (y - a.y) / (b.y - a.y);
            split.push(a + (b - a) * t);
        }
    }
    split
}

/// Every stretch of a closed path with the unit normal pointing away from `inside`.
fn normals(points: &[Pos2], inside: Pos2) -> Vec<egui::Vec2> {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            let direction = (b - a).normalized();
            let normal = vec2(direction.y, -direction.x);
            if normal.dot(a - inside) < 0.0 {
                -normal
            } else {
                normal
            }
        })
        .collect()
}

/// A closed path with every stretch moved `distance` towards `inside`, each corner where its
/// two moved stretches meet - so the new path runs parallel to the old at the same distance
/// everywhere. A negative distance moves it out.
fn offset_path(points: &[Pos2], inside: Pos2, distance: f32) -> Vec<Pos2> {
    let count = points.len();
    let moved: Vec<(Pos2, egui::Vec2)> = normals(points, inside)
        .into_iter()
        .enumerate()
        .map(|(i, normal)| {
            let (a, b) = (points[i], points[(i + 1) % count]);
            (a - normal * distance, (b - a).normalized())
        })
        .collect();
    (0..count)
        .map(|i| {
            let ((p, d), (q, e)) = (moved[(i + count - 1) % count], moved[i]);
            let cross = d.x * e.y - d.y * e.x;
            if cross.abs() < 1e-6 {
                return q;
            }
            let t = ((q.x - p.x) * e.y - (q.y - p.y) * e.x) / cross;
            p + d * t
        })
        .collect()
}

/// The centre of a polygon's corners: inside any convex one.
fn centroid(points: &[Pos2]) -> Pos2 {
    let sum = points
        .iter()
        .fold(egui::Vec2::ZERO, |sum, point| sum + point.to_vec2());
    (sum / points.len().max(1) as f32).to_pos2()
}

/// The rim: a 2 px band just inside `outline`, each point of it in the tone [`Tones::at`] gives
/// its height. Built as a mesh with a one-pixel feather either side, so its edges are as soft
/// as egui's own anti-aliasing and no softer.
fn rim(outline: &[Pos2], tones: Tones, pixel: f32) -> Shape {
    let (top, bottom) = outline
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let pass = RIM_PASS.min((bottom - top) / 2.0);
    let outline = split_at(&split_at(outline, top + pass), bottom - pass);
    let inside = centroid(&outline);

    let soft = pixel / 2.0;
    let depths = [-soft, soft, RIM - soft, RIM + soft];
    let opaque = [false, true, true, false];
    let rows: Vec<Vec<Pos2>> = depths
        .iter()
        .map(|depth| offset_path(&outline, inside, *depth))
        .collect();

    let mut mesh = egui::Mesh::default();
    let count = outline.len() as u32;
    for (row, solid) in rows.iter().zip(opaque) {
        for point in row {
            let colour = if solid {
                tones.at(point.y, top, bottom)
            } else {
                Color32::TRANSPARENT
            };
            mesh.colored_vertex(*point, colour);
        }
    }
    for band in 0..rows.len() as u32 - 1 {
        for i in 0..count {
            let next = (i + 1) % count;
            let (a, b) = (band * count + i, band * count + next);
            let (c, d) = (a + count, b + count);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    Shape::mesh(mesh)
}

/// A filled shape inside its rim: the fill runs to the rim's inner edge and no further, so a
/// see-through fill never darkens the rim's tones.
fn filled(outline: &[Pos2], fill: Color32) -> Shape {
    Shape::convex_polygon(
        offset_path(outline, centroid(outline), RIM - 0.5),
        fill,
        Stroke::NONE,
    )
}

/// One physical pixel, in points.
fn pixel_of(ui: &egui::Ui) -> f32 {
    1.0 / ui.ctx().pixels_per_point()
}

/// A box: `fill` inside a rim of `tones`, corners rounded to `radius`.
fn rimmed_box(ui: &egui::Ui, rect: Rect, radius: f32, fill: Color32, tones: Tones) -> Shape {
    let rect = rect.round_to_pixels(ui.ctx().pixels_per_point());
    let outline = rounded(rect, radius, ui.ctx().pixels_per_point());
    Shape::Vec(vec![
        filled(&outline, fill),
        rim(&outline, tones, pixel_of(ui)),
    ])
}

// ── Text on the page ─────────────────────────────────────────────────────────

/// Paint `galley` with the shadow the game gives lettering laid straight on a picture
/// (`FontStyle="Shadow"`): a dark haze round the line, soft enough to read as shade rather
/// than as a box, and a black drop a pixel down.
fn shadowed(painter: &egui::Painter, at: Pos2, galley: &Arc<egui::Galley>, colour: Color32) {
    let line = Rect::from_min_size(at, galley.size()).shrink2(vec2(0.0, galley.size().y * 0.15));
    painter.add(
        egui::epaint::RectShape::filled(line, 4.0, Color32::from_black_alpha(165))
            .with_blur_width(14.0),
    );
    painter.galley_with_override_text_color(
        at + vec2(0.0, 1.0),
        galley.clone(),
        Color32::from_black_alpha(170),
    );
    painter.galley(at, galley.clone(), colour);
}

/// `galley` painted `around` times in a ring of `radius` in `colour`: a glow or a haze, since
/// egui has no blur.
fn halo(
    painter: &egui::Painter,
    at: Pos2,
    galley: &Arc<egui::Galley>,
    colour: Color32,
    radius: f32,
    around: usize,
) {
    for step in 0..around {
        let angle = std::f32::consts::TAU * step as f32 / around as f32;
        painter.galley_with_override_text_color(
            at + radius * vec2(angle.cos(), angle.sin()),
            galley.clone(),
            colour,
        );
    }
}

/// A label laid straight on the page rather than in a panel, which carries its shadow so it
/// reads over the painting behind it. Wraps as a label does.
pub fn page_label(ui: &mut egui::Ui, text: egui::RichText) -> egui::Response {
    let spoken = text.text().to_owned();
    let galley = egui::WidgetText::from(text).into_galley(
        ui,
        Some(egui::TextWrapMode::Wrap),
        ui.available_width(),
        egui::TextStyle::Body,
    );
    let (rect, response) = ui.allocate_exact_size(galley.size(), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &spoken));
    if ui.is_rect_visible(rect) {
        let colour = ui.visuals().text_color();
        if has_backdrop(ui.ctx()) {
            shadowed(ui.painter(), rect.min, &galley, colour);
        } else {
            ui.painter().galley(rect.min, galley, colour);
        }
    }
    response
}

// ── Panels ───────────────────────────────────────────────────────────────────

/// A framed panel: see-through navy over the page, the panel rim around it, a fan in each top
/// corner, and an optional section heading. Spans the full content width.
pub fn panel<R>(
    ui: &mut egui::Ui,
    caption: Option<&str>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    framed(
        ui,
        theme::panel_glass(),
        Tones::PANEL,
        true,
        caption,
        contents,
    )
}

/// The notice variant of the panel: the rim takes an outcome accent and the ground warms very
/// slightly toward it; the text stays parchment - the rim carries the tone. No fans: a notice
/// is a message within a section, not a section.
pub fn notice<R>(
    ui: &mut egui::Ui,
    accent: Color32,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let glass = theme::panel_glass();
    let warmed = Color32::from_rgba_unmultiplied(
        theme::PANEL_NAVY.r().saturating_add(accent.r() / 24),
        theme::PANEL_NAVY.g().saturating_add(accent.g() / 24),
        theme::PANEL_NAVY.b().saturating_add(accent.b() / 24),
        glass.a(),
    );
    framed(ui, warmed, Tones::accent(accent), false, None, contents)
}

fn framed<R>(
    ui: &mut egui::Ui,
    fill: Color32,
    tones: Tones,
    fans: bool,
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
                section_heading(ui, caption);
            }
            contents(ui)
        });

    let ppp = ui.ctx().pixels_per_point();
    let rect = inner.response.rect.round_to_pixels(ppp);
    let outline = rounded(rect, RADIUS, ppp);
    let mut shapes = vec![filled(&outline, fill)];
    if fans && let Some([left, right]) = art(ui.ctx()).and_then(|art| art.fans) {
        shapes.push(fan(rect, left.id(), false, ppp));
        shapes.push(fan(rect, right.id(), true, ppp));
    }
    shapes.push(rim(&outline, tones, pixel_of(ui)));
    ui.painter().set(ground, Shape::Vec(shapes));
    inner.inner
}

/// One corner fan, the game's Wonder popup corner: [`FAN`] square from the rim's outer
/// corner, cut to the panel's rounded inner box, so the rim lies over its outer edge and it
/// begins right where the rim ends.
fn fan(panel: Rect, texture: egui::TextureId, right: bool, ppp: f32) -> Shape {
    let square = if right {
        Rect::from_min_size(
            pos2(panel.right() - FAN, panel.top()),
            egui::Vec2::splat(FAN),
        )
    } else {
        Rect::from_min_size(panel.left_top(), egui::Vec2::splat(FAN))
    };
    // The square cut to the inner box: its rounded corner, then the rest of the square.
    let inner = panel.shrink(RIM);
    let radius = RADIUS - RIM;
    let steps = ((radius * ppp).ceil() as usize).clamp(3, 24);
    let (centre, from) = if right {
        (
            pos2(inner.right() - radius, inner.top() + radius),
            270.0_f32,
        )
    } else {
        (pos2(inner.left() + radius, inner.top() + radius), 180.0)
    };
    let mut points: Vec<Pos2> = (0..=steps)
        .map(|step| {
            let angle = (from + 90.0 * step as f32 / steps as f32).to_radians();
            centre + radius * vec2(angle.cos(), angle.sin())
        })
        .collect();
    if right {
        points.extend([
            pos2(inner.right(), square.bottom()),
            square.left_bottom(),
            pos2(square.left(), inner.top()),
        ]);
    } else {
        points.extend([
            pos2(square.right(), inner.top()),
            square.right_bottom(),
            pos2(inner.left(), square.bottom()),
        ]);
    }
    let mut mesh = egui::Mesh::with_texture(texture);
    for point in &points {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: *point,
            uv: ((*point - square.min) / FAN).to_pos2(),
            color: Color32::WHITE,
        });
    }
    for i in 1..points.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    Shape::mesh(mesh)
}

/// A section's heading: its name glowing warm gold, as the game's titles do, with a rule
/// running on from it to the panel's edge, standing on the name's baseline.
fn section_heading(ui: &mut egui::Ui, caption: &str) {
    let galley = egui::WidgetText::from(
        egui::RichText::new(caption)
            .size(theme::CAPTION_SIZE)
            .color(theme::HEADING),
    )
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, galley.size().y), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, caption));
    ui.add_space(2.0);
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let glow = theme::HEADING_GLOW;
    halo(
        painter,
        rect.min,
        &galley,
        glow.gamma_multiply(0.045),
        4.0,
        16,
    );
    halo(
        painter,
        rect.min,
        &galley,
        glow.gamma_multiply(0.08),
        1.5,
        12,
    );
    painter.galley_with_override_text_color(
        rect.min + vec2(0.0, 1.0),
        galley.clone(),
        Color32::from_black_alpha(160),
    );
    painter.galley(rect.min, galley.clone(), theme::HEADING);

    // The rule: a rim's top tone over its side tone, raised off the baseline by about the
    // height of a lowercase letter's middle.
    let ppp = ui.ctx().pixels_per_point();
    let baseline = galley
        .rows
        .first()
        .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
        .unwrap_or(galley.size().y * 0.8);
    let y = (rect.top() + baseline - 0.3 * theme::CAPTION_SIZE).round_to_pixels(ppp);
    let from = rect.left() + galley.size().x + 0.6 * theme::CAPTION_SIZE;
    if from < rect.right() {
        rule(painter, from..=rect.right(), y, 1.0, pixel_of(ui));
    }
}

/// The two-row rule a heading runs on with, the hairline divides by, and the top bar stands
/// on: a rim's top tone over its side tone, `top` its upper edge. `fade` from 1 (solid to
/// the end) towards 0 is how much of it holds before it fades out towards the right.
fn rule(
    painter: &egui::Painter,
    span: std::ops::RangeInclusive<f32>,
    top: f32,
    fade: f32,
    pixel: f32,
) {
    let (left, right) = (*span.start(), *span.end());
    let hold = left + (right - left) * fade;
    let row = 1.0_f32.max(pixel);
    for (y, colour) in [(top, theme::RIM_TOP), (top + row, theme::RIM_SIDE)] {
        let mut mesh = egui::Mesh::default();
        let band = Rect::from_min_max(pos2(left, y), pos2(right, y + row));
        if fade >= 1.0 {
            mesh.add_colored_rect(band, colour);
        } else {
            let points = [
                (left, colour),
                (hold, colour),
                (right, Color32::TRANSPARENT),
            ];
            for pair in points.windows(2) {
                let base = mesh.vertices.len() as u32;
                for (x, c) in pair {
                    mesh.colored_vertex(pos2(*x, band.top()), *c);
                    mesh.colored_vertex(pos2(*x, band.bottom()), *c);
                }
                mesh.add_triangle(base, base + 1, base + 3);
                mesh.add_triangle(base, base + 3, base + 2);
            }
        }
        painter.add(Shape::mesh(mesh));
    }
}

/// A divider inside a panel: the rule, across the panel's width.
pub fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 6.0), egui::Sense::hover());
    let top = (rect.center().y - 1.0).round_to_pixels(ui.ctx().pixels_per_point());
    rule(
        ui.painter(),
        rect.left()..=rect.right(),
        top,
        1.0,
        pixel_of(ui),
    );
}

// ── The top bar ──────────────────────────────────────────────────────────────

/// The top bar, the Civilopedia's masthead: page navy see-through to the painting, across the
/// window's top edge, standing on a rule that fades out towards the right. On it, at the left,
/// the wordmark - `before`, `emblem` on a hex tile, `after`, as the Civilopedia's sets its VP
/// - and `corner` at the right.
pub fn header(ui: &mut egui::Ui, [before, emblem, after]: [&str; 3], corner: Option<&str>) {
    let label = format!("{before} {emblem} {after}");
    let (bar, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), TOP_BAR), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter();
    painter.rect_filled(bar, 0.0, theme::chrome_glass());
    rule(
        painter,
        bar.left()..=bar.right(),
        bar.bottom() - RIM,
        0.62,
        pixel_of(ui),
    );

    let set = |text: String, size: f32, tracking: f32| {
        egui::WidgetText::from(
            egui::RichText::new(text)
                .size(size)
                .extra_letter_spacing(tracking),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Heading,
        )
    };
    // The words as a nameplate sets them, spaced capitals; the tile's letters a size up, so
    // the eye lands on what is being installed.
    let before = set(before.to_uppercase(), WORD_SIZE, WORD_TRACKING);
    let after = set(after.to_uppercase(), WORD_SIZE, WORD_TRACKING);
    let middle = (bar.top() + (bar.height() - RIM) / 2.0).round_to_pixels(ppp);
    let tile_radius = TILE_RADIUS;
    let tile_half_width = (tile_radius * HEX_WIDTH_RATIO).round();
    // Two letters fit the tile at the full size; a longer emblem is set smaller, by just as
    // much as it needs to clear the tile's rim.
    let room = 2.0 * (tile_half_width - RIM - 2.0);
    let emblem = {
        let full = set(emblem.to_owned(), EMBLEM_SIZE, 0.0);
        if full.size().x <= room {
            full
        } else {
            set(
                emblem.to_owned(),
                (EMBLEM_SIZE * room / full.size().x).floor(),
                0.0,
            )
        }
    };

    let shadow = has_backdrop(ui.ctx());
    let word = |galley: &Arc<egui::Galley>, x: f32| {
        let at = pos2(x, middle - galley.size().y / 2.0 + TEXT_DROP).round_to_pixels(ppp);
        if shadow {
            shadowed(painter, at, galley, theme::PARCHMENT_BRIGHT);
        } else {
            painter.galley(at, galley.clone(), theme::PARCHMENT_BRIGHT);
        }
    };
    // The whole wordmark on the window's centre.
    let width = before.size().x + 2.0 * (TILE_GAP + tile_half_width) + after.size().x;
    let mut x = (bar.center().x - width / 2.0).round_to_pixels(ppp);
    word(&before, x);
    x += before.size().x + TILE_GAP;
    let centre = pos2(x + tile_half_width, middle).round_to_pixels(ppp);
    let tile = hexagon(centre, tile_radius);
    painter.add(filled(&tile, theme::PANEL_NAVY));
    painter.add(rim(&tile, Tones::PANEL, pixel_of(ui)));
    let emblem_at = pos2(
        centre.x - emblem.size().x / 2.0,
        centre.y - emblem.size().y / 2.0 + TEXT_DROP,
    );
    halo(
        painter,
        emblem_at,
        &emblem,
        theme::HEADING_GLOW.gamma_multiply(0.1),
        2.0,
        12,
    );
    painter.galley(emblem_at, emblem.clone(), theme::GOLD_BRIGHT);
    x = centre.x + tile_half_width + TILE_GAP;
    word(&after, x);

    if let Some(corner) = corner {
        let font = egui::TextStyle::Small.resolve(ui.style());
        painter.text(
            pos2(bar.right() - f32::from(PAGE_MARGIN), middle),
            egui::Align2::RIGHT_CENTER,
            corner,
            font,
            theme::PARCHMENT_DIM,
        );
    }
}

/// How far below the line box's centre the capitals' optical centre falls.
const TEXT_DROP: f32 = 1.0;
/// The wordmark's words: size and tracking.
const WORD_SIZE: f32 = 17.0;
const WORD_TRACKING: f32 = 2.5;
/// The emblem's size: the largest type in the bar.
const EMBLEM_SIZE: f32 = 19.0;
/// The tile's centre-to-point radius: as tall as the bar leaves room for.
const TILE_RADIUS: f32 = 19.0;
/// The air either side of the tile.
const TILE_GAP: f32 = 7.0;

// ── Progress ─────────────────────────────────────────────────────────────────

/// How far a progress bar's fill sits inside its trough: clear of the 2 px rim by a pixel.
const FRAME_INSET: f32 = 3.0;

/// The progress bar: a well with the panel rim. `Some(fraction)` draws the gold fill from the
/// left, lit from above as the rims are; `None` draws diagonal stripes across the whole
/// trough - working, but with nothing to measure. Never animated, so a still render is a
/// faithful picture of it.
pub fn progress(ui: &mut egui::Ui, fraction: Option<f32>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.add(rimmed_box(ui, rect, RADIUS, theme::WELL_NAVY, Tones::PANEL));

    let trough = rect.shrink(FRAME_INSET);
    match fraction {
        Some(fraction) => {
            let filled = fraction.clamp(0.0, 1.0) * trough.width();
            if filled >= 1.0 {
                let fill = Rect::from_min_size(trough.min, vec2(filled, trough.height()));
                // Gold, brightest along its top, as the rim is.
                let mut mesh = egui::Mesh::default();
                for (y, colour) in [
                    (fill.top(), theme::GOLD_BRIGHT),
                    (fill.bottom(), theme::GOLD_DARK),
                ] {
                    mesh.colored_vertex(pos2(fill.left(), y), colour);
                    mesh.colored_vertex(pos2(fill.right(), y), colour);
                }
                mesh.add_triangle(0, 1, 3);
                mesh.add_triangle(0, 3, 2);
                painter.add(Shape::mesh(mesh));
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

// ── Buttons and fields ───────────────────────────────────────────────────────

/// The primary action: minimum 150 × 30 px, its label a size up. A plain egui button
/// underneath, so AccessKit still sees a button with a label.
pub fn primary_button(ui: &mut egui::Ui, enabled: bool, label: &str) -> egui::Response {
    button(
        ui,
        enabled,
        egui::Button::new(
            egui::RichText::new(label)
                .size(theme::BUTTON_LARGE_SIZE)
                .color(if enabled {
                    theme::PARCHMENT_BRIGHT
                } else {
                    theme::PARCHMENT_DIM
                }),
        )
        .min_size(vec2(150.0, 30.0)),
    )
}

/// Ground reserved under a widget, painted once the widget has reported its rectangle and
/// what state it is in.
///
/// egui's own frame has one flat border colour, and the rim is three; so the widget is added
/// with its own fill and border suppressed, and this paints the box underneath. Reserved
/// *before* the widget so it lands behind it.
#[must_use = "a plate that is never settled leaves its widget with no frame at all"]
pub struct Plate(egui::layers::ShapeIdx);

/// Reserve the ground under the next widget added to `ui`.
pub fn plate(ui: &egui::Ui) -> Plate {
    Plate(ui.painter().add(Shape::Noop))
}

impl Plate {
    /// The box for something that is pressed: raised navy, lit when the pointer is on it or it
    /// is held down, its rim brightened by keyboard focus; sunk to the panel in the quiet rim
    /// while it cannot be used.
    pub fn settle(self, ui: &egui::Ui, response: &egui::Response) {
        let tones = tones_for(response);
        let fill = if !response.enabled() {
            theme::PANEL_NAVY
        } else if tones == Tones::PANEL {
            theme::RAISED_NAVY
        } else {
            theme::RAISED_HOVER
        };
        ui.painter()
            .set(self.0, rimmed_box(ui, response.rect, RADIUS, fill, tones));
    }

    /// The box for a field: sunk into a well, lifted to the panel's tone while it is being
    /// typed in. Its rim stays the panel's; only a field that cannot be used goes quiet.
    pub fn settle_well(self, ui: &egui::Ui, response: &egui::Response) {
        let (fill, tones) = if !response.enabled() {
            (theme::WELL_NAVY, Tones::QUIET)
        } else if response.has_focus() {
            (theme::PANEL_NAVY, Tones::PANEL)
        } else {
            (theme::WELL_NAVY, Tones::PANEL)
        };
        ui.painter()
            .set(self.0, rimmed_box(ui, response.rect, RADIUS, fill, tones));
    }
}

/// Blank the fill and border egui would paint for a widget, so a [`Plate`] can be painted in
/// their place.
///
/// [`button`] and [`text_field`] say this on the widget's own builder, which is tidier. This
/// is for the ones whose builder has no such setting - `ComboBox` has neither a fill nor a
/// stroke - and so has to be told through the style instead. Set it on a scoped `Ui`, and
/// restore it inside anything that widget opens: a dropdown's rows need their real colours to
/// show which one the pointer is on.
pub fn blank_frames(ui: &mut egui::Ui) {
    let widgets = &mut ui.visuals_mut().widgets;
    for state in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
    ] {
        state.bg_fill = Color32::TRANSPARENT;
        state.weak_bg_fill = Color32::TRANSPARENT;
        state.bg_stroke = Stroke::NONE;
    }
}

/// A button in the rim: egui's own, with its frame suppressed and the box painted under it.
/// Sizing, sense, focus and keyboard handling stay egui's.
pub fn button(ui: &mut egui::Ui, enabled: bool, button: egui::Button<'_>) -> egui::Response {
    let plate = plate(ui);
    let response = ui.add_enabled(
        enabled,
        button.fill(Color32::TRANSPARENT).stroke(Stroke::NONE),
    );
    plate.settle(ui, &response);
    response
}

/// A single-line text box in a well, on the same terms as [`button`].
///
/// The margin is set here because a frameless `TextEdit` puts its text hard against the edge
/// of its rectangle, and the box needs the same room egui's own frame would have taken.
pub fn text_field(ui: &mut egui::Ui, field: egui::TextEdit<'_>) -> egui::Response {
    let plate = plate(ui);
    let response = ui.add(
        // An empty frame, not egui's. The room for the text goes on that frame and not
        // through `TextEdit::margin`, which egui only consults when it is building the frame
        // itself - hand it one and the margin is silently dropped.
        field.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
            TEXT_FIELD_MARGIN.0,
            TEXT_FIELD_MARGIN.1,
        ))),
    );
    plate.settle_well(ui, &response);
    response
}

/// How far a path box's text sits inside its box.
const TEXT_FIELD_MARGIN: (i8, i8) = (8, 4);

// ── Choices ──────────────────────────────────────────────────────────────────

/// A radio button: a hex tile, pointy-top as the game's map tiles stand - a well in the quiet
/// rim, the panel's rim under the pointer, and a smaller gold tile inside the one chosen. The
/// same tile the top bar's emblem sits on.
pub fn radio_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    chosen: T,
    label: &str,
) -> egui::Response {
    let selected = *current == chosen;
    let mut response = choice(
        ui,
        egui::WidgetType::RadioButton,
        selected,
        label,
        |painter, mark, tones, ppp| {
            // A point a pixel above and below the mark, so the tile, narrower than a square
            // of its height, carries the same weight as the checkboxes beside it.
            let outline = hexagon(mark.center(), mark.height() / 2.0 + 1.0);
            painter.add(filled(&outline, theme::WELL_NAVY));
            painter.add(rim(&outline, tones, 1.0 / ppp));
            if selected {
                let mark_radius = mark.height() * MARK_RADIUS;
                painter.add(Shape::convex_polygon(
                    hexagon(mark.center(), mark_radius + 1.5),
                    theme::HEADING_GLOW.gamma_multiply(0.25),
                    Stroke::NONE,
                ));
                painter.add(Shape::convex_polygon(
                    hexagon(mark.center(), mark_radius),
                    theme::GOLD_BRIGHT,
                    Stroke::NONE,
                ));
            }
        },
    );
    if response.clicked() && !selected {
        *current = chosen;
        response.mark_changed();
    }
    response
}

/// The chosen radio's inner tile, centre to point, as a fraction of the mark's height.
const MARK_RADIUS: f32 = 0.24;

/// A checkbox: a small well in the quiet rim, the panel's rim under the pointer; ticked, a gold
/// tick.
pub fn checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str) -> egui::Response {
    let ticked = *checked;
    let mut response = choice(
        ui,
        egui::WidgetType::Checkbox,
        ticked,
        label,
        |painter, mark, tones, ppp| {
            let outline = rounded(mark, CHECKBOX_RADIUS, ppp);
            painter.add(filled(&outline, theme::WELL_NAVY));
            painter.add(rim(&outline, tones, 1.0 / ppp));
            if ticked {
                // The Civilopedia's tick, drawn in a 10-unit box at the mark's centre.
                let unit = (mark.width() - 2.0 * RIM) / 11.0;
                let at = |x: f32, y: f32| mark.center() + unit * vec2(x - 5.0, y - 5.0);
                painter.add(Shape::line(
                    vec![at(1.5, 5.2), at(4.0, 7.6), at(8.6, 2.4)],
                    Stroke::new(2.0, theme::GOLD_BRIGHT),
                ));
            }
        },
    );
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response
}

/// A choice mark with its label: lays the two out as egui lays out its own, reports itself to
/// AccessKit as `kind`, and leaves the mark to `paint`, given a square on whole pixels and
/// the rim its state calls for.
fn choice(
    ui: &mut egui::Ui,
    kind: egui::WidgetType,
    selected: bool,
    label: &str,
    paint: impl FnOnce(&egui::Painter, Rect, Tones, f32),
) -> egui::Response {
    let mark_size = ui.spacing().icon_width;
    let gap = ui.spacing().icon_spacing;
    let galley = egui::WidgetText::from(label).into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let (rect, response) = ui.allocate_exact_size(
        vec2(
            mark_size + gap + galley.size().x,
            galley.size().y.max(mark_size),
        ),
        egui::Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(kind, ui.is_enabled(), selected, label));

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let ppp = ui.ctx().pixels_per_point();
        // On whole pixels, so the rim's straight stretches are crisp.
        let mark = Rect::from_center_size(
            pos2(rect.left() + mark_size / 2.0, rect.center().y),
            egui::Vec2::splat(mark_size),
        )
        .round_to_pixels(ppp);
        // A step quieter than a button's: the quiet rim at rest, the panel's under the pointer.
        let tones = match tones_for(&response) {
            tones if tones == Tones::PANEL => Tones::QUIET,
            tones if tones == Tones::LIT => Tones::PANEL,
            tones => tones,
        };
        paint(ui.painter(), mark, tones, ppp);
        ui.painter().galley(
            pos2(
                rect.left() + mark_size + gap,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            visuals.text_color(),
        );
    }
    response
}

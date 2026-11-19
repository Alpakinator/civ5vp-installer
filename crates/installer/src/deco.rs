//! The painted components: backdrop, top bar, panels, headings, rules, progress, and the
//! widgets egui would otherwise draw for itself.
//!
//! The look is the Vox Populi Civilopedia's Civ V style (`docs/ui-reference/visual-language.md`):
//! rounded boxes in a 2 px gold rim lit from straight above, see-through navy panels over the
//! game's own main-menu painting, a corner fan in each panel's top corners, and section
//! headings in warm gold with a rule running on from the name. The painting, the fans
//! and a card's ring and glass are the game's, read from the player's installation at runtime
//! (`game_art`, ADR-0012); everything else here is painted in code.
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
    /// The ring and the glass a card's emblem is drawn with, only ever together.
    emblem: Option<[egui::TextureHandle; 2]>,
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
    let emblem = art.icon_frame.zip(art.bare_disc).map(|(ring, glass)| {
        [
            ctx.load_texture("icon-frame", ring, small),
            ctx.load_texture("bare-disc", glass, small),
        ]
    });
    ctx.data_mut(|data| {
        data.insert_temp(
            art_id(),
            Art {
                backdrop,
                fans,
                emblem,
            },
        );
    });
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
    rounded_each(rect, [radius; 4], pixels_per_point)
}

/// [`rounded`], each corner to its own radius: top left, top right, bottom right, bottom left.
fn rounded_each(rect: Rect, radii: [f32; 4], pixels_per_point: f32) -> Vec<Pos2> {
    let most = (rect.width() / 2.0).min(rect.height() / 2.0);
    let radii = radii.map(|radius| radius.clamp(0.0, most));
    let corners = [
        (rect.left_top(), vec2(1.0, 1.0), 180.0_f32),
        (rect.right_top(), vec2(-1.0, 1.0), 270.0),
        (rect.right_bottom(), vec2(-1.0, -1.0), 0.0),
        (rect.left_bottom(), vec2(1.0, -1.0), 90.0),
    ];
    let mut points: Vec<Pos2> = Vec::new();
    for ((corner, inward, start), radius) in corners.into_iter().zip(radii) {
        if radius <= 0.0 {
            points.push(corner);
            continue;
        }
        let centre = corner + inward * radius;
        let steps = ((radius * pixels_per_point).ceil() as usize).clamp(3, 24);
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

/// A section's heading: its name in warm gold over a dark drop a pixel down, with no glow, and a
/// rule running on from it to the panel's edge, standing on the name's baseline.
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
/// the wordmark - `before`, `emblem` engraved on a hex tile of glass, `after`, as the
/// Civilopedia's sets its VP - and `corner` at the right.
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
    // The words as a nameplate sets them, spaced capitals.
    let before = set(before.to_uppercase(), WORD_SIZE, WORD_TRACKING);
    let after = set(after.to_uppercase(), WORD_SIZE, WORD_TRACKING);
    let middle = (bar.top() + (bar.height() - RIM) / 2.0).round_to_pixels(ppp);
    let tile_radius = TILE_RADIUS;
    let tile_half_width = (tile_radius * HEX_WIDTH_RATIO).round();

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
    // The tile is the dropdown's emblem cut to the map tile's shape: the same glass, running
    // to the rim and no further, and the same engraved letters.
    let tile = hexagon(centre, tile_radius);
    let glass = offset_path(&tile, centre, RIM - 0.5);
    painter.add(hex_glass(ui, centre, &glass));
    painter.add(rim(&tile, Tones::PANEL, pixel_of(ui)));
    // The letters fit the circle the hexagon's flat sides touch, so a wide emblem clears the
    // rim at the sides as a narrow one does.
    let flat = glass
        .iter()
        .map(|corner| corner.distance(centre))
        .fold(0.0, f32::max)
        * HEX_WIDTH_RATIO;
    engrave(ui, centre, flat, &[emblem]);
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

// ── The emblem dropdown ──────────────────────────────────────────────────────

/// One line of an [`emblem_dropdown`]: the value it picks, the letters engraved on its emblem
/// (a line each), its words, and small print after them for what the words abbreviate.
pub struct EmblemChoice<T> {
    pub value: T,
    pub emblem: &'static [&'static str],
    pub label: &'static str,
    pub note: Option<&'static str>,
}

/// A card's ring across, which is also the card's height: the Civilopedia's 32 px cards, grown
/// as the installer's lettering is larger than the Civilopedia's (16 to 15 and more).
const CARD_RING: f32 = 36.0;
/// The rounding of a card box's right end: the Civilopedia's card radius.
const CARD_RADIUS: f32 = 9.0;
/// Between the ring and the words, and the words and their note: the Civilopedia's 0.4 em.
const CARD_GAP: f32 = 7.0;
/// After the words, inside a card's right end: 0.6 em.
const CARD_END: f32 = 10.0;
/// Between the cards in the open list.
const CARD_SPACING: f32 = 3.0;
/// The open list's padding round its cards, across and down.
const LIST_PADDING: egui::Margin = egui::Margin::symmetric(8, 6);
/// Between the closed box and the list under it.
const LIST_GAP: f32 = 3.0;
/// The closed box's arrow, and how far in from the box's right edge it ends.
const ARROW: egui::Vec2 = vec2(9.0, 6.0);
const ARROW_INSET: f32 = 7.0;
/// How bright a card's ring is drawn in the list: at rest, and hovered or chosen.
const RING_QUIET: f32 = 0.68;
const RING_ACTIVE: f32 = 0.85;

/// A dropdown whose lines are the Civilopedia's cards: each an emblem in the game's ring as its
/// left edge, and a box from under the ring's centre holding the words. Closed, it is the chosen
/// line's card, raised, with an arrow at its right end; open, a list of the cards, the chosen
/// one's words in gold. To AccessKit it is a combo box called `name` whose value is the chosen
/// line's words, and each card an option.
pub fn emblem_dropdown<T: PartialEq + Clone>(
    ui: &mut egui::Ui,
    id_salt: &str,
    name: &str,
    current: &mut T,
    choices: &[EmblemChoice<T>],
) -> egui::Response {
    let id = ui.make_persistent_id(id_salt);
    let popup_id = id.with("popup");
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    let texts: Vec<(Arc<egui::Galley>, Option<Arc<egui::Galley>>)> = choices
        .iter()
        .map(|choice| emblem_texts(ui, choice))
        .collect();
    // Every card is as wide as the longest line, and the box as wide as the list, so the list
    // hangs flush under the box.
    let words = texts
        .iter()
        .map(|(label, note)| {
            label.size().x + note.as_ref().map_or(0.0, |note| CARD_GAP + note.size().x)
        })
        .fold(0.0, f32::max);
    let list_frame = egui::Frame::menu(ui.style()).inner_margin(LIST_PADDING);
    let list_margin = list_frame.total_margin().sum().x;
    let width = (CARD_RING + CARD_GAP + words + CARD_END + list_margin)
        .max(CARD_RING + CARD_GAP + words + CARD_GAP + ARROW.x + ARROW_INSET);
    let card_width = width - list_margin;
    let chosen = choices
        .iter()
        .position(|choice| choice.value == *current)
        .unwrap_or(0);

    let (rect, _) = ui.allocate_exact_size(vec2(width, CARD_RING), egui::Sense::hover());
    let mut response = ui.interact(rect, id, egui::Sense::click());
    let label = choices.get(chosen).map_or("", |choice| choice.label);
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), name);
        info.current_text_value = Some(label.to_owned());
        info
    });
    if ui.is_rect_visible(rect) {
        let enabled = response.enabled();
        let tones = if enabled && open && !response.has_focus() {
            Tones::LIT
        } else {
            tones_for(&response)
        };
        let fill = match tones {
            _ if !enabled => theme::PANEL_NAVY,
            Tones::PANEL => theme::RAISED_NAVY,
            _ => theme::RAISED_HOVER,
        };
        let ink = if enabled {
            theme::PARCHMENT_BRIGHT
        } else {
            theme::PARCHMENT_DIM
        };
        let ring = card(ui, rect, fill, tones, false);
        if let (Some(choice), Some((label, note))) = (choices.get(chosen), texts.get(chosen)) {
            emblem(ui, ring, 1.0, choice.emblem);
            card_words(ui, ring, label, note.as_ref(), ink);
        }
        let right = rect.right() - ARROW_INSET;
        let arrow = Rect::from_min_max(
            pos2(right - ARROW.x, rect.center().y - ARROW.y / 2.0),
            pos2(right, rect.center().y + ARROW.y / 2.0),
        );
        ui.painter().add(Shape::convex_polygon(
            vec![arrow.left_top(), arrow.right_top(), arrow.center_bottom()],
            ink,
            Stroke::NONE,
        ));
    }

    let mut changed = false;
    egui::Popup::menu(&response)
        .id(popup_id)
        .width(width)
        .gap(LIST_GAP)
        .frame(list_frame)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = CARD_SPACING;
            for (choice, (label, note)) in choices.iter().zip(&texts) {
                let selected = choice.value == *current;
                let (rect, row) =
                    ui.allocate_exact_size(vec2(card_width, CARD_RING), egui::Sense::click());
                row.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::SelectableLabel,
                        ui.is_enabled(),
                        selected,
                        choice.label,
                    )
                });
                if ui.is_rect_visible(rect) {
                    // At rest a card is quiet, its rim and ring dimmed; hovered, pressed or
                    // chosen it takes the panel's rim and a brighter ring, and hovered or
                    // pressed its fill lightens. The chosen one's words are gold.
                    let lit = row.hovered() || row.is_pointer_button_down_on();
                    let tones = if row.has_focus() && !row.hovered() {
                        Tones::FOCUS
                    } else if lit || selected {
                        Tones::PANEL
                    } else {
                        Tones::QUIET
                    };
                    let fill = if lit || row.has_focus() {
                        theme::RAISED_HOVER
                    } else {
                        theme::RAISED_NAVY
                    };
                    let ink = if selected {
                        theme::GOLD_BRIGHT
                    } else if lit || row.has_focus() {
                        theme::PARCHMENT_BRIGHT
                    } else {
                        theme::PARCHMENT
                    };
                    let brightness = if lit || selected || row.has_focus() {
                        RING_ACTIVE
                    } else {
                        RING_QUIET
                    };
                    let ring = card(ui, rect, fill, tones, true);
                    emblem(ui, ring, brightness, choice.emblem);
                    card_words(ui, ring, label, note.as_ref(), ink);
                }
                if row.clicked() && !selected {
                    *current = choice.value.clone();
                    changed = true;
                }
            }
        });
    if changed {
        response.mark_changed();
    }
    response
}

/// Open the [`emblem_dropdown`] that `ui` is about to show under `id_salt`.
pub fn open_emblem_dropdown(ui: &egui::Ui, id_salt: &str) {
    egui::Popup::open_id(ui.ctx(), ui.make_persistent_id(id_salt).with("popup"));
}

/// A line's words in the Body style, as a Civilopedia card's name is in the page's, and its note
/// in the Small one.
fn emblem_texts<T>(
    ui: &egui::Ui,
    choice: &EmblemChoice<T>,
) -> (Arc<egui::Galley>, Option<Arc<egui::Galley>>) {
    let galley = |text: egui::RichText, style: egui::TextStyle| {
        egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            style,
        )
    };
    let label = galley(
        egui::RichText::new(choice.label).color(theme::PARCHMENT_BRIGHT),
        egui::TextStyle::Body,
    );
    let note = choice.note.map(|note| {
        galley(
            egui::RichText::new(note)
                .small()
                .color(theme::PARCHMENT_DIM),
            egui::TextStyle::Small,
        )
    });
    (label, note)
}

/// A card in `rect`: the square of its ring at its left, as tall as the card, and its box from
/// under the ring's centre to the card's right, so that the ring is the card's left edge and the
/// box's top and bottom run straight into the ring's tangents. The box's left end is square and
/// lies under the ring; its right end is rounded. `fill` inside a rim of `tones`, and the game's
/// shading over the fill if `shaded`. Returns the ring's square, for the emblem.
fn card(ui: &egui::Ui, rect: Rect, fill: Color32, tones: Tones, shaded: bool) -> Rect {
    let ppp = ui.ctx().pixels_per_point();
    let rect = rect.round_to_pixels(ppp);
    let ring = Rect::from_min_size(rect.min, egui::Vec2::splat(rect.height()));
    let body = Rect::from_min_max(pos2(ring.center().x, rect.top()), rect.max);
    let radii = [0.0, CARD_RADIUS, CARD_RADIUS, 0.0];
    let painter = ui.painter();
    painter.add(shaded_fill(body, radii, fill, shaded, ppp));
    painter.add(rim(&rounded_each(body, radii, ppp), tones, pixel_of(ui)));
    ring
}

/// The shading the game lays over its boxes (`Grid9Black8`), as the Civilopedia's cards wear it:
/// a thin sheen along the top, the lower half sinking darker, and a faint blue light thrown back
/// along the bottom. Each stop is a depth into the box inside its rim, in points from the top or
/// as a share of the height, the colour, and its opacity over the fill.
fn shade_stops(height: f32) -> [(f32, [u8; 3], f32); 7] {
    [
        (0.0, [255, 255, 255], 0.096),
        (3.0, [255, 255, 255], 0.03),
        (6.0, [0, 0, 0], 0.0),
        (0.3 * height, [0, 0, 0], 0.0),
        (0.78 * height, [0, 0, 0], 0.24),
        (height - 5.0, [0, 0, 0], 0.24),
        (height, [70, 140, 220], 0.12),
    ]
}

/// The shading `depth` points into a box `height` tall, over `fill`: its stops blended as a CSS
/// gradient blends them, each no higher than the one before.
fn shaded_tone(fill: Color32, depth: f32, height: f32) -> Color32 {
    let mut stops = shade_stops(height);
    for k in 1..stops.len() {
        stops[k].0 = stops[k].0.max(stops[k - 1].0);
    }
    // Each stop's colour premultiplied by its opacity, so a fade to clear does not darken.
    let premultiplied =
        |(_, rgb, alpha): (f32, [u8; 3], f32)| (rgb.map(|c| f32::from(c) * alpha), alpha);
    let (colour, alpha) = match stops.iter().position(|stop| depth < stop.0) {
        None => premultiplied(stops[stops.len() - 1]),
        Some(0) => premultiplied(stops[0]),
        Some(next) => {
            let (a, b) = (stops[next - 1], stops[next]);
            let t = ((depth - a.0) / (b.0 - a.0).max(f32::EPSILON)).clamp(0.0, 1.0);
            let ((from, from_alpha), (to, to_alpha)) = (premultiplied(a), premultiplied(b));
            (
                [0, 1, 2].map(|k| from[k] + (to[k] - from[k]) * t),
                from_alpha + (to_alpha - from_alpha) * t,
            )
        }
    };
    let over = |base: u8, k: usize| (f32::from(base) * (1.0 - alpha) + colour[k]).round() as u8;
    Color32::from_rgb(over(fill.r(), 0), over(fill.g(), 1), over(fill.b(), 2))
}

/// A box's fill, running under its rim as [`filled`]'s does, with each corner rounded to its
/// own radius and, if `shaded`, the game's shading over it. Built as strips across the box, a
/// strip's edges at every stop of the shading and every pixel through the rounded corners, each
/// edge in its depth's tone, so the shading is exact and the corners round.
fn shaded_fill(body: Rect, radii: [f32; 4], fill: Color32, shaded: bool, ppp: f32) -> Shape {
    let inset = RIM - 0.5;
    let inner = body.shrink(inset);
    let radii = radii.map(|radius| (radius - inset).max(0.0));
    let padding = body.shrink(RIM);
    let pixel = 1.0 / ppp;
    let mut heights = vec![inner.top(), inner.bottom()];
    if shaded {
        heights.extend(
            shade_stops(padding.height())
                .iter()
                .map(|stop| padding.top() + stop.0),
        );
    }
    let corner = radii.iter().copied().fold(0.0, f32::max);
    let mut y = inner.top();
    while y < inner.top() + corner {
        heights.extend([y, inner.bottom() - (y - inner.top())]);
        y += pixel;
    }
    heights.retain(|y| (inner.top()..=inner.bottom()).contains(y));
    heights.sort_by(f32::total_cmp);
    heights.dedup_by(|a, b| (*a - *b).abs() < 1e-3);

    // How far a side is drawn in at `y` by its top and bottom corners.
    let indent = |y: f32, top: f32, bottom: f32| {
        let (from_top, from_bottom) = (y - inner.top(), inner.bottom() - y);
        let cut = |radius: f32, from: f32| {
            let d = radius - from;
            radius - (radius * radius - d * d).max(0.0).sqrt()
        };
        if from_top < top {
            cut(top, from_top)
        } else if from_bottom < bottom {
            cut(bottom, from_bottom)
        } else {
            0.0
        }
    };
    let mut mesh = egui::Mesh::default();
    for (k, &y) in heights.iter().enumerate() {
        let tone = if shaded {
            shaded_tone(fill, y - padding.top(), padding.height())
        } else {
            fill
        };
        mesh.colored_vertex(pos2(inner.left() + indent(y, radii[0], radii[3]), y), tone);
        mesh.colored_vertex(pos2(inner.right() - indent(y, radii[1], radii[2]), y), tone);
        if k > 0 {
            let (a, b, c, d) = (
                2 * k as u32 - 2,
                2 * k as u32 - 1,
                2 * k as u32,
                2 * k as u32 + 1,
            );
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    Shape::mesh(mesh)
}

/// A card's words: its name a gap after the ring, centred on the card's height, in `ink`, and
/// its note after the name on the same baseline, in Parchment dim.
fn card_words(
    ui: &egui::Ui,
    ring: Rect,
    label: &Arc<egui::Galley>,
    note: Option<&Arc<egui::Galley>>,
    ink: Color32,
) {
    let painter = ui.painter();
    let at = pos2(
        ring.right() + CARD_GAP,
        ring.center().y - label.size().y / 2.0,
    )
    .round_to_pixels(ui.ctx().pixels_per_point());
    painter.galley_with_override_text_color(at, label.clone(), ink);
    if let Some(note) = note {
        let baseline = |galley: &egui::Galley| {
            galley
                .rows
                .first()
                .and_then(|row| row.row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y))
                .unwrap_or(galley.size().y * 0.8)
        };
        painter.galley_with_override_text_color(
            pos2(
                at.x + label.size().x + CARD_GAP,
                at.y + baseline(label) - baseline(note),
            )
            .round_to_pixels(ui.ctx().pixels_per_point()),
            note.clone(),
            theme::PARCHMENT_DIM,
        );
    }
}

// ── The emblem ───────────────────────────────────────────────────────────────

/// `IconFrame64` in its own pixels: its size; its ring's centre and outer radius, as the
/// Civilopedia measures them (`card_ring`), where a card's box meets the ring's tangents; and
/// the centre and radius of its black hole, where the glass goes.
const FRAME_SIZE: f32 = 64.0;
const RING_CENTRE: egui::Vec2 = vec2(31.84, 31.72);
const RING_RADIUS: f32 = 25.73;
const HOLE_CENTRE: egui::Vec2 = vec2(32.01, 31.97);
const HOLE_RADIUS: f32 = 22.0;
/// `45x45BaseFrame` in its own pixels: its size, and its glass's centre and the radius out to
/// which it is still glass, short of the dark line where its gold begins.
const DISC_SIZE: f32 = 45.0;
const DISC_CENTRE: egui::Vec2 = vec2(22.56, 22.41);
const DISC_RADIUS: f32 = 17.0;

/// An emblem in `ring`, the square of a card's ring: the game's frame for its 64 px icons,
/// drawn `brightness` bright, the glass of the game's bare round button filling the frame's
/// hole, and `lines` engraved on the glass, centred on it. Without the game's art, the frame and
/// the glass are drawn in code to the same measure.
fn emblem(ui: &egui::Ui, ring: Rect, brightness: f32, lines: &[&str]) {
    let painter = ui.painter();
    let pixel = pixel_of(ui);
    // One of the frame's pixels, in points.
    let k = ring.width() / (2.0 * RING_RADIUS);
    let centre = ring.center();
    let hole_centre = centre + (HOLE_CENTRE - RING_CENTRE) * k;
    let hole = HOLE_RADIUS * k;
    let shade = Color32::from_gray((255.0 * brightness).round() as u8);
    if let Some([frame, glass]) = art(ui.ctx()).and_then(|art| art.emblem) {
        painter.image(
            frame.id(),
            Rect::from_min_size(centre - RING_CENTRE * k, egui::Vec2::splat(FRAME_SIZE * k)),
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            shade,
        );
        painter.add(glass_disc(glass.id(), hole_centre, hole, pixel));
    } else {
        painter.add(drawn_ring(centre, RING_RADIUS * k, hole, shade, pixel));
        painter.add(drawn_glass(hole_centre, hole, pixel));
    }
    engrave(ui, hole_centre, hole, lines);
}

/// The bare button's glass laid in a hole of `radius` at `centre`: its glass out to
/// [`DISC_RADIUS`], stretched to the hole, with half a pixel's feather either side of the edge.
/// Sampled once, at the size it is drawn.
fn glass_disc(texture: egui::TextureId, centre: Pos2, radius: f32, pixel: f32) -> Shape {
    let mut mesh = egui::Mesh::with_texture(texture);
    let uv = |offset: egui::Vec2| {
        ((DISC_CENTRE + offset * (DISC_RADIUS / radius)) / DISC_SIZE).to_pos2()
    };
    let vertex = |offset: egui::Vec2, color: Color32| egui::epaint::Vertex {
        pos: centre + offset,
        uv: uv(offset),
        color,
    };
    mesh.vertices.push(vertex(egui::Vec2::ZERO, Color32::WHITE));
    let around = 72;
    for step in 0..around {
        let angle = std::f32::consts::TAU * step as f32 / around as f32;
        let direction = vec2(angle.cos(), angle.sin());
        mesh.vertices
            .push(vertex(direction * (radius - pixel / 2.0), Color32::WHITE));
        mesh.vertices.push(vertex(
            direction * (radius + pixel / 2.0),
            Color32::TRANSPARENT,
        ));
    }
    for step in 0..around {
        let next = (step + 1) % around;
        let (inner, outer) = (1 + 2 * step, 2 + 2 * step);
        let (inner_next, outer_next) = (1 + 2 * next, 2 + 2 * next);
        mesh.add_triangle(0, inner, inner_next);
        mesh.add_triangle(inner, outer, outer_next);
        mesh.add_triangle(inner, outer_next, inner_next);
    }
    Shape::mesh(mesh)
}

/// `IconFrame64`'s gold as it runs round the ring, lit from above: near white at the top, gold
/// down the sides, bronze at the foot.
const RING_TOP: Color32 = Color32::from_rgb(0xFF, 0xFD, 0xF3);
const RING_SIDE: Color32 = Color32::from_rgb(0xD6, 0xBE, 0x8E);
const RING_FOOT: Color32 = Color32::from_rgb(0x9A, 0x7E, 0x48);

/// A ring of `IconFrame64`'s measure drawn in code, for when the game's art is not to hand: a
/// band of gold from the hole out to `outer`, brightest across its middle, shaded by `shade`,
/// with a pixel's feather outside and the hole's black inside.
fn drawn_ring(centre: Pos2, outer: f32, hole: f32, shade: Color32, pixel: f32) -> Shape {
    let lit = |colour: Color32| {
        Color32::from_rgb(
            (u16::from(colour.r()) * u16::from(shade.r()) / 255) as u8,
            (u16::from(colour.g()) * u16::from(shade.g()) / 255) as u8,
            (u16::from(colour.b()) * u16::from(shade.b()) / 255) as u8,
        )
    };
    let gold = |at: Pos2, across: f32| {
        let down = ((at.y - centre.y) / outer).clamp(-1.0, 1.0);
        let tone = if down < 0.0 {
            lerp(RING_SIDE, RING_TOP, (-down).powf(1.5))
        } else {
            lerp(RING_SIDE, RING_FOOT, down.powf(1.2))
        };
        // Rounded metal: full at the crest, a fifth darker at either edge.
        let crest = 1.0 - 0.2 * ((across - 0.45) / 0.55).abs().min(1.0).powi(2);
        lit(lerp(Color32::BLACK, tone, crest))
    };
    let bands: [(f32, f32, BandTone<'_>); 3] = [
        (hole - pixel, hole, &|_, _| Color32::BLACK),
        (hole, outer, &|at, r| gold(at, (r - hole) / (outer - hole))),
        (outer, outer + pixel, &|at, r| {
            if r > outer {
                Color32::TRANSPARENT
            } else {
                gold(at, 1.0)
            }
        }),
    ];
    let mut mesh = egui::Mesh::default();
    for (inner, outer, tone) in bands {
        ring_band(&mut mesh, centre, inner, outer, tone);
    }
    Shape::mesh(mesh)
}

/// The glass of `45x45BaseFrame`, drawn in code to fill a hole of `radius` at `centre`: grey
/// gloss under the top, near black across the middle, deepening to blue toward the foot, and
/// darker toward the rim, most on the left.
fn drawn_glass(centre: Pos2, radius: f32, pixel: f32) -> Shape {
    let tone = |at: Pos2, _: f32| drawn_glass_tone(at, centre, radius);
    let mut mesh = egui::Mesh::default();
    let rings = 12;
    for ring in 0..rings {
        let (inner, outer) = (
            radius * ring as f32 / rings as f32,
            radius * (ring + 1) as f32 / rings as f32,
        );
        ring_band(&mut mesh, centre, inner, outer, &tone);
    }
    ring_band(&mut mesh, centre, radius, radius + pixel / 2.0, &|at, r| {
        if r > radius {
            Color32::TRANSPARENT
        } else {
            tone(at, r)
        }
    });
    Shape::mesh(mesh)
}

/// A colour for each point of a band round a centre, given the point and its distance out.
type BandTone<'a> = &'a dyn Fn(Pos2, f32) -> Color32;

/// Down the glass's middle, top to foot, as `45x45BaseFrame` has it: at each height, as a share
/// of the radius from the centre, its tone.
const GLASS_DOWN: [(f32, Color32); 9] = [
    (-1.0, Color32::from_rgb(0x80, 0x80, 0x80)),
    (-0.85, Color32::from_rgb(0x6C, 0x6C, 0x6C)),
    (-0.61, Color32::from_rgb(0x4C, 0x4C, 0x4C)),
    (-0.38, Color32::from_rgb(0x31, 0x31, 0x33)),
    (-0.14, Color32::from_rgb(0x17, 0x1A, 0x23)),
    (0.0, Color32::from_rgb(0x0C, 0x12, 0x22)),
    (0.33, Color32::from_rgb(0x00, 0x08, 0x24)),
    (0.68, Color32::from_rgb(0x00, 0x0E, 0x3C)),
    (1.0, Color32::from_rgb(0x00, 0x18, 0x66)),
];

/// The drawn glass's tone at `at`, on glass of `radius` at `centre`: its middle's tone at that
/// height, darkened toward the rim, most on the left.
fn drawn_glass_tone(at: Pos2, centre: Pos2, radius: f32) -> Color32 {
    let (x, y) = ((at.x - centre.x) / radius, (at.y - centre.y) / radius);
    let colour =
        GLASS_DOWN
            .windows(2)
            .find(|pair| y <= pair[1].0)
            .map_or(GLASS_DOWN[8].1, |pair| {
                lerp(
                    pair[0].1,
                    pair[1].1,
                    ((y - pair[0].0) / (pair[1].0 - pair[0].0)).clamp(0.0, 1.0),
                )
            });
    let reach = (x * x + y * y).sqrt();
    let rim = ((reach - 0.75) / 0.25).clamp(0.0, 1.0) * (0.55 - 0.35 * x);
    lerp(colour, Color32::BLACK, rim.clamp(0.0, 1.0))
}

/// The glass cut to a hexagon: `corners`, the hexagon's six points round `centre`, filled with
/// the glass of the bare button (or the drawn glass without the game's art) as if a disc of it
/// reached just past the points - so the glass runs to the hexagon's edge and no further, its
/// dark rim showing only toward the points. Built as nested hexagons, each side cut in eight,
/// so the drawn glass's tones fall where they should.
fn hex_glass(ui: &egui::Ui, centre: Pos2, corners: &[Pos2]) -> Shape {
    let radius = corners
        .iter()
        .map(|corner| corner.distance(centre))
        .fold(0.0, f32::max);
    let texture = art(ui.ctx())
        .and_then(|art| art.emblem)
        .map(|[_, glass]| glass.id());
    let mut mesh = texture.map_or_else(egui::Mesh::default, egui::Mesh::with_texture);
    let (rings, cuts) = (8u32, 8u32);
    let around = corners.len() as u32 * cuts;
    for ring in 0..=rings {
        let scale = ring as f32 / rings as f32;
        for side in 0..corners.len() {
            let (from, to) = (corners[side], corners[(side + 1) % corners.len()]);
            for cut in 0..cuts {
                let along = from + (to - from) * (cut as f32 / cuts as f32);
                let at = centre + (along - centre) * scale;
                let vertex = match texture {
                    // A hair inside the glass's own edge at the points, so its gold never shows.
                    Some(_) => egui::epaint::Vertex {
                        pos: at,
                        uv: ((DISC_CENTRE + (at - centre) * (0.97 * DISC_RADIUS / radius))
                            / DISC_SIZE)
                            .to_pos2(),
                        color: Color32::WHITE,
                    },
                    None => egui::epaint::Vertex {
                        pos: at,
                        uv: egui::epaint::WHITE_UV,
                        color: drawn_glass_tone(at, centre, radius),
                    },
                };
                mesh.vertices.push(vertex);
            }
        }
    }
    for ring in 0..rings {
        for step in 0..around {
            let next = (step + 1) % around;
            let (a, b) = (ring * around + step, ring * around + next);
            let (c, d) = (a + around, b + around);
            mesh.add_triangle(a, b, d);
            mesh.add_triangle(a, d, c);
        }
    }
    Shape::mesh(mesh)
}

/// A band round `centre` from `inner` to `outer`, each of its points coloured by `tone`.
fn ring_band(mesh: &mut egui::Mesh, centre: Pos2, inner: f32, outer: f32, tone: BandTone<'_>) {
    let around = 72u32;
    let base = mesh.vertices.len() as u32;
    for step in 0..around {
        let angle = std::f32::consts::TAU * step as f32 / around as f32;
        for r in [inner, outer] {
            let at = centre + r * vec2(angle.cos(), angle.sin());
            mesh.colored_vertex(at, tone(at, r));
        }
    }
    for step in 0..around {
        let next = (step + 1) % around;
        let (a, b, c, d) = (
            base + 2 * step,
            base + 2 * step + 1,
            base + 2 * next,
            base + 2 * next + 1,
        );
        mesh.add_triangle(a, b, c);
        mesh.add_triangle(b, d, c);
    }
}

/// The engraved letters: cream at the top darkening to gold at the foot, at these heights down
/// a line's letters - the Civilopedia's link buttons' fill.
const ENGRAVING: [(f32, Color32); 4] = [
    (0.0, Color32::from_rgb(0xFE, 0xFE, 0xEC)),
    (0.5, Color32::from_rgb(0xEF, 0xEE, 0xD2)),
    (0.85, Color32::from_rgb(0xDC, 0xD9, 0xB4)),
    (1.0, Color32::from_rgb(0xC8, 0xBF, 0x94)),
];
/// How much of the glass the letters may cover: across, and down for one line or for all of
/// several, as shares of the glass's radius. The Civilopedia's logos span about seven tenths of
/// the disc.
const ENGRAVING_ACROSS: f32 = 1.38;
const ENGRAVING_DOWN: f32 = 0.84;
const ENGRAVING_DOWN_LINES: f32 = 1.12;

/// `lines` engraved on the glass of `radius` at `centre`, as the Civilopedia engraves its link
/// buttons' logos: each line as large as fits its share of the glass, the block centred on its
/// ink; a soft dark shadow cast down and a little right, then the letters in the engraving's
/// gradient, drawn once in the font's own weight.
fn engrave(ui: &egui::Ui, centre: Pos2, radius: f32, lines: &[&str]) {
    if lines.is_empty() {
        return;
    }
    let painter = ui.painter();
    let ppp = ui.ctx().pixels_per_point();
    let galley = |text: &str, size: f32| {
        let mut job = egui::text::LayoutJob::single_section(
            text.to_owned(),
            egui::TextFormat {
                font_id: egui::FontId::proportional(size),
                color: Color32::WHITE,
                ..Default::default()
            },
        );
        job.wrap.max_width = f32::INFINITY;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    // The first of several lines takes the larger share of the height.
    let (down, first_share) = if lines.len() > 1 {
        (ENGRAVING_DOWN_LINES, 0.56)
    } else {
        (ENGRAVING_DOWN, 1.0)
    };
    let gap = radius * 0.12;
    let room = vec2(
        radius * ENGRAVING_ACROSS,
        radius * down - gap * (lines.len() - 1) as f32,
    );
    let mut laid: Vec<(Arc<egui::Galley>, Rect)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let share = if index == 0 {
            first_share
        } else {
            (1.0 - first_share) / (lines.len() - 1) as f32
        };
        let probe = ink_of(&galley(line, 20.0));
        let size =
            20.0 * (room.x / probe.width().max(1.0)).min(room.y * share / probe.height().max(1.0));
        let line = galley(line, size.max(6.0));
        let ink = ink_of(&line);
        laid.push((line, ink));
    }
    let total =
        laid.iter().map(|(_, ink)| ink.height()).sum::<f32>() + gap * (laid.len() - 1) as f32;
    let mut top = centre.y - total / 2.0;
    // The shadow, in the glass's own pixels as the Civilopedia's filter has it: cast 0.45 right
    // and 0.9 down, blurred 0.7, at eight tenths black. Drawn as a few soft copies.
    let unit = radius / DISC_RADIUS;
    let cast = vec2(0.45, 0.9) * unit;
    let spread = 0.7 * unit;
    for (line, ink) in laid {
        let at = pos2(centre.x - ink.center().x, top - ink.top()).round_to_pixels(ppp);
        top += ink.height() + gap;
        painter.galley_with_override_text_color(
            at + cast,
            line.clone(),
            Color32::from_black_alpha(115),
        );
        for step in 0..6 {
            let angle = std::f32::consts::TAU * step as f32 / 6.0;
            painter.galley_with_override_text_color(
                at + cast + spread * vec2(angle.cos(), angle.sin()),
                line.clone(),
                Color32::from_black_alpha(30),
            );
        }
        painter.galley(at, graded(&line, ink), Color32::WHITE);
    }
}

/// `galley` with its letters coloured down `ink`, the box its letters cover, in the engraving's
/// gradient: each glyph cut across at the gradient's stops, so the tone between two cuts is the
/// gradient's own, and each cut in its height's tone.
fn graded(galley: &Arc<egui::Galley>, ink: Rect) -> Arc<egui::Galley> {
    let tone = |y: f32| {
        let t = ((y - ink.top()) / ink.height().max(f32::EPSILON)).clamp(0.0, 1.0);
        ENGRAVING
            .windows(2)
            .find(|pair| t <= pair[1].0)
            .map_or(ENGRAVING[3].1, |pair| {
                lerp(
                    pair[0].1,
                    pair[1].1,
                    (t - pair[0].0) / (pair[1].0 - pair[0].0),
                )
            })
    };
    let mut graded = (**galley).clone();
    let (mut vertices, mut indices) = (0, 0);
    for placed in &mut graded.rows {
        let mut row = (*placed.row).clone();
        let source = std::mem::take(&mut row.visuals.mesh);
        let mut mesh = egui::Mesh::with_texture(source.texture_id);
        for quad in source.vertices[row.visuals.glyph_vertex_range.clone()].chunks_exact(4) {
            let [left_top, right_top, left_bottom, right_bottom] =
                [quad[0], quad[1], quad[2], quad[3]];
            let (top, bottom) = (left_top.pos.y, left_bottom.pos.y);
            let mut cuts = vec![top, bottom];
            cuts.extend(
                ENGRAVING
                    .iter()
                    .map(|(t, _)| ink.top() + t * ink.height() - placed.pos.y)
                    .filter(|y| *y > top && *y < bottom),
            );
            cuts.sort_by(f32::total_cmp);
            let mut previous: Option<u32> = None;
            for y in cuts {
                let t = (y - top) / (bottom - top).max(f32::EPSILON);
                let colour = tone(placed.pos.y + y);
                let first = mesh.vertices.len() as u32;
                for (upper, lower) in [(left_top, left_bottom), (right_top, right_bottom)] {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: upper.pos + (lower.pos - upper.pos) * t,
                        uv: upper.uv + (lower.uv - upper.uv) * t,
                        color: colour,
                    });
                }
                if let Some(above) = previous {
                    mesh.add_triangle(above, above + 1, first);
                    mesh.add_triangle(first, above + 1, first + 1);
                }
                previous = Some(first);
            }
        }
        row.visuals.glyph_index_start = 0;
        row.visuals.glyph_vertex_range = 0..mesh.vertices.len();
        vertices += mesh.vertices.len();
        indices += mesh.indices.len();
        row.visuals.mesh = mesh;
        placed.row = Arc::new(row);
    }
    graded.num_vertices = vertices;
    graded.num_indices = indices;
    Arc::new(graded)
}

/// Where a galley's letters actually put ink, relative to its top left: the union of its
/// glyphs' rectangles, rather than its line's.
fn ink_of(galley: &egui::Galley) -> Rect {
    let mut ink = Rect::NOTHING;
    for row in &galley.rows {
        for glyph in &row.row.glyphs {
            let min = row.pos + glyph.pos.to_vec2() + glyph.uv_rect.offset;
            ink = ink.union(Rect::from_min_size(min, glyph.uv_rect.size));
        }
    }
    if ink.is_positive() {
        ink
    } else {
        Rect::from_min_size(Pos2::ZERO, galley.size())
    }
}

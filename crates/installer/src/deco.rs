//! The painted art-deco components: page, header, panels, divider, progress.
//!
//! Everything here is drawn from the recipes in `docs/ui-reference/visual-language.md` -
//! the four motifs (lit rim, hex crown, rule, hatched fill), the line
//! weights, and the chamfered corners. All of it is original artwork painted in code; no
//! game asset is embedded or traced (ADR-0003).
//!
//! Presentation only: these functions lay out and paint, and decide nothing.

use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Shape, Stroke, pos2, vec2};

use crate::theme;

/// Page margin on every side of the window (visual language, "Proportions").
pub const PAGE_MARGIN: i8 = 20;
/// Internal padding of a framed panel.
const PANEL_PADDING: i8 = 10;
/// The 45° cut taken off each corner of a panel.
const PANEL_CHAMFER: f32 = 8.0;
/// How far a progress bar's fill sits inside its trough: clear of the 2 px rim by a pixel.
const FRAME_INSET: f32 = 3.0;

/// The window surface: page-navy ground, page margin all round.
///
/// All round except on the right, where the margin belongs to the scroll bar: the page
/// scrolls, and a bar drawn at the edge of the content would sit on the panels' right frame
/// line. The scrolling area is given the whole width and puts the margin back inside itself,
/// so the panels end where they always did and the bar rides in the space beyond them.
pub fn page(style: &egui::Style) -> egui::Frame {
    // No margin on either side: both are held *inside* the scrolling area instead, as its
    // content margin. egui hands a wheel event to a scroll area only while the pointer is
    // within that area's own rectangle, so a side margin left out here is a strip of the
    // window where the wheel does nothing - which is how the left edge behaved until 0.1.5.
    // The right was already inside, which is why that edge and the scroll bar always worked.
    //
    // Top and bottom stay here on purpose. Moving them in makes the scrolling viewport 40px
    // taller, which shifts every screen; the wheel is not dead there in the same way, because
    // the content reaches those edges once the page has scrolled at all.
    egui::Frame::central_panel(style)
        .fill(theme::PAGE_NAVY)
        .inner_margin(egui::Margin {
            left: 0,
            right: 0,
            ..egui::Margin::same(PAGE_MARGIN)
        })
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

/// A panel's ground: a filled chamfered octagon with a raised rim on its edge.
fn rimmed_plate(rect: Rect, fill: Color32, metal: Color32, pixel: f32) -> Shape {
    let outline = chamfered(rect, PANEL_CHAMFER);
    let mut shapes = vec![Shape::convex_polygon(outline.clone(), fill, Stroke::NONE)];
    shapes.extend(rim(
        &outline,
        true,
        rect.center(),
        metal,
        Relief::Raised,
        pixel,
    ));
    Shape::Vec(shapes)
}

// ── The rim ──────────────────────────────────────────────────────────────────

/// Which way a rim is struck: standing out of the surface, or sunk into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relief {
    Raised,
    Sunken,
}

/// Where the light comes from: above and a little to the right, as Civilization V lights its
/// own frames - their top and right edges catch it, their left and bottom fall into bronze.
const LIGHT: egui::Vec2 = vec2(0.33, -0.94);

/// How wide a rim is: a lit face and the shade beside it.
const RIM: f32 = 2.0;

/// The metal edge every frame in the skin is drawn with, after Civilization V's own: one line,
/// two pixels deep - a face that catches the light, and a shade inside it that only brightens
/// where the face is turned fully up.
///
/// Built as facets, the way a machined frame is: every straight stretch is lit by the one way
/// it faces and is that one colour from end to end, and neighbouring stretches meet on a mitre
/// at the corner. Light is never blended along an edge, so no edge brightens or darkens as it
/// runs, and two pieces facing the same way are always the same colour.
///
/// `outline` is the outer edge of the solid shape and the rim lies just inside it, so a frame
/// whose corners are on whole pixels has every straight stretch of its rim on whole pixels.
/// `inside` is any point on the inner side of every stretch: the shape's centre will do.
/// `pixel` is one physical pixel in points - the width of the soft edge either side.
fn rim(
    outline: &[Pos2],
    closed: bool,
    inside: Pos2,
    metal: Color32,
    relief: Relief,
    pixel: f32,
) -> Vec<Shape> {
    vec![Shape::mesh(rim_mesh(
        outline, closed, inside, metal, relief, pixel,
    ))]
}

fn rim_mesh(
    outline: &[Pos2],
    closed: bool,
    inside: Pos2,
    metal: Color32,
    relief: Relief,
    pixel: f32,
) -> egui::Mesh {
    // Depths into the shape at which the colour is pinned: transparent outside, the face,
    // the face giving way to the shade across one pixel, the shade, transparent inside. On
    // whole pixels every pixel centre lands on a pinned colour, so the straight stretches are
    // crisp; on a slope the same ramps are its anti-aliasing.
    let soft = pixel / 2.0;
    let depths = [-soft, soft, 1.0 - soft, 1.0 + soft, RIM - soft, RIM + soft];
    let rows: Vec<Vec<Pos2>> = depths
        .iter()
        .map(|depth| offset_path(outline, closed, inside, *depth))
        .collect();

    let mut mesh = egui::Mesh::default();
    for (i, (_, _, normal)) in facings(outline, closed, inside).into_iter().enumerate() {
        let towards = normal.dot(LIGHT);
        let lit = match relief {
            Relief::Raised => towards,
            Relief::Sunken => -towards,
        };
        let face = tone(metal, 1.0 + 0.6 * lit);
        let shade = tone(metal, 0.35 + 0.6 * lit.max(0.0).powi(2));
        let colours = [
            Color32::TRANSPARENT,
            face,
            face,
            shade,
            shade,
            Color32::TRANSPARENT,
        ];
        let next = (i + 1) % rows[0].len();
        for band in 0..rows.len() - 1 {
            let base = mesh.vertices.len() as u32;
            for (row, colour) in [(band, colours[band]), (band + 1, colours[band + 1])] {
                mesh.colored_vertex(rows[row][i], colour);
                mesh.colored_vertex(rows[row][next], colour);
            }
            mesh.add_triangle(base, base + 1, base + 3);
            mesh.add_triangle(base, base + 3, base + 2);
        }
    }
    mesh
}

/// One physical pixel, in points.
fn pixel_of(ui: &egui::Ui) -> f32 {
    1.0 / ui.ctx().pixels_per_point()
}

/// `metal` brightened (`k` above 1, towards a pale highlight) or darkened (below 1).
fn tone(metal: Color32, k: f32) -> Color32 {
    let channel = |c: u8| {
        let c = f32::from(c);
        let value = if k <= 1.0 {
            c * k.max(0.0)
        } else {
            c + (255.0 - c) * (k - 1.0).min(1.0)
        };
        value.round().clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(channel(metal.r()), channel(metal.g()), channel(metal.b()))
}

/// Every stretch of a path with the unit normal pointing away from `inside`.
fn facings(points: &[Pos2], closed: bool, inside: Pos2) -> Vec<(Pos2, Pos2, egui::Vec2)> {
    let count = points.len();
    let stretches = if closed {
        count
    } else {
        count.saturating_sub(1)
    };
    (0..stretches)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % count]);
            let direction = (b - a).normalized();
            let mut normal = vec2(direction.y, -direction.x);
            if normal.dot(a - inside) < 0.0 {
                normal = -normal;
            }
            (a, b, normal)
        })
        .collect()
}

/// A path with every stretch moved `distance` towards `inside`, each corner where its two
/// moved stretches meet - so the new path runs exactly parallel to the old at the same
/// distance everywhere, slopes included. An open path's two ends move with their own stretch.
fn offset_path(points: &[Pos2], closed: bool, inside: Pos2, distance: f32) -> Vec<Pos2> {
    let moved: Vec<(Pos2, egui::Vec2)> = facings(points, closed, inside)
        .into_iter()
        .map(|(a, b, normal)| (a - normal * distance, (b - a).normalized()))
        .collect();
    let meet = |(p, d): (Pos2, egui::Vec2), (q, e): (Pos2, egui::Vec2)| {
        let cross = d.x * e.y - d.y * e.x;
        if cross.abs() < 1e-6 {
            return q;
        }
        let t = ((q.x - p.x) * e.y - (q.y - p.y) * e.x) / cross;
        p + d * t
    };
    let count = moved.len();
    if closed {
        (0..count)
            .map(|i| meet(moved[(i + count - 1) % count], moved[i]))
            .collect()
    } else {
        let Some(&(last_start, last_direction)) = moved.last() else {
            return points.to_vec();
        };
        let mut path = vec![moved[0].0];
        path.extend((1..count).map(|i| meet(moved[i - 1], moved[i])));
        let last_length = (points[count] - points[count - 1]).length();
        path.push(last_start + last_direction * last_length);
        path
    }
}

/// A framed panel: panel-navy ground under the content, a raised rim around it, an
/// optional caption in gold at the top. Spans the full content width.
pub fn panel<R>(
    ui: &mut egui::Ui,
    caption: Option<&str>,
    contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    framed(ui, theme::PANEL_NAVY, theme::GOLD, caption, contents)
}

/// The notice variant of the panel: the border takes an outcome accent and the ground warms
/// very slightly toward it; the text stays parchment - the border carries the tone.
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
    ui.painter().set(
        ground,
        rimmed_plate(inner.response.rect, fill, outer_line, pixel_of(ui)),
    );
    inner.inner
}

/// The header: `emblem` in a hex tile on the window's exact horizontal centre, `before` and
/// `after` on two pointed plates either side of it, a second hex outline around the tile above
/// and below the plates, and the rule running out from the plates' points to the edges of the page. Rendered once
/// per window.
///
/// One geometry throughout, so the parts read as one piece: every edge is horizontal,
/// vertical, or parallel to the tile's own slopes, and every edge is the same lit rim the
/// panels are framed with.
pub fn header(ui: &mut egui::Ui, [before, emblem, after]: [&str; 3], corner: Option<&str>) {
    let label = format!("{before} {emblem} {after}");
    let set = |text: String, size: f32, tracking: f32, colour: Color32| {
        egui::WidgetText::from(
            egui::RichText::new(text)
                .size(size)
                .extra_letter_spacing(tracking)
                .color(colour),
        )
        .into_galley(
            ui,
            Some(egui::TextWrapMode::Extend),
            f32::INFINITY,
            egui::TextStyle::Heading,
        )
    };
    // The words as a nameplate sets them - spaced capitals, a size below the emblem - so the
    // tile's two letters are the largest thing in the header and the eye lands on them first.
    let word = |text: &str| {
        set(
            text.to_uppercase(),
            WORD_SIZE,
            WORD_TRACKING,
            theme::PARCHMENT_BRIGHT,
        )
    };
    let (before, after) = (word(before), word(after));
    let emblem = set(emblem.to_owned(), EMBLEM_SIZE, 0.0, theme::GOLD_BRIGHT);

    let (area, response) = ui.allocate_exact_size(
        vec2(ui.available_width(), Crest::HEIGHT),
        egui::Sense::hover(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));

    let crest = Crest::new(
        ui.ctx().pixels_per_point(),
        area,
        before.size().x.max(after.size().x),
    );
    let painter = ui.painter();
    painter.extend(crest.shapes(area));

    // Each word is centred on the face its plate's rim encloses, and every line of text sits
    // on the band's middle.
    let text_top = |galley: &Arc<egui::Galley>| crest.centre.y - galley.size().y / 2.0 + TEXT_DROP;
    let face = offset_path(&crest.plate(1.0), true, crest.plate_centre(1.0), RIM);
    let (near, far) = face
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    let plate_middle = (near + far) / 2.0 - crest.centre.x;
    for (galley, middle) in [
        (&before, crest.centre.x - plate_middle),
        (&after, crest.centre.x + plate_middle),
        (&emblem, crest.centre.x),
    ] {
        painter.galley(
            pos2(middle - galley.size().x / 2.0, text_top(galley)),
            galley.clone(),
            theme::PARCHMENT_BRIGHT,
        );
    }

    // Painted rather than laid out, so it costs no vertical space: it rests on the rule at
    // the right, in the blank beyond the plates.
    if let Some(corner) = corner {
        let font = egui::TextStyle::Small.resolve(ui.style());
        ui.painter().text(
            pos2(area.right(), crest.centre.y - RIM / 2.0 - CORNER_LIFT),
            egui::Align2::RIGHT_BOTTOM,
            corner,
            font,
            theme::PARCHMENT_DIM,
        );
    }
    ui.add_space(1.0);
}

/// How far above the rule the corner text rests, so its descenders clear the line.
const CORNER_LIFT: f32 = 2.0;

/// How far below the line box's centre the capitals' optical centre falls.
const TEXT_DROP: f32 = 1.0;

/// The flanking words' size and tracking.
const WORD_SIZE: f32 = 19.0;
const WORD_TRACKING: f32 = 3.0;
/// The emblem's size: a fifth above the Display size, the largest type in the window.
const EMBLEM_SIZE: f32 = 31.0;

/// The header's geometry, settled once and on whole pixels, so every part is placed from the
/// same numbers and every straight rim lands on whole pixels.
struct Crest {
    /// The tile's centre: the window's horizontal centre, on a pixel boundary, so the tile's
    /// two sides are whole pixels equally far either side of it.
    centre: Pos2,
    /// Half the tile's side length.
    half: f32,
    /// Half a plate's height: out to the corners of the outline around the tile, so the
    /// plates and that outline meet.
    plate_half: f32,
    /// Centre to the tile's upright sides.
    side: f32,
    /// Centre to where each plate begins, a little clear of the tile.
    plate_start: f32,
    /// Centre to where each plate's straight run ends and its point begins.
    plate_end: f32,
    /// How far each plate's point reaches beyond `plate_end`.
    point: f32,
    /// One physical pixel, in points.
    pixel: f32,
}

/// Half the band's height. The tile is twice this from its centre to its points.
const BAND_HALF: f32 = 15.0;
/// The perpendicular gap between the tile and the outline around it.
const CROWN_GAP: f32 = 5.0;
/// How far the outline's points stand beyond the tile's: a hex's point moves 2/√3 of the
/// distance its edges are offset.
const CROWN_RISE: f32 = CROWN_GAP * 1.155;
/// The gap between the tile and each plate - the tile is set into the band, not welded to it.
const PLATE_GAP: f32 = 3.0;
/// Air between a plate's rim and the word on it.
const TITLE_GAP: f32 = 4.0;

impl Crest {
    /// Everything the header occupies: a pixel of air, the outline over the tile, the tile,
    /// the outline under it, and room beneath.
    const HEIGHT: f32 = 1.0 + 2.0 * CROWN_RISE + 4.0 * BAND_HALF + 4.0;

    fn new(pixels_per_point: f32, area: Rect, longest_word: f32) -> Self {
        let whole = |v: f32| (v * pixels_per_point).round() / pixels_per_point;

        let half = BAND_HALF;
        let band_top = whole(area.top() + 1.0 + CROWN_RISE + half);
        let centre = pos2(whole(area.center().x), band_top + half);
        // A regular hex's side is √3/2 of its point radius; on a whole pixel, a fraction of
        // a pixel away from that, which no eye can see and every pixel of the sides shows.
        let side = whole(2.0 * half * HEX_WIDTH_RATIO);
        let plate_start = side + PLATE_GAP;
        let plate_end = whole(plate_start + 2.0 * (RIM + TITLE_GAP) + longest_word);
        // Offsetting the tile's edges outward moves its side corners out along the slope, by
        // the gap times tan 30° - on a whole pixel, a tenth of one from exact.
        let plate_half = whole(half + CROWN_GAP / 3.0_f32.sqrt());
        Self {
            centre,
            half,
            plate_half,
            side,
            plate_start,
            plate_end,
            // The point's edges meet at 120°, a hex's own corner.
            point: plate_half / 3.0_f32.sqrt(),
            pixel: 1.0 / pixels_per_point,
        }
    }

    /// The tile, pointy-top.
    fn tile(&self) -> Vec<Pos2> {
        let Pos2 { x, y } = self.centre;
        let (h, s) = (self.half, self.side);
        vec![
            pos2(x, y - 2.0 * h),
            pos2(x + s, y - h),
            pos2(x + s, y + h),
            pos2(x, y + 2.0 * h),
            pos2(x - s, y + h),
            pos2(x - s, y - h),
        ]
    }

    /// One plate, `side` -1 for the left and 1 for the right.
    fn plate(&self, side: f32) -> Vec<Pos2> {
        let Pos2 { x, y } = self.centre;
        let h = self.plate_half;
        vec![
            pos2(x + side * self.plate_start, y - h),
            pos2(x + side * self.plate_end, y - h),
            pos2(x + side * (self.plate_end + self.point), y),
            pos2(x + side * self.plate_end, y + h),
            pos2(x + side * self.plate_start, y + h),
        ]
    }

    fn plate_centre(&self, side: f32) -> Pos2 {
        pos2(
            self.centre.x + side * (self.plate_start + self.plate_end) / 2.0,
            self.centre.y,
        )
    }

    fn shapes(&self, area: Rect) -> Vec<Shape> {
        let metal = theme::GOLD;
        let tile = self.tile();
        let mut shapes = Vec::new();

        // The rule: out from each plate's point to the edge of the page - a rim seen edge-on,
        // lit exactly as the plates' own top edges are, fading as it goes so the header's
        // weight stays at the centre.
        let reach = self.plate_end + self.point;
        for side in [-1.0_f32, 1.0] {
            let from = self.centre.x + side * reach;
            let to = if side < 0.0 {
                area.left()
            } else {
                area.right()
            };
            let top = self.centre.y - RIM / 2.0;
            let mut rule = rim_mesh(
                &[pos2(from, top), pos2(to, top)],
                false,
                pos2(from, self.centre.y + 1.0),
                metal,
                Relief::Raised,
                self.pixel,
            );
            let span = (to - from).abs().max(1.0);
            for vertex in &mut rule.vertices {
                let gone = ((vertex.pos.x - from).abs() / span).clamp(0.0, 1.0);
                vertex.color = vertex.color.gamma_multiply(1.0 - 0.7 * gone);
            }
            shapes.push(Shape::mesh(rule));
        }

        // The crown: the tile's outline offset outward, over its top and under its bottom,
        // the upright sides of each half carried to stand on the plates.
        let grown = offset_path(&tile, true, self.centre, -CROWN_GAP);
        let (top, bottom) = (
            self.centre.y - self.plate_half,
            self.centre.y + self.plate_half,
        );
        for half in [
            [
                pos2(grown[5].x, top),
                grown[5],
                grown[0],
                grown[1],
                pos2(grown[1].x, top),
            ],
            [
                pos2(grown[4].x, bottom),
                grown[4],
                grown[3],
                grown[2],
                pos2(grown[2].x, bottom),
            ],
        ] {
            shapes.extend(rim(
                &half,
                false,
                self.centre,
                metal,
                Relief::Raised,
                self.pixel,
            ));
        }

        // The tile and the two plates: navy faces, each with its own rim.
        for (piece, inside) in [
            (tile, self.centre),
            (self.plate(-1.0), self.plate_centre(-1.0)),
            (self.plate(1.0), self.plate_centre(1.0)),
        ] {
            shapes.push(Shape::convex_polygon(
                piece.clone(),
                theme::PANEL_NAVY,
                Stroke::NONE,
            ));
            shapes.extend(rim(&piece, true, inside, metal, Relief::Raised, self.pixel));
        }
        shapes
    }
}

/// Width of a pointy-top hexagon over its height: √3 / 2.
const HEX_WIDTH_RATIO: f32 = 0.866_025_4;

/// The six corners of a pointy-top hexagon - the orientation of the game's map tiles - with
/// `radius` from the centre to its top and bottom points.
fn hexagon_points(centre: Pos2, radius: f32) -> Vec<Pos2> {
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

/// The progress bar: a well-navy trough with a sunken rim. `Some(fraction)` draws
/// the gold fill from the left with a bright highlight along its top edge; `None` draws the
/// hatched-fill motif across the whole trough - working, but with nothing to measure.
/// Never animated, so a still render is a faithful picture of it.
pub fn progress(ui: &mut egui::Ui, fraction: Option<f32>) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 14.0), egui::Sense::hover());
    let painter = ui.painter();

    let outline = chamfered(rect, 3.0);
    painter.add(Shape::convex_polygon(
        outline.clone(),
        theme::WELL_NAVY,
        Stroke::NONE,
    ));
    painter.extend(rim(
        &outline,
        true,
        rect.center(),
        theme::GOLD_DARK,
        Relief::Sunken,
        pixel_of(ui),
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
    button(
        ui,
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
        .min_size(vec2(150.0, 30.0)),
    )
}

/// The 45° cut taken off each corner of a widget - a button, a path box - as against a
/// panel's [`PANEL_CHAMFER`]. Smaller, because a widget is a fraction of a panel's height and
/// the panel's cut would take its corners off altogether.
const WIDGET_CHAMFER: f32 = 4.0;

/// Ground reserved under a widget, painted once the widget has reported its rectangle and
/// what state it is in.
///
/// egui draws every widget frame as a rounded rectangle and offers no chamfer, so the frame
/// is drawn here instead: the widget is added with its own fill and border suppressed, and
/// this paints the cut-cornered plate underneath. Reserved *before* the widget so it lands
/// behind it - the same order [`rimmed_plate`] is put under a panel.
#[must_use = "a plate that is never settled leaves its widget with no frame at all"]
pub struct Plate(egui::layers::ShapeIdx);

/// Reserve the ground under the next widget added to `ui`.
pub fn plate(ui: &egui::Ui) -> Plate {
    Plate(ui.painter().add(Shape::Noop))
}

impl Plate {
    /// The plate for something that reacts to the pointer: the fill and border egui would
    /// have used for the state the widget is actually in, on a chamfered outline.
    pub fn settle(self, ui: &egui::Ui, response: &egui::Response) {
        let visuals = ui.style().interact(response);
        // Raised until it is pressed, when it sinks into the panel under the pointer.
        let relief = if response.is_pointer_button_down_on() {
            Relief::Sunken
        } else {
            Relief::Raised
        };
        self.paint(
            ui,
            response.rect,
            visuals.weak_bg_fill,
            visuals.bg_stroke.color,
            relief,
        );
    }

    /// The plate for a sunken well - a text box, whose ground stays the same whether or not
    /// the pointer is over it. Only its border answers.
    pub fn settle_well(self, ui: &egui::Ui, response: &egui::Response) {
        let visuals = ui.style().interact(response);
        self.paint(
            ui,
            response.rect,
            theme::WELL_NAVY,
            visuals.bg_stroke.color,
            Relief::Sunken,
        );
    }

    /// The fill under a chamfered rim in the metal the widget's state calls for: dark gold
    /// at rest, bright when the pointer is on it.
    fn paint(self, ui: &egui::Ui, rect: Rect, fill: Color32, metal: Color32, relief: Relief) {
        let outline = chamfered(rect, WIDGET_CHAMFER);
        let mut shapes = vec![Shape::convex_polygon(outline.clone(), fill, Stroke::NONE)];
        if metal != Color32::TRANSPARENT {
            shapes.extend(rim(
                &outline,
                true,
                rect.center(),
                metal,
                relief,
                pixel_of(ui),
            ));
        }
        ui.painter().set(self.0, Shape::Vec(shapes));
    }
}

/// Blank the fill and border egui would paint for a widget, so a chamfered [`Plate`] can be
/// painted in their place.
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

/// A button with its corners cut: egui's own, with the rounded frame suppressed and a
/// chamfered one painted under it. Sizing, sense, focus and keyboard handling stay egui's.
pub fn button(ui: &mut egui::Ui, enabled: bool, button: egui::Button<'_>) -> egui::Response {
    let plate = plate(ui);
    let response = ui.add_enabled(
        enabled,
        button.fill(Color32::TRANSPARENT).stroke(Stroke::NONE),
    );
    plate.settle(ui, &response);
    response
}

/// A single-line text box with its corners cut, on the same terms as [`button`].
///
/// The margin is set here because a frameless `TextEdit` puts its text hard against the edge
/// of its rectangle, and the plate needs the same room egui's own frame would have taken.
pub fn text_field(ui: &mut egui::Ui, field: egui::TextEdit<'_>) -> egui::Response {
    let plate = plate(ui);
    let response = ui.add(
        // An empty frame, not egui's: its own would paint the rounded rectangle this exists
        // to be rid of. The room for the text goes on that frame and not through
        // `TextEdit::margin`, which egui only consults when it is building the frame itself -
        // hand it one and the margin is silently dropped, and the text sits on the border.
        field.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(
            TEXT_FIELD_MARGIN.0,
            TEXT_FIELD_MARGIN.1,
        ))),
    );
    plate.settle_well(ui, &response);
    response
}

/// How far a path box's text sits inside its plate. Wider than tall: the cut corners eat into
/// the ends of the box, and a path that started in the cut would look clipped.
const TEXT_FIELD_MARGIN: (i8, i8) = (8, 4);

/// A radio button drawn as a hex tile, pointy-top as the game's map tiles stand, with a
/// smaller filled tile inside it when it is the one chosen.
///
/// egui's own is a circle, and a circle is the one shape this skin does not have. A hex is
/// the game's own unit of space, and the same tile the header's emblem sits in. Everything except the mark is egui's: the sizing, the click sense, and the
/// accessibility information a screen reader and the shell tests both read.
pub fn radio_value<T: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut T,
    chosen: T,
    label: &str,
) -> egui::Response {
    let selected = *current == chosen;
    let mut response = radio(ui, selected, label);
    if response.clicked() && !selected {
        *current = chosen;
        response.mark_changed();
    }
    response
}

fn radio(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
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
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::RadioButton,
            ui.is_enabled(),
            selected,
            label,
        )
    });

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact(&response);
        let mark = Rect::from_center_size(
            pos2(rect.left() + mark_size / 2.0, rect.center().y),
            egui::Vec2::splat(mark_size),
        );
        ui.painter().add(Shape::convex_polygon(
            hexagon_points(mark.center(), mark_size / 2.0),
            visuals.bg_fill,
            visuals.bg_stroke,
        ));
        if selected {
            ui.painter().add(Shape::convex_polygon(
                hexagon_points(mark.center(), mark_size * MARK_HALF),
                visuals.fg_stroke.color,
                Stroke::NONE,
            ));
        }
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

/// Centre-to-point of the filled mark, as a fraction of the outline's height.
const MARK_HALF: f32 = 0.3;

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

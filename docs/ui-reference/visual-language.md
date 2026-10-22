# Visual Language - the installer's art-deco skin

**This document is the source the skin is generated from.** Ticket 09's rule: components are
built from this prose, not from screenshots. The captures that may sit beside this file are
for *checking* the result afterwards, never an input. Every colour, line, and proportion in
`crates/installer/src/theme.rs` and `crates/installer/src/deco.rs` must trace to a sentence
here; when the skin changes, this file changes in the same commit.

All artwork described here is original. Nothing is extracted from, traced over, or measured
out of the game's files (ADR-0003).

## Intent

Civilization V frames its interface like a 1930s exposition poster: deep night-blue grounds,
warm parchment lettering, and machined gold trim. Ornament is disciplined - a few strong
motifs (lit rims, hex crowns, rules) applied sparingly on large
calm surfaces. The installer should read as a small, well-made instrument from that same
workshop: dark, warm, symmetrical, with gold used as a *line* colour far more often than as
a *fill* colour.

## Palette

The palette is three families - navy grounds, parchment inks, gold trim - plus two accent
tones for outcomes. Backgrounds get darker as surfaces get deeper (page → panel wells);
text gets brighter as it gets more important.

### Navy grounds

| Name        | Hex       | Role |
| ----------- | --------- | ---- |
| Page navy   | `#0E1A28` | The window background. Everything sits on this. |
| Panel navy  | `#152638` | Fill of framed panels - one step up from the page. |
| Raised navy | `#1C3350` | Interactive surfaces at rest: buttons, hovered rows. |
| Well navy   | `#0A1420` | Sunken troughs: text-entry fields, the progress trough. Darker than the page - a well, not a plate. |

### Parchment inks

| Name             | Hex       | Role |
| ---------------- | --------- | ---- |
| Parchment        | `#E6D8B0` | Body text. The default ink on navy. |
| Parchment bright | `#F4EACC` | The title and text that must carry: headings, button labels. |
| Parchment dim    | `#AC9F7C` | Secondary text: quiet status lines, activity-log detail. |

### Gold trim

| Name        | Hex       | Role |
| ----------- | --------- | ---- |
| Gold bright | `#E7C565` | Emphasis lines: hovered borders, panel captions, the lit half of a divider, progress fill highlights. |
| Gold        | `#C3963C` | The standard trim line: outer panel frames, progress fill, divider diamonds. |
| Gold dark   | `#7E6226` | Recessed trim: inner frame lines, resting widget borders, the quiet ends of a divider. |

### Outcome accents

| Name   | Hex       | Role |
| ------ | --------- | ---- |
| Laurel | `#9DBB61` | Success - "installed". Used for text and notice borders only, never large fills. |
| Ember  | `#C96A45` | Failure and refusal. Same restraint: text and border tint, never a filled alarm box. |

Rules of use: gold is trim - the only gold *fills* are the progress bar's fill and a
pressed button. Accents colour words and borders, not areas. There is no pure white and no
pure black anywhere.

## Typography

One face: **Jost** (embedded, ADR-0003 - OFL), a geometric sans of exactly the
period the ornament comes from. It is used for every piece of UI text. No italics, no
synthetic bold; hierarchy comes from size and colour.

| Style   | Size | Colour           | Use |
| ------- | ---- | ---------------- | --- |
| Display | 26 px | Parchment bright | The window title, once, in the header. |
| Caption | 17 px | Gold bright      | Panel captions ("What to install"). |
| Body    | 16 px | Parchment        | Everything conversational. |
| Button  | 17 px | Parchment bright | Button labels. |
| Small   | 13 px | Parchment dim    | Activity-log lines, fine print. |

## Motif vocabulary

Four motifs, and only these four. Restraint is the style.

1. **Lit rim.** Every border is one metal edge, 2 px deep, lying just inside the shape's
   outline, after Civilization V's own frames (`grid9framegold8`): a face pixel and a shade
   pixel inside it. Light comes from above and a little to the right, so each stretch is
   toned by the way it faces - the face brightens towards a pale highlight on top and right
   edges and falls to bronze on the left and bottom; the shade stays dark except where the
   face turns fully up. The rim is faceted like a machined frame: each straight stretch is one
   colour from end to end, set only by the way it faces, and stretches meet on a mitre at
   the corner - so the light never shifts along an edge, and every edge facing the same way,
   on any piece, is the same colour. A *raised* rim
   (panels, plates, buttons) catches the light on its outward faces; a *sunken* one (text
   boxes, the progress trough, a pressed button) on its far wall.
2. **Hex crown.** The game's own map tile - a pointy-top hexagon - set inside a second
   outline of itself 5 px out, which shows above and below whatever crosses the tile, its
   upright sides carried to stand on it. Used once, around the header's emblem, as a raised
   rim.
3. **Rule.** A rim seen edge-on, lit as any upward-facing stretch is: its face row over its
   shade row. In the header it runs out from the points of the title plates to the edges of
   the page, fading as it goes.
4. **Hatched fill.** Diagonal 45° Gold stripes (3 px wide, 9 px apart, ≈ 55% opacity) inside
   a well - the "working" state of the progress bar, when there is no fraction to show.

## Line weights

- **2 px** - every border and rule: the lit rim. One line, never doubled.
- Emphasis is the metal, not the width: a hovered widget's rim is struck in Gold bright.
- Straight stretches of a rim sit on whole pixels, so each of its two rows is one crisp
  pixel, not two half-lit ones. Slopes are only ever parallel to the hex's own edges.

## Corner treatments

- **Painted panels:** chamfered corners, 8 px cut at 45°, the rim following the cut. Never
  rounded.
- **Painted widgets** (buttons, path boxes, the version box): chamfered corners, 4 px cut at
  45°, painted by the shell rather than by egui - egui draws widget frames as rounded
  rectangles and has no chamfer, so the frame is drawn underneath and the widget's own is
  suppressed (`deco::button`, `deco::text_field`, `deco::plate`).
- **Radio buttons**: a hex tile, pointy-top as the game's map tiles stand, with a smaller
  filled hex inside the one chosen. Painted by the shell (`deco::radio_value`); egui's own is
  a circle, and a circle is the one shape this skin does not have.
- **Everything else** (checks, and every part of the embedded file browser): square,
  zero radius. Nothing in this skin is rounded; deco is straight lines and 45° cuts, and a
  radius - however small - is the one shape it does not have. Where the cut cannot be painted,
  square is at least in the language.
- Chamfers are always 45° and always symmetrical.

## Proportions and spacing

- Window: 900 × 640 points is both the default and the minimum - the layout is designed
  down to that size and the window will not shrink further.
- Page margin: 20 px on every side of the window.
- Header: ≈ 77 px tall - the VP tile in its hex outline, the title plates, the rule.
- Panels: 10 px internal padding; 6 px vertical rhythm between panels.
- Grid (label/field rows): 12 px column gap, 4 px row gap; fields 360 px wide.
- Buttons: minimum 150 × 30 px for the primary action; label centred.
- Progress bar: 14 px tall, full content width, 3 px between trough frame and fill.
- The activity log is the flexible element: it takes exactly the height that remains and
  scrolls inside itself, pinned to the newest line, so no screen ever overflows the page.

## Component recipes

**Panel.** Panel-navy octagon (8 px chamfer) filled under the content, a raised Gold rim on
its edge, 10 px padding. An optional caption in Caption style sits at the top-left inside the
frame, followed by 6 px of air. Panels span the full content width.

**Header.** "VP" in Gold bright at 31 px, a fifth above Display, in a pointy-top hex tile on
the window's exact horizontal centre. Two plates flank it 3 px clear of its sides, tall
enough to meet the corners of the outline around the tile and ending in a point whose edges
meet at 120°, a hex's own corner; "CIVILIZATION 5" and "INSTALLER" sit on them in spaced
capitals (19 px, 3 px tracking), each centred on its plate, and the plates are equal so the
whole stays symmetrical. Tile and plates have Panel-navy faces and raised Gold rims. The hex
crown frames the tile above and below the plates, and the rule runs out from the plates'
points to the page edges, the version resting on its right end. Rendered once per window.

**Button.** Raised-navy fill, a raised Gold-dark rim, Button-style parchment-bright label.
Hover: the rim is struck in Gold bright and the fill lightens one step. Pressed: the rim
sinks, the fill flips to Gold, the label to Page navy - the one moment gold is a fill. Disabled: fill
falls to Panel navy, label to Parchment dim, border stays Gold dark.

**Text field.** Well-navy trough, a sunken Gold-dark rim, Body parchment text; the caret and
selection use Gold. Focus strikes the rim in Gold bright.

**Choice marks (radio / checkbox).** Well-navy trough with a Gold-dark ring or box; the
chosen mark is a Gold fill. Label in Body parchment.

**Progress bar.** A well-navy trough with a sunken Gold-dark rim (3 px chamfer). Determinate: a Gold fill from the left, inset 3 px, with a 1 px Gold-bright
highlight along its top edge. Indeterminate: the hatched-fill motif across the whole
trough. Never animated in still renders.

**Notice.** A panel variant whose rim is struck in an outcome accent (Laurel or Ember) and
whose fill is Panel navy warmed very slightly toward the accent. The text stays parchment -
the border carries the tone.

## Screen composition

Every screen is: header, then panels, on the page ground - nothing floats outside a frame
except the status line and the primary button. The status line sits under the button in
Body size, coloured by outcome: Parchment dim at rest, Gold bright while working, Laurel on
success, Ember on failure. No screen leaves any surface in an unthemed colour.

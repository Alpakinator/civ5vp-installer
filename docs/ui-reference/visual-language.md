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
motifs (stepped frames, sunburst fans, diamond-studded rules) applied sparingly on large
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

1. **Stepped frame.** A panel border is two concentric lines: a 1 px Gold outer line on the
   panel's edge and a 1 px Gold-dark inner line inset 3 px. Corners are chamfered (see
   below), so the two lines step around each corner in parallel 45° cuts - the "stepped
   ziggurat" read.
2. **Sunburst fan.** Straight rays fanning symmetrically from a low centre point, used once:
   behind the window title. Rays are 1.5 px Gold at low opacity (≈ 22%), spanning roughly
   150° of arc, inner radius ≈ 14 px, outer radius ≈ 48 px, one ray every 10°. The fan is
   ornament behind text - it must never fight the lettering.
3. **Diamond rule.** A horizontal divider: a 1 px Gold-dark hairline across the full width,
   over-struck for its central ≈ 160 px by a 1 px Gold line, with a small solid Gold
   diamond (a square rotated 45°, 4 px from centre to point) at the exact centre. Marks the
   boundary between header and content.
4. **Hatched fill.** Diagonal 45° Gold stripes (3 px wide, 9 px apart, ≈ 55% opacity) inside
   a well - the "working" state of the progress bar, when there is no fraction to show.

## Line weights

- **1 px** - almost everything: frames, rules, widget borders. The style is drawn with a
  fine pen.
- **1.5 px** - sunburst rays only.
- **2 px** - reserved for emphasis borders (a hovered or focused interactive element).
- Nothing heavier. Weight is expressed by doubling lines, never by thickening one.

## Corner treatments

- **Painted panels:** chamfered corners, 8 px cut at 45° on the outer line; the inner line
  follows at its 3 px inset with a 6 px cut. Never rounded.
- **Native widgets** (buttons, fields, checks): 2 px corner radius - at that size a small
  radius and a small chamfer are indistinguishable, and the widget frame stays crisp.
- Chamfers are always 45° and always symmetrical.

## Proportions and spacing

- Window: 900 × 640 points is both the default and the minimum - the layout is designed
  down to that size and the window will not shrink further.
- Page margin: 20 px on every side of the window.
- Header: ≈ 50 px tall - title centred, fan behind, diamond rule below.
- Panels: 10 px internal padding; 6 px vertical rhythm between panels.
- Grid (label/field rows): 12 px column gap, 4 px row gap; fields 360 px wide.
- Buttons: minimum 150 × 30 px for the primary action; label centred.
- Progress bar: 14 px tall, full content width, 3 px between trough frame and fill.
- The activity log is the flexible element: it takes exactly the height that remains and
  scrolls inside itself, pinned to the newest line, so no screen ever overflows the page.

## Component recipes

**Panel.** Panel-navy octagon (8 px chamfer) filled under the content, stepped frame around
it, 14 px padding. An optional caption in Caption style sits at the top-left inside the
frame, followed by 6 px of air. Panels span the full content width.

**Header.** The display title centred at the top of the page, sunburst fan painted behind
it, diamond rule below. Rendered once per window.

**Button.** Raised-navy fill, 1 px Gold-dark border, Button-style parchment-bright label.
Hover: the border lifts to Gold bright at 2 px and the fill lightens one step. Pressed: the
fill flips to Gold, the label to Page navy - the one moment gold is a fill. Disabled: fill
falls to Panel navy, label to Parchment dim, border stays Gold dark.

**Text field.** Well-navy trough, 1 px Gold-dark border, Body parchment text; the caret and
selection use Gold. Focus lifts the border to Gold bright.

**Choice marks (radio / checkbox).** Well-navy trough with a Gold-dark ring or box; the
chosen mark is a Gold fill. Label in Body parchment.

**Progress bar.** A well-navy trough with its own stepped frame (1 px Gold-dark, 3 px
chamfer). Determinate: a Gold fill from the left, inset 3 px, with a 1 px Gold-bright
highlight along its top edge. Indeterminate: the hatched-fill motif across the whole
trough. Never animated in still renders.

**Notice.** A panel variant whose border takes an outcome accent (Laurel or Ember) and
whose fill is Panel navy warmed very slightly toward the accent. The text stays parchment -
the border carries the tone.

## Screen composition

Every screen is: header, then panels, on the page ground - nothing floats outside a frame
except the status line and the primary button. The status line sits under the button in
Body size, coloured by outcome: Parchment dim at rest, Gold bright while working, Laurel on
success, Ember on failure. No screen leaves any surface in an unthemed colour.

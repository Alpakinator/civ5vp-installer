# Visual Language - the installer's Civ V skin

**This document is the source the skin is generated from.** Every colour, line and proportion in
`crates/installer/src/theme.rs` and `crates/installer/src/deco.rs` must trace to a sentence
here; when the skin changes, this file changes in the same commit.

The skin is the Vox Populi Civilopedia's Civ V style (`web/src/civ5.css` in that project),
carried over to egui, so the installer and the Civilopedia read as one family. It replaced the
installer's own art-deco skin (chamfered frames lit at 45°, a centred hex crest), which the
Civilopedia had first copied and then left.

## Intent

The Civilopedia lays the game's own paintings behind see-through navy panels framed in a thin
gold rim lit from straight above. Ornament is the game's: a gold fan in each panel's top
corners, warm glowing headings. The page is calm and dark, the painting gives it depth, and
gold is a line colour, not a fill.

## Game art

Two pieces of the skin are the game's, read at runtime from the player's own installation and
never shipped (`game_art.rs`, ADR-0012):

- **The backdrop**: the base game's main-menu painting, `CivilzationVAtlas.dds` in
  `Resource/DX9/UITextures.fpk` - the one behind the Civilopedia's home page. It covers the
  window: scaled to fill it, centred, cropped at the edges, fixed while the page scrolls.
- **The fans**: the Wonder popup's `TopLeftCorner.dds` and `TopRightCorner.dds`, Brave New
  World's copies first. Each is lit as the rim it lies under, as the Civilopedia's build does:
  every row scaled by a factor passing from the rim's top tone to its side tone over the first
  10 px, so the fan's gold meets the rim at the rim's brightness.

Without a readable game (no game folder yet, a damaged archive), the page is plain Page navy
and panels have no fans. Everything else is unchanged, so the skin never depends on the art.

## Palette

### Navy grounds

| Name         | Hex       | Role |
| ------------ | --------- | ---- |
| Page navy    | `#0E1A28` | Under the backdrop; the page when there is none. The top bar, at 92%. |
| Panel navy   | `#152638` | Panels, at 92%: see-through to the painting. Opaque for what floats over the page - dropdown lists, the file browser. |
| Raised navy  | `#1C3350` | Buttons at rest. |
| Raised hover | `#234068` | A button hovered, pressed or open. |
| Well navy    | `#0A1420` | Sunken: text fields, checkboxes, radio buttons, the progress trough. |

The 92% is the Civilopedia owner's, chosen from 80 to 95 in steps of one.

### Parchment inks

| Name             | Hex       | Role |
| ---------------- | --------- | ---- |
| Parchment        | `#E6D8B0` | Body text. |
| Parchment bright | `#F4EACC` | Button labels, the wordmark. |
| Parchment dim    | `#AC9F7C` | Secondary text, the version, fine print. |
| Heading          | `#F8E7B0` | Section headings, with their glow (`#E7B450`). |

### Gold

| Name        | Hex       | Role |
| ----------- | --------- | ---- |
| Gold bright | `#E7C565` | The emblem, a radio's pearl, a checkbox's tick, the progress fill's top. |
| Gold        | `#C3963C` | The hatched stripes. |
| Gold dark   | `#7E6226` | The progress fill's foot; text selection. |

### Rims

A rim has three tones, top, side, bottom. It holds its top tone along the top, passes into the
side tone over 10 px down the sides, and into the bottom tone along the bottom: lit from
straight above. A box under 20 px tall passes over half its height instead.

| Strength | Top       | Side      | Bottom    | Used for |
| -------- | --------- | --------- | --------- | -------- |
| Panel    | `#C2A66B` | `#8A6F42` | `#5A4422` | Panels, buttons and fields at rest, the hex tile, the progress trough. |
| Quiet    | `#857350` | `#64553A` | `#463A28` | Checkboxes and radio buttons at rest; anything disabled. |
| Lit      | `#D9BF84` | as Panel  | as Panel  | Hovered, pressed or open. A checkbox or radio takes the Panel rim instead. |
| Focus    | `#EAD29A` | `#B4955A` | as Panel  | Keyboard focus, in place of an outline. |

Two strengths, so the eye lands on the content and on what is active rather than on a grid of
equal outlines.

### Outcome accents

| Name   | Hex       | Role |
| ------ | --------- | ---- |
| Laurel | `#9DBB61` | Success - text only. |
| Ember  | `#C96A45` | Failure and refusal - a notice's rim and text. |

A notice's rim is its accent down the sides, 25% toward Parchment bright along the top and 45%
toward black along the bottom.

## Typography

One face, **Jost** (embedded, ADR-0003, OFL). No italics, no synthetic bold.

| Style    | Size  | Colour           | Use |
| -------- | ----- | ---------------- | --- |
| Heading  | 19 px | Heading, glowing | Section headings ("What to install"). |
| Wordmark | 17 px, 2.5 px tracking, capitals | Parchment bright | The top bar's words. |
| Emblem   | 19 px | Gold bright      | The letters on the hex tile; smaller as needed to clear its rim. |
| Body     | 16 px | Parchment        | Everything conversational. |
| Button   | 17 px | Parchment bright | Button labels; the primary action's too. |
| Small    | 13 px | Parchment dim    | Fine print, the activity log, the version. |

**Glow.** A heading glows warm gold as the game's titles do: the Civilopedia's text-shadow
(6 px of `#E7B450` at 75%, 14 px at 35%, a black drop) approximated without blur - the name
painted in rings at 1.5 px and 4 px in faint glow colour, a black copy 1 px down, then the name.

**Page note.** The strap line under the top bar is Small in Parchment bright on a plate of black
at 59% (alpha 150), shrink-wrapped to the text with 8 × 3 px of air and 4 px corners, so it
reads over the brightest part of the painting.

**Text on the painting.** Other lettering laid straight on the page rather than in a panel - the
status line, the first-run note - carries the game's text shadow: a soft dark haze
behind the line (black at 65%, 14 px blur) and a black drop 1 px down. Only while a painting is
behind it; on plain navy it needs none.

## Shapes

- **Corners**: every box is rounded to 6 px - panels, buttons, fields, the progress trough, and
  the boxes egui draws itself (dropdown rows, the file browser). A checkbox is rounded to 3 px.
- **Rim**: 2 px, lying just inside the shape's outline, with a one-physical-pixel feather either
  side. Each point is the tone its height gives it, so the rim is the same at any size.
- **Fill**: inside the rim and no further, so a see-through fill never darkens the rim.
- **The hex tile**: a pointy-top hexagon, the game's map tile. The one shape not rounded: the
  top bar's emblem sits on one, 19 px from centre to point, and every radio button is one.

## Components

**Top bar.** The Civilopedia's masthead: 46 px across the window's top edge, Page navy at 92%,
staying put while the page scrolls under it. It stands on a rule (below) that holds solid for
62% of the width and then fades out to the right. On the window's centre, the wordmark:
"CIVILIZATION 5", the hex tile with the emblem, "INSTALLER", 7 px between word and tile - set
the way the Civilopedia's logo sets its VP inside the word. The tile is Panel navy in the Panel
rim; the emblem names what is being installed (VP, CBP, MOD). The version sits at the right,
20 px in, in Small.

**Panel.** Panel navy at 92% in the Panel rim, 10 px padding, full content width. A fan in each
top corner, 24 px square from the rim's outer corner, cut to the panel's rounded inner box, so
the rim lies over its outer edge. An optional section heading at the top.

**Section heading.** The name in Heading style, then a rule running on from it, 0.6 em after the
name, to the panel's inner edge, its top 0.3 em above the name's baseline.

**Rule.** Two 1 px rows: the Panel rim's top tone over its side tone. The heading's rule, the
divider inside a panel, and the top bar's foot.

**Button.** Raised navy in the Panel rim, Button-style label. Hovered or pressed: Raised hover in
the Lit rim. Keyboard focus: the Focus rim. Disabled: Panel navy in the Quiet rim, label
Parchment dim.

**Text field.** Well navy in the Panel rim. While typed in, the fill lifts to Panel navy.

**Checkbox.** A Well navy box rounded to 3 px in the Quiet rim; Panel rim under the pointer.
Ticked: the Civilopedia's tick, 2 px Gold bright.

**Radio button.** A Well navy hex tile in the Quiet rim, its points a pixel above and below the
mark's square so it carries the checkboxes' weight; Panel rim under the pointer. Chosen: a
smaller Gold bright tile inside, centre to point a quarter of the mark's height, in a faint glow.

**Dropdown list, file browser.** egui's own: opaque Panel navy, a 2 px border in the rim's side
tone, 6 px corners. Shadows `#0A1420` at 70%, cast down and to the right: 6 px offset and 5 px
blur for a dropdown, 12 px and 16 px for a window.

**Progress bar.** 14 px, a Well navy trough in the Panel rim. Determinate: a fill inset 3 px,
Gold bright at the top shading to Gold dark at the foot - lit from above, as the rims are.
Indeterminate: 45° Gold stripes, 3 px wide and 9 px apart, at 55% opacity. Never animated.

**Notice.** A panel in its accent's rim, no fans, its fill Panel navy warmed very slightly
toward the accent. The text stays parchment - the rim carries the tone.

## Proportions

- Window: 900 × 640 points is both the default and the minimum.
- Page margin: 20 px either side and below; 10 px between the top bar and the page.
- Panels: 10 px internal padding; 6 px between panels.
- Buttons: minimum 150 × 30 px for the primary action.
- The activity log takes the height that remains and scrolls inside itself, pinned to the
  newest line.

## Screen composition

Every screen is the top bar, then panels, over the painting. Outside a panel there are only the
strap line, the primary buttons, the status line (Parchment dim at rest, Gold bright while
working, Laurel on success), the progress bar and the storage toggle. No surface is left in an
unthemed colour.

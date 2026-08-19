# UI Reference

Screenshots of Civilization V's own interface, kept here as **visual reference only**.

## What this is for

Ticket 09 styles the installer in Civ5's art-deco language. These captures are how that work gets *checked* - render a screen with `cargo run -- --screenshot <dir>`, put it beside the reference, and judge whether they read as the same visual family.

## What this is not for

**These are not inputs to generating the skin.** Handed a screenshot and told "make it look like this", an agent produces a near-copy - which is both the wrong output and a worse one. Ticket 09's first deliverable is a *written* description of the visual language (palette values, motifs, line weights, corner treatments, proportions), and the components are built from that description. These images only come out at verification time.

## Licensing

No file in this directory is ever shipped, embedded, or redistributed in the installer binary. All installer artwork is original work in the same visual language - see ADR-0003. Keep this directory out of any release artifact.

## What's useful to capture

Whatever shows the language clearly. Roughly:

- Main menu and the game-setup screens - the primary panel and frame treatment
- A dialog or popup - borders, headers, button states
- The tech tree or civilopedia - dense-information layout
- Anything with a progress bar, a list, or a scroll region
- Close-ups of ornament: sunbursts, stepped frames, corner pieces, dividers

PNG, at whatever resolution you play at. Name them for what they show - `main-menu.png`, `dialog-confirm.png`, `panel-corner-detail.png`.

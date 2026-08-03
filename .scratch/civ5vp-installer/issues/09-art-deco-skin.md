# 09 — Civ5 art-deco skin

**What to build:** The installer looks and feels like part of Civilization V: original artwork in the game's art-deco language (navy/parchment/gold palette, sunburst and stepped-frame motifs, nine-slice panels), the embedded Tw Cen MT typeface throughout (ADR-0003), and game-quality styling of every screen — version picker, Flavor/toggle options, progress, the plain-language failure panel with copy/open-log buttons, and the storage panel. Civ5's own UI art is reference only; no game asset is shipped (ADR-0003). Hand-crafted egui theming per ADR-0002.

**Blocked by:** 01 — Walking skeleton (can proceed in parallel with 02–08; final pass restyles whatever screens exist by then).

**Status:** ready-for-human

**Expect this ticket to need human eyes.** Fine-grained visual verification — "this is off by a few pixels", "this border reads wrong" — is a known weak spot even for current models, and it is the one place in this project where an agent's own "looks right to me" should not be trusted. The render harness from ticket 01 exists to put images in front of a person quickly, not to replace the person.

**How to work:** generate from a *written* description of the visual language, not by imitating a screenshot. Handed a reference image and told "make it look like this", an agent produces a near-copy — which is both the wrong output (ADR-0003 requires original artwork) and a worse one. So the first deliverable is prose: palette values, motif vocabulary, line weights, corner treatments, proportions. Build from that. The reference captures in `docs/ui-reference/` are for *checking* the result at the end, never the input to generating it.

- [x] A written visual-language description exists (palette, motifs, line weights, proportions) and the components are generated from it — the reference captures are used only to verify, never as the thing being reproduced. It is `docs/ui-reference/visual-language.md`.
- [x] Tw Cen MT embedded and used for all UI text (`theme.rs`, first for both font families)
- [x] Reusable art-deco component set (panels, buttons, headers, progress) — painted in code (`deco.rs`), all original artwork
- [x] Every screen in the flow styled; nothing left in default egui grey
- [x] Window at common sizes/DPI scales renders without broken slicing or clipped text — verified by rendering all five screens through `--screenshot` at 900x640 / 1280x800 (and 800x600 during layout work) at scales 1 / 1.5 / 2 and reading the PNGs. 900x640 is now also the window's minimum inner size; the activity log flexes and scrolls internally so the tail never overflows.
- [x] Every styled screen has a reviewed `egui_kittest` snapshot baseline committed, so later changes surface as visual diffs
- [x] Side-by-side comparison against the Civ5 captures in `docs/ui-reference/` reads as the same visual family (reference only — no game asset is shipped, ADR-0003). Checked against the two captures in `art_for_inspiration/` after the skin was built: same near-black navy grounds, thin double-line framed panels, Tw Cen MT, parchment-on-dark — with original ornament (chamfered stepped frames, sunburst fan), nothing traced.

**Status note:** implementation complete and verified by rendering; set to ready-for-human because the ticket itself asks for human eyes on the fine-grained visuals ("this border reads wrong" is exactly the judgement an agent's 'looks right to me' should not settle).

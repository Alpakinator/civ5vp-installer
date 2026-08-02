# 09 — Civ5 art-deco skin

**What to build:** The installer looks and feels like part of Civilization V: original artwork in the game's art-deco language (navy/parchment/gold palette, sunburst and stepped-frame motifs, nine-slice panels), the embedded Tw Cen MT typeface throughout (ADR-0003), and game-quality styling of every screen — version picker, Flavor/toggle options, progress, the plain-language failure panel with copy/open-log buttons, and the storage panel. Civ5's own UI art is reference only; no game asset is shipped (ADR-0003). Hand-crafted egui theming per ADR-0002.

**Blocked by:** 01 — Walking skeleton (can proceed in parallel with 02–08; final pass restyles whatever screens exist by then).

**Status:** ready-for-agent

- [ ] Tw Cen MT embedded and used for all UI text
- [ ] Reusable art-deco component set (panels, buttons, headers, progress) as nine-slices/textures — all original artwork
- [ ] Every screen in the flow styled; nothing left in default egui grey
- [ ] Window at common sizes/DPI scales renders without broken slicing or clipped text
- [ ] Side-by-side comparison against Civ5 reference screenshots reads as the same visual family

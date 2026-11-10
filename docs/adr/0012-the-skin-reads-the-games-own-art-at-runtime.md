# The skin reads the game's own art at runtime

The installer takes the Vox Populi Civilopedia's Civ V look, which is drawn partly with the
game's own art: the main-menu painting behind the page and the Wonder popup's corner fans on the
panels. ADR-0003 rules out shipping any game asset, and that still holds. The installer ships
none. It reads both pictures from `Resource/DX9/*.fpk` in the player's own Game Installation,
the folder detection has already found, and decodes them in-process (`game_art.rs`). This is the
Civilopedia's own approach, moved from build time to runtime.

The read only reads, so the Claimed Folders rule (rule 6) is not engaged. It never fails
loudly: art that is missing or unreadable is logged and dropped, and the skin carries on with a
plain navy page and fanless panels. That fallback is also what CI's snapshot baselines show.
`--screenshot --game-art <dir>` renders the screens over a real game's art for checking by eye.

Considered: painting an original backdrop and fans in code (no painting comes close to the
game's own), and shipping the extracted art (redistribution, which ADR-0003 forbids).

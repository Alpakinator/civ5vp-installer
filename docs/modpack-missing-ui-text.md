# Missing Modpack UI text (#13364)

## Diagnosis

[Upstream issue #13364](https://github.com/LoneGazebo/Community-Patch-DLL/issues/13364)
reports untranslated loading tips and Interface Options in `5.4.6_VP-EUI_SSEM`.
The screenshots show `TXT_KEY_VPUI_TITLE_TIP`, `TXT_KEY_VPUI_TIP_26`, and several
`TXT_KEY_EUI_*_DISPLAY` keys.

All of those strings are defined in upstream's `VPUI Text/VPUI_tips_en_us.xml`.
Despite its name, this file supplies both loading tips and EUI option labels.
Its Git blob is identical at `Release-5.4.4` and `Release-5.4.6`:
`c11e209afc0d6f9a54a9fb4b710677f7f46012bb`. The reported difference between those
packs is therefore not explained by an upstream change to this text file.

The installer copied this file to the creator's Documents/Text folder during Sync,
but the earlier Modpack assembly step merged only `.modinfo` database actions into
the saved base. This standalone Text Folder file has no such action. A base that
already contained its text could mask the omission; a base without it produced a
Modpack without it. The creator's local Text file could also hide the problem while
testing a pack destined for another computer.

The in-game Modpack Maker dumps the game's already merged localization database,
which includes Documents/Text. Offline assembly must explicitly supply this input.
There is no compiler or Proton-specific behavior in this omission.

Inspection of the user's downloaded `5.4.6_VP-EUI_SSEM.zip` confirmed this directly:
its `VP_MODPACK/Override/CIV5Units_Mongol.xml` lacks all 46 English strings in the
Release's `VPUI_tips_en_us.xml`, and the archive contains no standalone copy of
that file. Both groups of keys visible in the report's screenshots are absent.

## Fix

The Core now adds the selected Version's Claimed Text File to the database job,
before the mods' activation actions. This refreshes stale cached text, preserves
modmod overrides, and includes the strings in
`VP_MODPACK/Override/CIV5Units_Mongol.xml`. Community Patch-only builds do not add
VPUI text. Sync still deploys the standalone Text File for ordinary installations.

Rebuild affected Modpacks with the fixed installer and redistribute them. Existing
downloaded packs are not changed by updating the installer alone.

For an existing pack, a local workaround is to copy the matching Release's
`VPUI Text/VPUI_tips_en_us.xml` into
`Documents/My Games/Sid Meier's Civilization 5/Text/`, then restart the game.
Under Proton, use that Documents directory inside Civilization V's `8930` prefix.

## Regression coverage

- Core-seam tests check that the Text input precedes managed and extra mod actions.
- A Core-seam test with the real SQLite assembler checks the deployed localization
  XML for the reported labels, with EUI enabled and disabled.
- The same test starts from stale cached text and an unrelated local Text file,
  then rebuilds without local text or game cache and checks identical output.
- Community Patch-only coverage checks that VPUI-only keys are not introduced.

These are fixture-based checks of the actual pack output, not an in-game rendering
test or a real DLL compilation.

Verification on 2026-09-15: the regression fails with the original omission and
passes with the fix; the full fast suite passes (431 tests, 24 ignored), and
`cargo clippy --all-targets -- -D warnings` passes. A separate scratch-only merge of
the real Release-5.4.6 Text File into a copy of the real game localization database
also restored the reported keys through its `Language_en_US` view and triggers.
That probe used a fixture gameplay database; it was not a full real VP merge.
`cargo build --release` also passed, and the resulting installer's `--help` runs.

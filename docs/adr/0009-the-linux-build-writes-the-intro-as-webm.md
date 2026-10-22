# The Linux build writes the Vox Populi intro as WebM, under the game's `.wmv` name

The 0.1.8 intro (ASF: WMV2 video, WMA2 audio, the format the game's own movie is in) played
on Windows. Under Proton it played as colour bars and a noise square.

## Why

Proton's media converter (Proton 9, 10, 11 and Experimental) intercepts `video/x-ms-asf` and
`video/mpeg` streams. It looks each one up by content hash in Steam's pre-transcoded cache
(`transcoded_video.foz`). A stream it has no entry for is replaced by a placeholder video,
`MEDIACONV_BLANK_VIDEO_FILE`: the colour bars. Brave New World's own movie is in that cache;
an edited intro never can be.

A WebM (VP9 video, Opus audio) under the same `Civ5XP2_Opening_Movie_en_US.wmv` name plays.
The game opens the movie through DirectShow, Wine identifies the stream by its content rather
than its extension, and WebM is not a format the converter intercepts. Confirmed in-game on
Proton 11.

## Decision

Each build carries one encoding of the same edit: the Windows build the WMV, the Linux build
the WebM (`VOX_POPULI_INTRO`, chosen with `cfg(windows)`). The game's file name is unchanged.

Both builds recognise *both* intros as the installer's own, by length and FNV-1a hash
(`INSTALLER_INTROS`). The 0.1.8 Linux build wrote the WMV, and a game still holding it must
not have it banked as the original when a later build replaces it. Recognising the other platform's
intro costs a constant, not a second embedded video.

## Consequences

- An intro re-edit adds its two encodings to `INSTALLER_INTROS` and keeps the old ones. A test
  pins the constants to the asset files, so neither can drift.
- The Linux binary is about 8 MB smaller: the WebM is 2.2 MB, the WMV 10.3 MB.
- Windows decodes WMV1, 2 and 3 natively, except on N editions without the Media Feature Pack,
  which cannot play the game's own movie either.

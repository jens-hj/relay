# Fonts

Zed Mono regular and bold are bundled from the official Zed fonts 1.2.0 release:
https://github.com/zed-industries/zed-fonts/releases/tag/1.2.0

These are the original Iosevka-derived Zed Mono faces, licensed under SIL OFL 1.1;
the accompanying license comes from `zed-iosevka/LICENSE.md` in that repository.

Reddit Sans regular, semibold, bold, and extra bold are bundled for titles and
subtitles from the official repository at commit
`aae51f87b9dc16ab78e8013c1f945dda85318ecc`:
https://github.com/reddit/redditsans/tree/aae51f87b9dc16ab78e8013c1f945dda85318ecc/fonts/sans/ttf

The unmodified static TTF files carry the weights used by Relay's headings;
they do not depend on variable-font axis support or installed system fonts.
They are licensed under SIL OFL 1.1; `Reddit-Sans-LICENSE.txt` is the upstream
`OFL.txt`, including the copyright notice. Both font families load from embedded
assets before the UI is built, so they are available offline on every client.

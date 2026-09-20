# Bundled font sources

- IBM Plex Mono and Newsreader: existing bundled faces; see their OFL files.
- Silkscreen, Bungee and Rubik Mono One: copied unchanged from nus-promo/public/fonts for the Dock font cycle; their original OFL files are included. Sequence and optical sizes match nus-promo/src/design/Wordmark.tsx.
- Victor Mono: https://github.com/rubjo/victor-mono — public/VictorMonoAll.zip, retrieved 2026-09-19. Original license retained in License-VictorMono.txt.
- JetBrains Mono: https://github.com/JetBrains/JetBrainsMono — fonts/ttf from the official master archive, retrieved 2026-09-19. Original license retained in License-JetBrainsMono.txt.
- ABC Areal, ABC Areal Semi Mono and ABC Areal Mono: user-supplied ABCAreal.zip, 2026-09-19. Static TTF faces copied unchanged; supplied licensing terms retained in License-ABCAreal.pdf.

The native app embeds Regular, Medium and Bold for each exposed family. Italic
faces are retained for future text-style support; they are not a synthetic effect.

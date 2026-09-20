# Design — "Broadsheet"

Chosen 2026-09-16 from three directions (canvas: design/, page 2 keeps the
record). Ink on paper, one monospace face, rules instead of boxes, and one
color on screen: the active Space's.

## Principles

1. Rules, not boxes. Hierarchy by weight and case, never by color.
2. One color on screen: the Space's signal. Everything else is ink on paper.
3. The terminal keeps its ANSI palette; the chrome stays monochrome around it.
4. Floating things get a 2px edge and a hard offset shadow. Nothing blurs,
   nothing is rounded, nothing is translucent.
5. Chromeless by default: a 6px signal band and a 30px top strip are all the
   furniture; the sidebar slides in on ⌘⇧S.

## Tokens

| token      | paper (light)          | ink (dark)                 |
|------------|------------------------|----------------------------|
| paper      | `#ffffff`              | `#141414`                  |
| ink        | `#141414`              | `#ece7da`                  |
| tint       | `rgba(20,20,20,0.06)`  | `rgba(236,231,218,0.07)`   |
| hot edge   | `rgba(20,20,20,0.12)`  | `rgba(236,231,218,0.14)`   |
| dim        | `#8a857a`              | `#8a857a`                  |
| page       | `#ffffff` (web content)| `#ffffff`                  |
| scrim      | `rgba(255,255,255,.55)`| `rgba(0,0,0,0.5)`          |
| caret      | the ink                | the ink                    |
| selection  | the ink at 22%         | the ink at 22%             |

Space signals (same in both themes): red `#c8102e`, blue `#1f5fbf`,
gold `#d9a400`, green `#2e7d32`, violet `#6b3fa0`, teal `#1a7f8a`.
Attention ("waiting") uses the Space's own signal as a filled label.

ANSI 0–15, paper theme:
`#141414 #b3261e #2e7d32 #9a6b00 #1f5fbf #8e3b8e #1a7f8a #8a857a`
`#4a4740 #d63a2f #3f9a45 #c48a00 #3b7ee0 #b04eb0 #22a3b0 #ffffff`

ANSI 0–15, ink theme:
`#141414 #e0574c #7ac77f #e5b94a #6ea3ef #d086d0 #6fd0da #bdb8ab`
`#5a564e #ff6f63 #93e39a #ffd06a #8fbcff #e9a0e9 #8be6ef #ece7da`

Terminal default fg/bg = ink/paper of the theme. Cursor: block, the
theme's caret on paper, no blink by default. Caret and selection are
tokens a theme may set per face (LOOK · TOKENS); the shell's selection,
the editor's selection and both carets draw from them, and the cursor
rule's other choices (signal, the tab's own) sit over the caret.

A program's own colours — truecolour and the 256 — are graded before
they reach the screen: any text under 4.5:1 against its background is
walked toward white or black, the way it leans, until it reads
(TERMINAL · PROGRAM COLOURS). THE THEME'S SIXTEEN snaps them to the
nearest of ours in Oklab, so a program wears the theme; `program(p)` in
rules.luau does either per program, gives one its own sixteen, or remaps
a colour it hardcodes.

## Type

- UI and terminal: IBM Plex Mono (bundled, OFL) by default. Regular 400, medium 500,
  semibold 600. Both are config keys: `font.terminal` and `font.ui`.
- Wordmark only: Newsreader Italic 500 (bundled, OFL) — `nus`, `go`,
  `quick`, `paper`, `ink`.
- Ramp: ui 13/1.5; ui strong 600; label 11 caps tracking 0.08em; palette
  input 16; preview 7.5/1.5; wordmark 34 (sidebar) / 20 (top strip) / 18.

## Rules, spacing, shadow

- Rule weights: 1 hairline (rows), 1.5 structure (sidebar edge, headers),
  2 floating (palette, quick terminal, PiP). 6 signal band.
- Shadow: hard offset only — 8×8 for palette/quick terminal, 6×6 for PiP,
  4×4 for the localhost chip. No blur radius, ever.
- Radii: 0 everywhere.
- Spacing: sidebar rows 12×14, pane headers 9×18, top strip 30, sidebar 272,
  browser split 520, palette 600, tab preview 52 tall.
- Inactive: 60% opacity. Selected: ink fill, paper text. Hover: 1px ink
  outline. Links: 1.5px underline, 3px offset.

## Surfaces

- **Top strip** (30): wordmark · `space · NN tab · cwd` · attention summary
  · ⌘K · window controls (— ▢ ✕). Present in every state; it is the
  drag region.
- **Sidebar** (272, ⌘⇧S): Space tabs as a ruled segmented row (selected =
  ink fill), numbered tabs 01–09 with 52px live previews, `+ new tab ⌘T`
  pinned to the bottom.
- **Panes**: terminal and browser are peers split by a 1.5 rule; each has a
  9×18 caps header. Browser URL is a 1px boxed field; devtools is a caps
  tab row under the page.
- **Palette** (⌘K, 600 wide, top 220): serif "go" prompt, 2px edge, 8×8
  shadow, selected row = ink fill.
- **Quick terminal** (⌥⌘T): 960 wide sheet from the top edge, no top
  border, 300 tall, serif "quick".
- **PiP**: 400 wide, 4px signal band, 2px edge, 6×6 shadow, caps footer
  with `space · NN`, title, ↗ (return to tab), ✕.
- **localhost chip**: anchored to the detected line, 1.5 edge, 4×4 shadow:
  `localhost:5173 · open split ⌘↵ · new tab ⌘⇧↵`.

## Keys

⌘K go · ⌘T new tab · ⌘1–9 tab · ⌘⌥1–9 space · ⌘⇧S sidebar · ⌥⌘T quick
terminal · ⌘↵ open detected URL in split · ⌘⇧↵ in new tab · ⌘D split ·
⌘W close. (Ctrl on Windows/Linux.)

# Design — complete appearances

Blueprint is the default appearance. The curated collection keeps the
Broadsheet structure: rules instead of boxes, clear type, and shared tokens
across chrome, terminals, editing, reading, and home artwork. New profiles
start in Blueprint; existing profiles keep their saved choices.

## Theme collection

The ten original themes form one flat list. Five use light text and five
use dark text. Familiar standards remain available alongside saved themes;
retired originals remain loadable for existing profiles and imports.

| Theme | Background | Text | Character |
|---|---|---|---|
| Blueprint | `#1f5fbf` | `#ffffff` | Cobalt, white rules, open corners |
| Canopy | `#125746` | `#f0f4cf` | Deep green and warm foliage |
| Carbon | `#000000` | `#f2f4f8` | OLED black, clear high-contrast text |
| Citron | `#efcf4b` | `#342532` | Yellow with plum lettering |
| Folio | `#efd0a8` | `#3f2a24` | Apricot with warm brown lettering |
| Indigo | `#292543` | `#edeaf4` | Quiet violet with pale text |
| Iris | `#c6b2e4` | `#392448` | Lavender with aubergine lettering |
| Lagoon | `#89d7ca` | `#143d48` | Aquamarine with deep blue lettering |
| Ledger | `#d4dfa9` | `#263c29` | Celery with forest lettering |
| Vermilion | `#a92e34` | `#fff3db` | Red with warm cream lettering |

Carbon, Indigo, Folio, and Ledger emphasize still surfaces and readable
editing colors for extended work. They receive no separate UI grouping.
Each original defines its foreground, background, ANSI colors, cursor,
selection, material, and artwork palette. Automatic terminal colors keep
these backgrounds intact and use accents instead. External pages and
programs may still draw their own backgrounds.

`spikes/composite/src/themes.rs` defines the collection. Blueprint captures
the user's cobalt appearance: 3 px white open-corner structure, no texture,
and white signal. Loading animation stays independent of theme selection. `blueprint.rs` supplies fresh-profile
and explicit APPLY BLUEPRINT terminal defaults: ABC Areal Mono Medium,
14 pt, 1.25 line height, 0.25 logical px tracking, and a 3 px underline
cursor with Glide and no blink. Theme selection applies a complete visual recipe: UI, terminal, code and prose
font pairings, size/leading/tracking/measure, cursor shape and movement, home
presentation and artwork, masthead, and visual motion register. These are a
whitelist in `appearance::VisualStyle`, not an import of the Behavior object.
Loading style, color, thickness and chase, sounds, reduced-motion overrides,
startup, privacy, routing and shortcuts remain independent. Saved looks and
system appearance snapshots retain the full visual recipe. Legacy imports
without that recipe retain the user's existing visual choices.

## Appearance behavior

An original is one complete look; Paper/Ink is no longer a competing
appearance switch. Follow OS chooses two remembered complete appearances,
configured as LIGHT SYSTEM THEME and DARK SYSTEM THEME. Direct theme
selection pins that look. The fallback pair is Folio and Blueprint.

The legacy serialized Paper/Ink palettes remain for saved-theme compatibility
and light/dark variants of standards. The palette source is stored separately
from the final text polarity: tinting a saved light palette cobalt must not
silently select or edit its other palette. Renderer polarity is derived after
background and contrast resolution. Custom colors survive a system round trip.

## Principles

1. Rules and typography establish hierarchy; color supplies identity and meaning.
2. Every theme is a coherent environment, with shared semantic colors across surfaces.
3. Main, secondary, and authored syntax colors remain readable on their backgrounds.
4. Themes use existing material and settings controls; no parallel theme engine.
5. Themes own the visual composition; functional preferences and loading remain independent.

## Legacy renderer base tokens

These are the Broadsheet fallback values underneath saved theme overrides,
retained for older/partial profiles. Fresh-profile colors are the Blueprint
values above, not this fallback palette.

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
the editor's selection and every caret draw from them — the shell's, the
editor's, the home line's and the palette's — and the cursor rule's other
choices (signal, the tab's own) sit over the caret. The home line and the
palette follow CURSOR's shape, blink and weight too: at SHELL they show
the bar a shell prompt shows.

A program's own colours — truecolour and the 256 — are graded before
they reach the screen: any text under 4.5:1 against its background is
walked toward white or black, the way it leans, until it reads
(TERMINAL · PROGRAM COLOURS). THE THEME'S SIXTEEN snaps them to the
nearest of ours in Oklab, so a program wears the theme; `program(p)` in
rules.luau does either per program, gives one its own sixteen, or remaps
a colour it hardcodes.

## Type

- UI and legacy terminal fallback: IBM Plex Mono (bundled, OFL). Blueprint's
  terminal default is ABC Areal Mono Medium as specified above. Regular 400, medium 500,
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

## One prompt, two surfaces

Home and Cmd/Ctrl-K share routing, result sources and action dispatch. Plain
words search by default; URLs visit pages, `>` runs a shell command, and `@`
opens an assistant draft. Explicit addresses remain addresses under every
preferred route. Both surfaces support Shift+Enter for a new window and
Cmd+Enter on macOS / Ctrl or Alt+Enter elsewhere for a new tab.

Home shows search and website entry before personal history by default, with
visible route labels and an empty-field hint. Assistants remain available
when typing or explicitly choosing an assistant-oriented preset. Only the
exact previous Mixed source arrangement migrates to the balanced default;
custom source lists, saved commands and search preferences remain intact.
Keyboard-selected home results reveal themselves in short panes, field edits
use the focused pane, and Cmd/Ctrl-K carries a home draft into the palette.

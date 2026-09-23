# Import and saved commands

The welcome profile flow now ends with an optional import page. “Import from”
is one sentence control: its icon and application name use Station split-flap cells. Each cell follows
the preceding cell across the sentence. Pointer hover and keyboard focus pause it; reduced motion
keeps a static name. Tab focuses the sentence and Enter opens the chooser.
The same page is available under Settings → Profile → Import from.

This first implementation accepts explicitly chosen local exports. Browser
bookmarks in HTML or Chromium-style JSON become a sidebar folder; only HTTP(S)
links without embedded credentials are accepted, and repeat imports skip
duplicates. Theme exports already supported by nus (Ghostty, VS Code, Windows
Terminal, base16) are normalized to colors only and added to Appearance imports.
The chooser names Arc, Dia, Safari, Chrome, Terminal, WezTerm, kitty, Alacritty,
Ghostty, VS Code, Cursor and Zed; it states the supported export formats before
opening a file. Listing an application does not imply its proprietary session
database or full configuration can be restored.

Reading an export builds a review without changing the profile. Import commits
the reviewed contents locally. It does not open links, evaluate configuration,
copy authentication state, or resume processes. Each file is bounded to 8 MB;
bookmark imports must contain fewer than 5,000 distinct accepted links. Live
tab/session discovery, project recents and general terminal configuration
translation remain future importer work.

Saved shortcuts have evolved into **Saved commands**, keeping the existing
stored routes intact. They use a bookmark mark, theme accent, name, command
preview and explicit Insert/Run/Open/Review action label in the palette and
Home. The collection has a full native Settings page with compact controls to
insert/open, run a shell command, name, edit, copy, reorder or remove each item.

Settings → Prompt controls the saved source's order, limit, visibility before
typing and visibility during search. Command previews, the persistent collection
entry, and shell activation behavior are configurable there and on the collection
page. Exact saved-name matches rank ahead of the shell fallback when the source
is enabled. Choosing a source preset preserves saved routes, names and behavior.

Shell commands default to insertion into a new terminal for review. No newline
is submitted. Commands containing control characters or line breaks cannot use
Insert; Run remains an explicit action. Assistant shortcuts retain their existing
review step, and web shortcuts open normally.

`scripts/check-import-saved.py` exercises real native drawing, source toggles,
named matching, Settings bindings, import idempotence and command insertion using
disposable profiles. Its screenshots include Paper, Ink, the reel mid-turn and
reduced motion. Unit tests cover import filtering and normalization, legacy saved
configuration, and control-character rejection.

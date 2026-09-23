# Documents, reading, and PiP

First arrival now uses the [14.6-second hyperdrive sequence](HYPERDRIVE.md):
independent background stars accelerate and brake, a separate constellation
forms the n, and an orbiting star inks the transparent workspace into view.
Click, Enter, Space or Escape continues immediately. Reduced motion and Still
use a brief stationary mark; None skips the introduction. Completion and skipping
share the persistent arrival marker. Secondary windows do not replay it.

## One reading list

The pinned Reading list, command palette and welcome page use the same
`profile/library` store. The starting nus.dev link is an ordinary item seeded
once; removing it is remembered. Existing saved copies and reading positions
remain compatible.

Use **Add item**, or right-click the list's background, to add a web link, an
absolute local path, or a note. A note has a title and text, with its link field
left empty. Right-click an item to edit its title, link and personal notes, open
its original or offline copy, archive it, mark it finished, or remove it. Right-
click a browser page or document to save it to the same list. Search includes
personal notes as well as title and source.

The form uses Tab/Shift-Tab to move between fields and actions, Ctrl/Cmd+Enter to
save, and Escape to cancel. Notes accept multiline typing and paste. Saving an
edit checks the revision shown when the form opened; concurrent edits are not
overwritten. Source changes invalidate that source's old offline copy and
position. Editing a link does not fetch it. Notes are stored locally; an offline
article's personal notes appear above its saved text.

## File viewers

Markdown (`.md`, `.markdown`, `.mdown`), JSON, CSV/TSV, and text/log files opened
from Files or a local file URL can use the built-in viewers. Explicit editor
commands still open the source. A document's context menu includes **Edit
source**. Reload reads the current file from disk.

The address remains the original file URL. Relative Markdown images and links,
including `../` paths and encoded filenames, resolve against that file. Links to
other supported documents use their viewer too. Markdown includes tables, task
lists, code fences, footnotes, heading anchors and an optional contents list.
HTML written inside Markdown displays as text. JSON has collapsible objects and
arrays and a complete formatted source view. CSV handles quoting, multiline
cells and optional headings.

**Settings → File viewers** controls:

- A master switch and a switch for each format.
- App colors, always-light Paper, or always-dark Ink.
- Body font, text size, page width, line spacing and long-line wrapping.
- Contents, local images, remote images and CSV headings.

Local images are on by default. Remote images are off until enabled; enabling
them permits requests to the linked servers. Document scripts, frames, forms,
and embedded active content are blocked. Theme-following updates preserve the
current scroll position. Rendering preference changes reload open documents.
Files are bounded to 8 MiB and UTF-8. Tables are bounded to 10,000 rows and
100,000 cells. Oversized, malformed or unreadable files show an explanation and
remain available through Edit source. JSON's tree has a 20,000-node display
budget; its source remains complete.

## PiP

The floating window follows the stream's intrinsic aspect ratio, including
portrait video and source changes. Edge, wheel, pinch, and native resizing keep
that ratio; pixel rounding does not accumulate into shape drift. CSS player
letterboxing is excluded from the sampled picture, and the top band overlays
the picture. `scripts/check-pip-aspect.py` checks these paths with local streams.

**Settings → Browser → Picture in picture** has independent switches for opening
PiP when leaving the app or video tab, and closing it on app focus, window click,
window restoration, or return to the video tab. Both automatic opening switches
are on by default; all return-close switches are off.

Leaving the app waits briefly for native focus to settle, ignores focus moving
to PiP or another auxiliary window, and tolerates a late media report. Manual
close cancels pending automatic requests. Existing transport, placement and
single-PiP-per-process behavior are retained. macOS uses a non-activating,
always-on-top window that remains visible across Spaces.

## Checks

- `cargo test --manifest-path scripts/library-check/Cargo.toml`
- `CEF_PATH=$PWD/vendor/cef cargo test --manifest-path spikes/composite/Cargo.toml --bin composite hyperdrive::tests`
- `python3 scripts/check-documents-reading-pip.py /path/to/nus.app`
- `python3 scripts/check-reading-arrival.py /path/to/nus.app`

The native focus regressions deliver application focus events. Actual OS
application switching, minimizing and restoring require a desktop check in
addition to these deterministic cases. Windows and Linux require their own
native runs.

### Verification on this change

The isolated native checks passed for all four formats, local Markdown images
and linked documents, theme override and format disabling, reading add/edit/
cancel at 1200 and 480 logical pixels, PiP keep/close return policies, and the
introduction in Paper and Ink. Screenshots were inspected. A desktop switch to
Finder created PiP; returning to nus kept it open. Minimize/restore and actual
Windows/Linux app switching remain platform checks.

The production-module harness passed 75 tests. The complete app unit run passed
242 tests with one ignored; three TLS-server tests failed inside the sandbox,
then passed in a six-test rerun with local socket access. The arrival timing
regression also passed. The macOS review bundle was built and ad hoc signed in
`/tmp/nus-feature-review.app`; the repository’s existing distribution bundle was
not replaced.

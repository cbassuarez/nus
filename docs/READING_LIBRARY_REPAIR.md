# Reading library: local implementation repair

This change extends the existing `library.rs` / `HomePane::library` implementation.
It does not install the earlier `reading.rs` overlay, replace the Home pane,
remove `mod celestial`, or change startup, artwork, theme, terminal or browser
architecture. The existing Library and SaveReading actions are retained. The
only additional application action is RefreshReading.

## Use

Open the command palette and choose **Reading library**, **Save to reading
library**, or **Refresh saved reading copy from the open original**. Saving does
not change the active tab. A repeated save preserves the existing copy, title,
reading position and archive state; refresh is deliberate. Source pages/files
remain open. New saves from incognito are refused.

The library has Unfinished, All saved, Finished and Archived views. Search is by
title and exact source text. Up/Down select an item; Enter opens its saved copy.
Tab walks visible controls. Escape clears the list's search, or returns from a
saved article to the list. The older `archive ...` search prefix still works.

A missing or invalid copy leaves its link visible and shows a reason. Opening
that item does NOT silently open the original. **Open original** is explicit,
and web originals/links use the recorded browser container. An unavailable
container is not silently substituted with another sign-in. Legacy web entries
predate container metadata; they retain the old personal-container interpretation.
Editor notes without a path have no original to reopen.

Within saved reading, Cmd/Ctrl+F finds text, Enter advances and Shift+Enter goes
back; Escape closes find first. Cmd/Ctrl+C copies a selection, Cmd/Ctrl+A selects
the article's rendered text, and Cmd/Ctrl+plus/minus/0 changes/reset reading size.
Arrow/Page/Home/End/Space keys scroll. Shift+horizontal arrows extend a selection
on UTF-8 boundaries. Shift+wheel or horizontal wheel scrolls preformatted code
and tables horizontally. **Copy code / table** uses the original block text,
including tabs and trailing/newline whitespace, not its display expansion.

Finished and Archived are independent, explicit states. Viewing or scrolling to
the end never automatically marks an article finished. Removal asks first; Undo
is guarded against a later conflicting change. Re-saving a removed source can
restore it after relaunch. **Removal is not secure deletion**: snapshots and
removal records are retained. These files are not encrypted by this module.

## Compatibility and disk format

The existing stable identifier algorithm and original personal/file namespace
are retained. Existing `profile/library/<id>.json` and `<id>.article` files are
read in place; simply reading or duplicate-saving a legacy entry does not rewrite
it. The existing externally-tagged Article/Block serialization remains readable.
Only additive metadata fields and a Link block variant are introduced.

New/updated snapshots are immutable content-addressed JSON at
`profile/library/objects/<sha256>.article`. A record's `snapshot` reference is
updated only after its object has been written. Existing legacy article files
are not replaced or removed. Hashes detect damage; they are not an authenticity
signature. Unknown metadata fields are retained; unknown schemas or block types
are refused rather than discarded. Metadata uses a bounded single-record
read/modify/replace, under a nonblocking OS file lock shared by cooperating
windows/processes. Lock failure is visible rather than a UI wait. Progress writes
modify only position fields and require the snapshot version still to match.

The locking code requires Rust 1.89 or newer. It uses existing production
`serde`, `serde_json`, `ring`, `tempfile`, `getrandom` and `png` dependencies; this
patch does not change the production Cargo manifests or lockfile.

Full-library scans run on a read-only worker; an older scan is not published over
a local mutation. Small metadata writes, snapshot opening and bounded image
upload still run synchronously. The active library periodically refreshes other
windows' changes. This is not a new profile-sync protocol: a client or sync tool
that ignores the writer lock may still conflict. New object-directory sync,
older-application downgrade behavior and multi-device conflict resolution are
not certified here. Back up the active library directory before the first run.
Reversing the source patch does not reverse profile changes; old builds do not
understand new snapshot references/Link blocks.

## Capture and reader boundaries

Extraction uses a tightened version of the project's own read-only heuristic,
not Mozilla Readability. It does not modify the live DOM. Extraction has explicit
node, block, text and link budgets. Overflow fails instead of silently truncating
an article. The page is untrusted: its data becomes native text/blocks, never
privileged HTML or script. The existing 1 MiB CDP result limit is unchanged.
Both capture identity and a second document-epoch check protect against a same-
URL reload; timeouts or a closed/moved source leave the link and older copy intact.

Only already-loaded, origin-clean images can be copied using a detached canvas.
No fetch, independent downloader, hidden browser or additional image request is
used. At most 12 images, 2,048 pixels per edge, 1,048,576 pixels per image,
2,097,152 total pixels, 64 KiB per encoded PNG and 192 KiB total PNG bytes are
accepted. Inspected JSON is also checked against the existing CDP budget. Large,
tainted, unavailable or unsupported images are identified as missing. Cross-
origin images without suitable origin permission cannot be bypassed. Saved
pictures are bounded and decoded locally; opening a snapshot initiates no
network work in this library module.

The existing reader is extended, not duplicated. Saved copies use its Newsreader
layout. Preformatted blocks use the configured fixed-width terminal font; the
interface font can be proportional. Reader layout caches include font identities
and scale. Paragraph identity, a source-text offset and surrounding quote context
support resume/reflow; ambiguous matches use an explicitly approximate fraction.
Image sizes are resolved before layout. Links are separate actionable references,
not full inline rich text. Tables are preformatted, with an explicit merged-cell
fallback. General text selection uses rendered runs; exact source whitespace is
provided by the code/table copy action. Unicode capture and UTF-8 boundaries are
tested separately from shaping, bidirectional layout, grapheme navigation and IME,
which still require native acceptance. Find highlights the matching rendered line.

Accessibility augments the existing AccessKit tree/dispatcher with named controls,
stable library IDs, text inputs, list options and article blocks. This is not a
claim of complete platform text-selection accessibility or tested VoiceOver/NVDA.
No browser-side page context-menu change is included: the existing palette
provides the save action. Reading size and the saved reading tab are not newly
session-persisted. No annotations, recommendation feed, AI, TTS, Readability
vendoring, automatic object garbage collection or celestial/welcome redesign is
part of this repair.

## Validation gates

The standalone test workspace below compiles the actual production storage file,
not a mock. Its tests cover legacy data, exact identity, independent states,
writer conflicts, interrupted commits, corruption, navigation capture generations,
delete/undo races and positions. Reader source-span tests live in the native app.

```sh
cargo test --manifest-path scripts/library-check/Cargo.toml
cargo test --manifest-path spikes/composite/Cargo.toml --locked
```

For browser fixtures, use Python with Playwright and Pillow and an installed
Chromium. Pass its actual executable on the local platform:

```sh
python3 scripts/library-check/test_capture.py --chromium /path/to/chromium \
  --report /tmp/nus-library-capture-results.json
```

The default suite uses two temporary loopback HTTP origins. `--offline-dom`
exists only for managed environments that prohibit loopback browsing: it skips,
not simulates, the independent-origin case. It does not bypass browser policy.

Build the native application with the project's existing bundle procedure, then
run the existing startup/settings checks. Before release, exercise this sequence
with a temporary profile: save an article; wait for the offline acknowledgement;
close its original browser; open the saved article; select/copy/find/resize; quit;
restart offline; reopen and resume; explicitly mark finished/archive/unarchive;
remove/cancel/undo; refresh while another window changes progress. Check a missing
copy, non-article/login page, large/tainted images, a same-URL reload and a closed
capture source. Confirm click/keyboard routing in both sides of a split and under
menus, narrow/wide layouts, paper/ink, focus, VoiceOver/NVDA and reduce-motion.
Verify native builds on each shipping platform and actual large-library timings.

Git application tests are not Rust compilation. JavaScript fixtures are not native
CEF/GPU or accessibility acceptance. Keep those outcomes separate in release notes.

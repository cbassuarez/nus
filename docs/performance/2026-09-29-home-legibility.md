# Native Home legibility acceptance

The shared Home reading area must keep the input, placeholder, results, saved
command details, selection, caret and route controls readable across artwork and
application palettes. This protocol checks the running native bundle. It does
not infer visual acceptance from a successful compile or a theme-token test.

## Current evidence status

The capture harness is prepared; a candidate bundle containing the shared
legibility implementation has not yet been tested with this harness. Native
execution, image review and any performance comparison remain pending. A
`prepared-not-run` manifest is a plan, not a passing result.

## Run the isolated capture suite

```sh
python3 scripts/check-home-legibility.py /absolute/path/nus.app --out /private/tmp/nus-home-legibility-candidate
```

The output path must be new. `--case wide narrow night private` selects cases;
omitting it runs all four launches and produces 35 expected pictures.
`--prepare-only` writes the profiles, shot scripts and manifest without launching
the app. The helper does not build or re-sign the supplied bundle.

On macOS the launch needs native WindowServer/Metal access; run through the
approved native execution path when a sandbox prevents that access. Each case
gets a new `NUS_SHOT_DIR`, profile and temporary directory. The caller's normal
profile is not reused. Input is dispatched inside the test app; no AppleScript
or OS keyboard injection is used. A timeout terminates only the process group
created for that case. Cases and failures remain on disk for inspection.

| Case | Coverage | Expected pictures |
| --- | --- | ---: |
| Wide, 1440 × 900 | Space, Pond, Brain, Memphis, Sky, None/Line and NUS N/Plate, each in Folio, Carbon and Blueprint | 21 |
| Narrow, 480 × 720 | Every look with the top/wide prompt and scrolled saved results; extra Space/Sky text selections | 9 |
| Night, 1440 × 900 | Local-night Sky against Folio and Blueprint | 2 |
| Private, 480 × 720 | Wrapped privacy note and input on constrained plain Home in three palettes | 3 |

The wide case shows placeholder and saved-command cards in Folio; a typed query
and the last selected result in Carbon; and an entirely selected long URL in
Blueprint. Twelve synthetic saved links provide repeatable result expansion.
The app's `homevisible` assertion proves the chosen last result has a visible hit
rectangle. No saved link is opened, submitted or sent to a service.

The script applies each curated palette before restoring the requested look,
since a palette can carry its own preferred artwork. `startpage prompt` saves
that choice before the existing `asserthomeart` checks renderer health and
persisted selection. `input` types through app keyboard handling, followed by
`key cmd+a` for selection. Outside recordings, the similarly named `type` command
targets a terminal and is deliberately not used. The existing API does not
provide a prompt selection-range assertion, so the selection itself needs
review in the image.

Astronomical time and location are fixed: latitude 40°, longitude −105°, daytime
2026-09-22 19:00 UTC and night 2026-09-23 07:00 UTC. Weather consent stays off.
Reduced motion is enabled. These are repeatable scene conditions, not a promise
of byte-identical pictures across fonts, GPUs or builds. Native captures are
offscreen scene readbacks, not photographs of display scanout or HDR output.

Incognito is launched with `--incognito` and `NUS_PRIVATE_LOOK`; the product
constrains its behavior and creates a separate temporary root. `privatecheck`
asserts that route and the helper confirms cleanup. This case tests the privacy
notice on plain Home, not an unsupported private-artwork configuration.

## Retained provenance and assertions

`manifest.json` records executable and harness SHA-256 values, host information,
script and initial-profile hashes, every expected capture, scene clock, logical
size, palette and state. Actual PNG dimensions, hashes and sizes are recorded
after success. The executable is hashed again after the suite. Changed binaries,
missing or extra captures, incorrect aspect/scale, unknown commands, panics,
native assertion failures and GPU validation failures fail the check.

Each case retains `check.shot`, `profile/settings.json`, `run.log`, temporary
files and screenshots. The private application's own profile is intentionally
removed on clean exit; its initial fixture remains in the evidence directory.
The `-paper.png` suffix is a transport label fixed when the shot runner starts;
the capture's `appearance` field and native assertion identify its actual mode.

## Visual review gates

- Inspect input, placeholder, saved title/detail/action badge, selected and
  unselected results, route labels/icons, selection text and the visible caret
  core. The protected area must include all functional content it claims to
  protect, without clipping the final visible row or wrapped incognito note.
- Inspect bright clouds, stars, fish, diagram connectors and authored Memphis
  pieces near glyphs. A feathered edge should lie outside the required reading
  bounds; a pleasing panel average does not excuse a bright shape under a letter.
- Preserve each artwork's identity. None, Memphis and NUS N may avoid additional
  tonal treatment when their existing composition meets the same text contract.
  Catalog stars must retain their actual positions.
- Compare narrow and wide captures at native scale. Do not judge small text only
  from a scaled thumbnail. Confirm selection remains distinguishable while its
  text remains readable, and that accent-only indicators do not disappear.

Normal functional text should meet at least 4.5:1 against its final composited
background; the primary input may target 7:1. Meaningful non-text indicators
should meet 3:1 against adjacent colors. These thresholds come from
[WCAG text contrast](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html)
and [non-text contrast](https://www.w3.org/WAI/WCAG22/Understanding/non-text-contrast.html).
The primary 7:1 target is a product preference, not the minimum required here.

The capture helper deliberately computes no screenshot WCAG score. Antialiased
glyph-edge pixels are blends, not the specified foreground color. A numerical
contract test must use the resolved foreground including opacity, the actual
layer order and guaranteed background range, with selection/card washes
included. Renderer color-space behavior must agree with that calculation.

## Remaining acceptance beyond these stills

Motion needs a separate sequence: cloud highlights, star/meteor crossings, Pond
ripples and diagram updates must not defeat the protected region or cause
foreground/scrim pumping. Also exercise the first frame of result expansion,
scroll changes, IME composition, focus changes, hover, custom low-contrast
palettes and different display scaling. These are not claimed by the still
suite. Root and `spikes/composite` contract tests remain separate evidence.

No GPU readback, fullscreen blur or continuously running analysis should be
introduced merely to maintain prompt contrast. Measure any new draw cost with
the same optimized bundle, size, artwork and power state, separately from these
screenshot runs. CPU submission time does not establish GPU execution time or
battery consumption. No such performance measurement is recorded here yet.

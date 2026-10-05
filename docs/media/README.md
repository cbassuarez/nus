# README media

The October 4, 2026 README uses native macOS captures from the same Hello World
project shown on the nus site. The images retain their original PNG pixels;
the GIF and H.264 videos are the site's existing exports, copied without
re-encoding. All files are local to this repository so the README also works
from a checkout.

| File | Content | Format |
| --- | --- | --- |
| `readme-browser.gif` | Page → counter → native Find → source → kept page | 960 × 652, 12 fps, 14 seconds |
| `readme-browser.mp4` | The same continuous browser workflow | 1600 × 1086, 30 fps, H.264 |
| `readme-shell-page.mp4` | Start a server in the shell, open its page, click the counter | 1600 × 1086, 30 fps, H.264 |
| `readme-browser.png` | Browser video's full-resolution poster | 2240 × 1520 PNG |
| `readme-workspace.png` | The development server beside its working page | 2240 × 1520 PNG |
| `readme-editor.png` | The counter's JavaScript in the native editor | 1433 × 957 PNG |
| `readme-hatch.png` | A running server and finished project checks | 1920 × 936 PNG |
| `readme-home.png` | Saved commands, project and resumable session | 2105 × 1246 PNG |
| `readme-history.png` | Searchable commands, output and navigation map | 1176 × 1208 PNG |

Capture uses the application's `NUS_SHOT` compositor, disposable profiles,
the Blueprint theme, cleared hover hints and disabled port toasts. The lossless
recordings run at a fixed 60 fps. These are paced demonstrations, not latency
measurements. The videos are silent, H.264, yuv420p, CRF 19, with faststart;
the GIF uses a 128-color palette.

`manifest.json` records the imported names, original names, dimensions,
durations, build hash, file sizes and SHA-256 hashes. The two copied provenance
files retain the original capture and interaction checks, script hashes and
export hashes from `nus-site/assets/films`. They identify a capture build at
`765a4d201493b0e0c682c7d5beb305fc32a6df5c` with capture changes; this is not a
claim that the frames came from the packaged `v0.0.3-preview.1` release.

The earlier September 18 images remain available for historical references.
Their original manifest is `manifest-2026-09-18.json`. `shots.txt` and
`scripts/shots.ps1` describe that earlier capture set; rerunning them does not
reproduce these October recordings.

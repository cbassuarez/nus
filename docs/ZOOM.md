# Zoom rendering

Browser pages and formatted file viewers ease between Chromium page zoom
levels. Keyboard, application-menu, and site-panel controls share percentage
steps and accumulate repeated input against the pending target. Every frame
requests a new Chromium render; nus does not magnify a saved page screenshot.
The motion preference controls timing, and reduced motion applies the target
immediately, including when enabled during a transition. Navigation cancels a
pending transition.

Display-density changes explicitly notify Chromium to refresh its screen
information, even if the logical viewport size stays the same. The main page,
embedded developer-tools pane, and little browser window use that path.
Raster images remain limited by their source resolution. SVG and text can be
rendered at the current output resolution.

Native terminal, editor, and app-page zoom still use their existing discrete
sizes and rasterize text at the selected physical size. This change does not
add interpolated native font sizes or stretch their glyph atlas.

`scripts/check-zoom.py /path/to/nus.app` uses a disposable profile and a local
Markdown/SVG fixture to check real intermediate page zoom, rapid input,
settling, reduced motion, and physical backing-texture dimensions. It also
captures native screenshots at 100% and 150%. Physically moving the window
between monitors with different densities remains a hardware check.

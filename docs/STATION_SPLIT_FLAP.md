# Station split-flap

The welcome import sentence and port board share a native Station renderer.
Each row has an independent transition clock. Flaps within that row follow
the preceding flap by 23 ms; the cascade continues across column boundaries.
Rows never wait for another row to finish. Unchanged characters remain still.
Glyphs are rasterized at the display scale, then their clipped halves turn
around the centre hinge with perspective. Settled lettering is drawn whole
so the seam does not obscure it. Reduced motion shows the current value
immediately; import hover and keyboard focus pause its rotation.

Port wording distinguishes observations from outcomes:

- TCP listeners and UDP bindings are counted separately.
- A wildcard address means “All interfaces”, not proven network exposure.
- Specific addresses are distinguished from loopback and unknown addresses.
- “Process age” comes from process start time; it is not server uptime.
- “Stop requested” does not claim success. The confirmation describes the
  existing three-second escalation to force-stopping the process tree.
- “Run saved command” describes command submission, not guaranteed restart.
- Import copy names the supported saved-link and color-theme imports.

`scripts/check-station.py /path/to/nus.app` checks row independence and captures
native import, mid-turn, port board, and detail states using disposable profiles
and fixture processes. It never stops a real process. Network reachability and
HTTP application health are not established by observing a listening socket.

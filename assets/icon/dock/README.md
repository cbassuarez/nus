These six startup frames come from the same clipped desktop-icon renderer as
the running app. They are embedded so launch never waits for runtime rendering.

Regenerate with the composite `dock` example, then copy `face-0.png` through
`face-5.png` into this directory. The native unit test compares their decoded
pixels with the renderer and fails if the artwork has drifted.

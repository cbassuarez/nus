These six startup frames come from the same Blender material-field renderer as
the running app. They are embedded so launch never waits for runtime rendering.

Regenerate with `cargo run -p nus-render --example icon -- assets/icon`.
The native unit test compares their decoded
pixels with the renderer and fails if the artwork has drifted.

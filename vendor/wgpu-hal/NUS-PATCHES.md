# Local wgpu-hal patch

Base: crates.io wgpu-hal 30.0.1, unchanged except the two Metal source files.
License: upstream MIT / Apache-2.0 (included).

Backport of https://github.com/gfx-rs/wgpu/pull/10302 (commit cca0369).
A newly created NSWindow can report itself occluded until the first drawable
is presented. Remember whether the window has ever been visible and enforce
the occlusion guard only after that. This allows the first frame while keeping
the existing protection against blocked drawables when a window is covered.
The upstream atomic import is adapted to core::sync::atomic for version 30.
The changed logic is macOS-only. Other backends are unchanged.

Both the root workspace and the composite workspace use this path patch.
Remove the path overrides and this directory after upgrading to a released
wgpu-hal that includes the fix; rerun the native PiP first-frame regression.

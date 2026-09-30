# Space: native Limb / Darkroom

This replaces the stock `space` chart with the recovered bivalent artwork.
There is one saved artwork key, `space`, and two views inside it. There is no
second legacy-chart picker entry, CEF page, HTML Home mock, or static wallpaper.

## Source and scope

The source is the uploaded visualization built by `build-bivalent.py` from
`limb-darkroom-template.html`, `bivalent-renderer.js`,
`constellations-renderer.js`, and the three data scripts. The generated source
HTML had SHA-256
`3b69f0907ee8fa12c4067f145a82fa7dff87a8f7f8482467163b541a7cbf83d1`.
Original provenance is preserved in `spikes/composite/assets/art/space/`.
`NOTICE.md` is included in the binary and available from the identity tooltip;
it is also retained next to the packed star data. The images describe historical
NASA composites, not live weather. The sky is a composed Orion view, not a
claimed observation from the user's current location.

The baseline is the user's supplied composite `src.zip`, `assets.zip`, build
files, and `nus-render-current.tar.gz`. A downloadable patch's preimage manifest
identifies these exact bytes; do not assume that a public commit identifies an
uncommitted local tree. No dependency or lockfile changes are needed.

The native reading material's opacity, HDR fallback and geometry contract are
not changed here. `scene.rs`, `quad.wgsl` and `home_contrast.rs` remain unchanged.
The prototype's separate CSS reading mask is intentionally not copied. Its
artwork edge gradient is retained. The current overly opaque native reading
surface will therefore still be visible until its separate correction lands.
No browser heartbeat, startup, sandbox or default-browser code is changed.

## Runtime integration

* `assets/art/space.luau` calls `c:orbital({})` and declares a dark backdrop.
* `art.rs` adds `Cmd::Space` and dispatches it to `App::draw_space_cmd`.
* `space.rs` in the render crate owns the wgpu resources and per-view cache.
* `space.wgsl` renders cached dust, analytic textured Earth/cloud shells and
  atmosphere, then instanced catalogue stars/figures, then authored edge shade.
* `space_motion.rs` is a pure monotonic-clock policy with no native or GPU deps.
* Composite `space.rs` decodes the four bundled JPEGs using the existing codec,
  supplies real native label typography and the two Home controls.
* `home.rs` supplies each Home's motion state and its actual reading geometry.
  Settings previews use the same scene path and fixed initial pose.
* `access.rs` exposes real controls and prompt focus; `shot.rs` adds native
  scenario assertions. Both the binary and Windows DLL include these modules.

The exact retired stock script supplied in this conversation is recognized by
SHA-256 and superseded in memory, including when present as `profile/art/space.luau`.
Its bytes stay on disk. An edited/custom file is not silently erased or
classified by a loose phrase match. A different historical chart version is not
recognized automatically; inspect it before adding a known hash. A modified
user script may intentionally keep its own rendering until the user changes it.

The Luau orbital operation is restricted to two instances per canvas; native
Home uses one. Parameters are finite/clamped. Source scripts do not control GPU
buffer layout, arbitrary image paths, network loading or shader source.

## Authored motion

Starts in Earth/Limb. Settled images do not advance camera/cloud time. After
65 eligible seconds, the view turns over 18 seconds. Look outward / Return to
orbit takes 10 seconds. The transition uses the recovered quintic smooth curve.
Hold pauses automatic motion and retains remaining idle time. A deliberate view
change while held may move the blend but does not advance cloud/camera time.
Typing and composition hold motion; typing leaves a six-second quiet interval.
Reduced motion disables automatic movement and snaps explicit view changes.
Hidden/unfocused/modal-blocked Home content does not accrue unattended motion.
Large host timing gaps do not cause a camera jump; transition steps are capped
at 120 milliseconds, matching the prototype.

F6 enters/leaves the artwork controls. Arrow keys choose a control; Enter/Space
activates. Tab/Shift-Tab traverse those controls back to the real prompt. The
real prompt handles text, command execution and search as before. Clicking a
control never passes its title to the shell. Wide and narrow native footer
layouts reserve space below the actual result rows. Existing palette, dialogs,
profile UI, startup surface and other blocking content retain input ownership.

## Rendering and adaptation boundaries

The port retains the recovered Earth/global/detail and cloud/global/detail
bytes, geo mapping, sun/camera equations, dust field and catalogue data. GLSL
bottom-left pixel coordinates are converted explicitly to native top-left
coordinates. Texture JPEG rows are not flipped. Maps use Rgba8Unorm because the
source shader explicitly decodes/tone-maps its color; an sRGB texture would
apply that conversion twice. Mipmaps are built once; Earth uses 8x anisotropy,
cloud maps 4x, dust linear sampling. Catalogue data remains 9,827 records, 674
segments and 88 label candidates.

GL point sprites and lines become instanced native quads. Labels use nus's own
font system instead of a Canvas2D text atlas, with measured clipping and actual
prompt avoidance. The catalogue-star quiet region uses current Home geometry,
not the prototype's hard-coded UI rectangle. These are explicit native
adaptations; no claim of pixel-identical output is made before GPU comparison.
The floating prototype controls become native footer controls.

Output longest dimension is at most 1,200 pixels. Dust longest dimension is at
most 720, with the recovered 35% overscan. Identical parameters and output size
reuse the rendered texture without a queue submission. Dust only rebuilds for
resize/seed changes. Images, pipelines and catalogue buffers are shared per App
GPU, with up to four per-view caches and idle trimming. Each Home owns its
camera state; thumbnails cannot move it. Fullscreen, split and HiDPI composition
continue through the existing Scene texture path.

Initial JPEG decode, mipmap creation and pipeline creation are lazy. They can
cost a first-use frame; asynchronous prewarming and adaptive image budgets are
not implemented by this repair. The caches limit repeated work, not initial
GPU compilation latency. Resource validation errors have a native diagnostic
surface, but shader/device validation failures still need the native gates below.

## Tests and harnesses

### Available without a Rust toolchain

```
python3 scripts/check-space-source.py
```

These are source/data contract checks, not Rust compilation, WGSL validation,
GPU tests or visual parity. They verify exact image/catalogue bytes, image
header dimensions, catalogue indices, selected API/interface strings,
registration shape, bounded cache intent and attribution retention.

### Native compilation and policy tests

From the complete nus checkout:

```
python3 scripts/check-space-policy.py
source scripts/env.sh
cargo test --locked --release -p nus-render space
cargo check --locked --manifest-path spikes/composite/Cargo.toml --bins --lib
cargo test --locked --release --manifest-path spikes/composite/Cargo.toml --bin composite
cargo build --locked --release --manifest-path spikes/composite/Cargo.toml --bins --lib
```

The standalone policy runner compiles the actual `space_motion.rs` with rustc.
The renderer tests validate `space.wgsl` with the existing Naga 30 dev dependency,
then exercise ABI/layout, catalogue integrity, image sizing and mipmaps. Compile
the Windows DLL target as well as the executable; do not edit Cargo.lock merely
to silence a source problem.

### Native GPU readback (explicit opt-in)

```
mkdir -p /tmp/nus-space-gpu
NUS_SPACE_CAPTURE_DIR=/tmp/nus-space-gpu \
  cargo test --locked --release \
    --manifest-path spikes/composite/Cargo.toml --bin composite \
    native_space_gpu_real_assets_and_cache -- --ignored --nocapture
```

This uses the actual native backend (Metal, DX12 or Vulkan), real JPEGs,
WGSL pipelines, catalogue and GPU readback. It saves Earth, Darkroom, mid-turn
and narrow Earth frames, checks opacity/nontrivial output, stable reuse, dust
reuse across turns, resize rebuild and independent views. Output differences
are necessary, not sufficient, for visual parity. Compare against the recovered
prototype/reference; do not label a nonblank image a faithful render without
looking at it.

### Real native Home app-handler scenario

```
python3 scripts/check-space-native.py /absolute/path/to/nus.app
```

Windows: pass the packaged top-level GUI bootstrap `nus.exe`, never `bin/nus.exe`.
Linux: pass the packaged `nus` launcher. The wrapper makes a new app profile,
runs `tests/space/native-home.shot`, imposes a timeout and retains evidence. It
checks all expected captures and records launcher + application payload hashes
when present. It does not change the OS's default browser. Nevertheless an app
profile is not an OS sandbox; use a disposable user/session for release QA.
The scenario uses native application handlers and offscreen captures, not
injected OS input or display scanout. It exercises both views, Hold, native
keyboard actions, narrow resize, and the real prompt. A separate real mouse/AX
check is required for the controls.

## Release gates still requiring native evidence

1. Rust binary/library compile and all ordinary regression tests pass on targets.
2. Naga and actual native GPU pipeline creation succeed; no validation errors.
3. Earth orientation, clouds, thin atmosphere, dust transition and occluded stars
   match the recovered reference at wide/narrow and HiDPI sizes.
4. Home/picker share a scene; separate Homes retain independent Hold/view state.
5. Idle reuse truly does not keep artwork work running; typing, Hold, power and
   hidden/modal states pause; explicit/reduced-motion controls work.
6. Real prompt selection/IME and Space controls coexist; click/key/AX do not
   reach hidden controls; overlay opacity is knowingly unchanged for this patch.
7. Bundled maps, catalogue and supplied attribution ship; no runtime download.

The originating generation environment had no Rust toolchain and could not
create a browser WebGL context. Native gates are not waived by source checks.

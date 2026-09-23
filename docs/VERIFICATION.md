# Verified output retention

The first Verus boundary is `crates/pty/src/ring.rs`, the implementation
re-exported as `nus_pty::hold::Ring` and used by the production holder.

For a ring created by `new(capacity)`, each successful `push(input)` retains
exactly `suffix(previous_contents ++ input, capacity)`. Capacity is unchanged
and retained length never exceeds it. Zero capacity and inputs larger than
capacity are covered. `bytes()` returns precisely that retained sequence.
The fields are private; ordinary callers cannot manufacture invalid states.

The source uses the pinned official Verus annotation macros in ordinary Cargo
builds. Executable code is shared, not copied into a separate mathematical
model. Verus specifications/proof hints are erased from production builds.
The verification job checks all three executable methods with `--no-cheating`.
It also substitutes a dropped-input mutation and requires a proof failure,
not a compilation error. Differential tests compare to an unbounded history.

## Reproduce

Use official Verus **0.2026.09.20.aef82ed** (source commit
`aef82eda71838deef4a8cd0260fda6c9504be9b9`) and Rust **1.98.1**.
The ordinary project Rust toolchain remains unchanged.

```
python3 scripts/verify-ring.py --verus /path/to/verus
cargo test --locked -p nus-pty --lib
cargo bench --locked -p nus-pty --bench hold -- pty/ring_v2
```

Download URLs and SHA-256 pins are in `.github/workflows/verify.yml`.
The arm64 macOS release archive SHA-256 is
`3f89fd250d1e9792ed6d0c7c3ad72c03c02fdbca3f0638987af6e153d69377bc`.
Normal release compilation does not execute Verus; CI must prove the source
before a release claim is made. Evidence records include the checked source,
linkage-file and dependency-lock hashes. A locally checked source is not proof
that an older shipped binary contains it.

## Trusted boundary

There are no local assumes, admits, external-body wrappers, or unsafe code in
this proof. That does not eliminate the trusted base: Verus, its solver, the
compiler, and the pinned standard-library contracts for Vec/slice allocation,
copying, truncation and extension remain trusted. Successful allocation is
assumed. The compiled macro-erasure path and actual OS integration are also
covered by normal Rust tests, not by a whole-system theorem.

The proof covers retained bytes, not complete PTY delivery, authenticated
transport, arbitrary truncated-stream screen reconstruction, allocator
footprint, crash recovery, liveness, or speed. The Python benchmark exporter
and website are independently tested and are outside this Verus boundary.

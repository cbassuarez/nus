# Updates, Mercury and local security verification

Validated on macOS arm64, September 23, 2026 UTC, using isolated profiles.
No production installation was replaced and no model requests were made.

- Composite application suite: 258 passed, one existing ignored check.
- Focused workspace suites (`nus-vault`, `nus-sync`, `nus-pty`, `nus-hold`):
  29 passed, including the live holder spawn/detach/reattach/kill test.
- Release metadata/package tests: 13 passed (`scripts/test-release.py`).
- Vault release-mode unit tests: four passed.
- Real OS credential probe: encrypted disk bytes, another process reading with
  the same OS credential, and refusal to read/write after credential removal.
  Ciphertext remained unchanged. The probe deletes its disposable credential.
- Native UI harness: animated and reduced-motion Mercury, persistence across
  relaunch, profile date/layout, header readiness icon and its click into the
  restart warning, encrypted legacy-memory migration, and encrypted held-process
  detach/reattach between the actual app and its separate helper. The harness
  deletes credentials belonging to its disposable profiles.
- Installer fixtures: complete directory swap, recovery after a failed second
  rename, hostile archive paths/links, valid framework links, semantic-version
  selection, checksum/digest mismatch rejection and explicit profile continuation.

Reproduce with `cargo test -p nus-vault -p nus-sync -p nus-pty -p nus-hold`,
`cargo test --manifest-path spikes/composite/Cargo.toml --bin composite`,
`cargo run -p nus-vault --example probe`, `python3 scripts/test-release.py`, and
`python3 scripts/check-updates-mercury-security.py /path/to/nus.app`.
The native tests need OS keychain and loopback access. On a machine without a
credential store the holder integration test checks refusal to save/start;
that branch does not validate live resume. Unit fixtures explicitly inject
in-memory keys; production startup never uses that provider.

Not established by these checks: a live GitHub download followed by replacement
of a signed release installation, native Windows/Linux installation or vault
behavior, Windows sandbox linkage, Linux namespace/seccomp enforcement, sandbox
escape resistance, exhaustive secret detection, or safe resumption of arbitrary
agent jobs after updates. The managed runtime is a design document, not shipped
isolation. These limits are detailed in [Local security](LOCAL_SECURITY.md) and
[Updates and Mercury](UPDATES_AND_MERCURY.md).

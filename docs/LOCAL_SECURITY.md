# Local security boundaries

This describes the implementation, not a SOC 2 certification, GDPR assessment,
or a guarantee that arbitrary secrets can always be recognized.

## Saved state

`nus-vault` uses XChaCha20-Poly1305 with a random 256-bit key and fresh 192-bit
nonces. The key is stored through macOS Keychain, Windows Credential Manager,
or Linux Secret Service. A public random identifier lives in `.vault-id`;
the encryption key is never written to a nus file. This is OS credential-store
protection, not a promise of hardware-enclave key derivation on every platform.
Unlocked keys and plaintext necessarily exist in process memory.

The vault covers session snapshots, recent-history state, assistant memory,
held-process authentication records, journal records, terminal replay streams,
replay images, the profile-sync encryption key, and the forge token. The primary
process migrates recognized legacy files in these locations before starting
state writers. Migration uses an encrypted temporary file and atomic replacement;
it does not create a plaintext backup. A completed migration marker makes later
plaintext reads fail, including damaged ciphertext headers. Existing backups, filesystem snapshots,
swap, and previously deleted plaintext are not securely erased by migration.

Each record authenticates its profile-relative path. Replay frames additionally
authenticate a stream identifier and sequence number. Frame reordering and
modification are rejected. This does not prevent rollback to a previously valid
file or truncation at a complete replay-frame boundary. Writes are bounded;
oversized, corrupt or unsupported state is preserved and reported, not silently
replaced with an empty session.

If the credential store cannot provide the key, sensitive persistence stops.
There is no plaintext fallback. The app shows a notice and status in Settings.
If an already unlocked key is cached in memory, locking the keychain does not
revoke that process's copy. Losing the OS credential makes ciphertext unreadable;
copying the profile alone is not a recovery mechanism. An explicitly configured
sync carrier decrypts locally and re-encrypts with the separate sync key, so
other authorized devices can use their own local vault keys.

The vault does **not** encrypt project files, ordinary settings, reading-list
articles, browser-managed storage, explicit screenshots/exports, or files owned
by independently installed Claude/Codex CLIs. The local control socket's discovery
token remains an owner-only file so the CLI can connect. The phone uses an
ephemeral credential. These are separate boundaries, not encrypted session data.

## Context and network transfers

Opening or editing a local buffer does not automatically upload it to a model.
In Ask, pressing Send authorizes the selected context to reach the selected
backend. **RUN authorizes shell execution; it is not the model-upload gate.**
Opt-in sync, remote-control requests, visiting a website, and user commands are
other explicit ways data can leave the machine. Release update checks contact
GitHub automatically when enabled; they contain no local text buffers.

Before nus hands Ask payloads or textual content responses from its control IPC
to another process, Rust applies precompiled patterns and an entropy heuristic.
The scanner handles labelled credentials, common provider tokens, private-key
blocks, authorization headers, connection URLs and mixed-case high-entropy
strings. It removes terminal escape sequences before matching. Suspected values
become `[!SECRET!]`; a payload over the 8 MiB scanning limit is withheld in full.
The source buffer remains unchanged. Scanner matches and original values are
not logged. Ask offers a redacted preview and an explicit **send original once**
override for that exact payload, with no persistent allowlist.

This is best-effort detection: unlabelled short passwords, encoded/fragmented
secrets, images and binary content may evade it, and false positives are possible.
Remote screenshots and explicit exports are not OCR-scanned. Independent CLI
agents can read additional files, inherit environment variables, retain their
own sessions and make their own network calls. Nus cannot sanitize those calls
with an IPC filter. See [the managed runtime design](MANAGED_AGENT_RUNTIME.md).

## Browser process isolation

The browser requests Chromium `site-per-process`, enables the CEF sandbox,
disables user command-line overrides, and removes switches that disable sandbox
or site isolation. macOS helper executables initialize the CEF sandbox before
running renderer code. Site isolation separates sites into renderer processes;
the OS sandbox restricts renderer access to host resources. The browser process
and its intentionally exposed IPC handlers remain privileged and require review.

Do not advertise an unconditional “web UI cannot read Rust memory” guarantee.
Sandbox escape vulnerabilities and privileged host interfaces remain possible.
Arbitrary privileged Chromium extensions are not an approved isolation boundary.
Windows currently passes no CEF sandbox-info object; Windows sandbox linkage and
startup must be completed and tested before publishing a Windows sandbox claim.
Linux namespace/seccomp enforcement also requires native package validation.
An enabled setting or command-line switch alone is not proof of enforcement.

## Verification and release requirements

Unit checks cover authenticated encryption, wrong keys/context, corruption,
legacy migration, replay ordering, scanner behavior, release validation and
profile continuation. A real macOS Keychain probe tests decryption by a separate
process and deletes its test credential. Neither substitutes for an adversarial
review or native Windows/Linux tests. `test-vault` is a development-only,
fixture-injection API. Only unit-test setup calls it for disposable directories;
merely enabling the feature never changes production key selection.

References: [RustCrypto AEAD](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/),
[keyring OS backends](https://docs.rs/crate/keyring/3.6.3/source/README.md),
[Chromium site isolation](https://www.chromium.org/developers/design-documents/site-isolation/).

See [the verification record](UPDATES_SECURITY_VERIFICATION.md) for executed checks and unverified platform boundaries.

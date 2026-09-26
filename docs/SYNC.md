# Sync

Settled 2026-09-17. The profile on more than one device, with no account,
no server of ours, and nothing readable in flight or at rest anywhere but
your machines. `crates/sync` is the core; `spikes/composite/src/syncui.rs`
is the app's side; `nus sync …` is the CLI.

## The shape

**A key you copy.** 32 random bytes, made once on the first device (SETTINGS
· SYNC · MAKE ONE, or `nus sync key`), shown as a word — `nus5-xxxx-xxxx-…`,
lowercase base32 in groups of four — that you paste on the next device
(JOIN WITH A KEY, or `nus sync join <word>`). It lives in
`profile/sync/key`, 0600 where the OS has modes. Nothing derives it from a
password, nothing stores it anywhere else, and forgetting it on a device
(FORGET) just makes that device stop taking part. Two devices with the same
key are the same person.

**Carriers you already have.** Two, either or both, set in SETTINGS · SYNC
or by `nus sync folder <path>` / `nus sync git <remote>`:

- **A folder** your OS or Syncthing already moves — iCloud Drive, OneDrive,
  Dropbox, a USB stick, a network share. nus writes sealed files there and
  reads what the other device wrote; the moving is somebody else's job.
- **A git remote**, private. A clone lives under `profile/sync/git`;
  `pull --rebase` before, `add · commit · push` after. Same files, with
  history for free.
- **A forge** (2026-09-19): the git remote made for you. On the profile
  card — the avatar in the footer, or PROFILE · HOW IT LIVES, or SYNC ·
  SIGN IN TO A FORGE — pick GitHub, Forgejo, Gitea or GitLab. GitHub signs
  in from the card by the device flow (a code to enter on
  github.com/login/device) when nus has an app id (`NUS_GITHUB_CLIENT_ID`
  at build or run time), else with a token of repo scope; the others take
  the instance's address and a token. nus asks who the token is, finds
  `nus-profile` or makes it, private, and points the git carrier at its
  https url. The token lives in `profile/sync/forge.token` (0600 where
  modes exist), goes to the forge as an `Authorization` header on each
  git command (`-c http.extraheader`), and is never in a url, in git's
  config, or on the carrier; `profile/sync/forge.json` remembers the forge,
  the login and the repo. FORGET THE FORGE drops both and clears the remote;
  the repo stays yours to delete. The web calls go through `curl`.

**Only ciphertext leaves.** Every file is sealed with XChaCha20-Poly1305
under the key, the file's relative path as associated data so a blob cannot
be moved to another slot. The blob is `NUS1` · 24-byte nonce · ciphertext.
The manifest (device, clock, per-file hash · mtime · size) is sealed the
same way. Names on the carrier are hashes of the key and the label, so a
stranger holding the folder sees neither your hostnames nor which files
exist — only a count and some sizes.

**Last writer wins, the loser kept.** Per file, by the writer's clock. A
file newer on another device replaces ours and ours is kept beside it as
`<file>.<device>.lost`; a file we changed since is pushed. Same hash, no
work. This is file-level conflict resolution, not a collaborative document
merge. Reading-library exchange takes the same local writer lock as the reader
and defers a busy library. That coordinates cooperating nus processes on one
device; it does not serialize independent devices or external sync software.

**What travels.** The profile's own files: `settings.json`, `me.json` (your
name, face and first day), `rules.luau`, `folders.json`, `ports.json`,
`memory.md`, `sites.json`, `containers.json`, `blocklist.txt`,
`avatar.png`, every file in `layouts/`, `themes/` and `surfaces/`, and the
profile notes in `notes/` (the `.md` files; sealed on both devices, like
`memory.md`, and never their `.lost` copies). Folder notes live in the
project, not the profile, and travel only with the project's own git. Saved
reading records and their immutable article objects also travel; temporary
files, writer locks and conflict backups are excluded. Local article snapshots
remain ordinary readable files in the profile: encryption protects the carrier,
not the local library. Removal is not secure erasure of retained snapshots or
carrier history. See [reading-library compatibility](READING_LIBRARY_REPAIR.md).
Not
the device's name (`profile/sync/device`), which is what tells the two
apart. The session (open tabs, `session.json`) only when SETTINGS ·
SYNC · WHAT TRAVELS · THE SESSION TOO is on — off by default, because a
laptop and a desk machine rarely want the same tabs. Never cookies, caches,
downloads or shell history.

## When

- **Every N minutes** (EVERY: ON DEMAND · 5 · 10 (default) · 30), on a
  worker thread; the first run thirty seconds after launch.
- **At quit** (AT QUIT · SYNC, default on), waited on for up to eight
  seconds so the last edit lands.
- **On demand**: NOW in settings, the palette's `sync` rows, `nus sync`.

After a pull the prefs, rules and folders reload and the layout re-runs, so
what arrived shows without a restart. A notice says `sync · 2 in · 1 out`,
or `· 1 kept as .lost`, or the first error; a run that changed nothing says
nothing.

## The CLI

```
nus sync              # exchange now
nus sync key          # make (or show) this device's key
nus sync join <key>   # take a key from another device and exchange
nus sync status       # key · carriers · last run
nus sync folder <p>   # set the folder carrier ("" clears)
nus sync git <remote> # set the git carrier ("" clears)
```

## Threat model, plainly

- The carrier operator (Apple, Microsoft, Dropbox, GitHub) sees encrypted
  blobs with opaque names, their sizes, and when they change. Not the key,
  not a file name, not a byte of the profile.
- Someone with the folder but not the key has the same view.
- Someone with the key has the profile. The key is the secret; copy it over
  a channel you trust (the two devices in the same room, a password manager,
  never a chat you don't control).
- There is no recovery: lose the key on every device and the sealed copies
  are noise. The profile is still on each device in the clear, so make a new
  key and carry on.

## Not this

Not a server of ours, not an account, not a merge editor, not a backup of
cookies or logins, not a way to share a profile with another person (though
nothing stops two people who share a key). Arc and Dia sync through an
account; this is the same outcome with the trust kept at home.

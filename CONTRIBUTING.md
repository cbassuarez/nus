# Contributing

Read the [README](README.md) first: nus is one person's terminal and browser,
open because open is right. This file says what that means in practice.

## What I welcome

- **Feature requests** that explain the problem and how the change would help.
  Check existing settings, rules, themes and layouts first. A request does not
  promise implementation.
- **Bug reports** with a reproduction: what you did, what happened, what you
  expected, your OS, and the commit. A screenshot or a `NUS_DUMP` capture of
  the terminal bytes helps. If you cannot reproduce it consistently, describe what you know.
- **Small fixes**: a crash, a platform build break, a wrong doc, a typo, a
  clippy or fmt failure in CI. Open the PR directly.
- **Platform work** on macOS and Linux for pieces that are Windows-only today
  (the global hotkey, the taskbar, the OS colour scheme). Open an issue first
  so we agree on the shape; the Windows code shows the shape.

## What I decline

- **Behaviour changes** I didn't ask for. Before a PR that changes what nus
  does or how it looks, open an issue first so we can agree on the scope. The design is settled in [docs/PRODUCT.md](docs/PRODUCT.md)
  and [docs/DESIGN.md](docs/DESIGN.md); explain any proposed departure before implementing it.
- **Refactors for their own sake**, dependency swaps, formatting churn, and
  anything that widens the surface without a settled design behind it.

## AI-generated changes

You'll use an assistant; so do I. The bar is the same either way: **you ran
it, you understand it, you can explain every line if asked.** Say in the PR
that it was assisted. A PR whose author can't explain it is closed without
review, however green the CI.

## The mechanics

See [the review and build guide](docs/REVIEWING.md) for prerequisites, both
Rust workspaces, release verification, and the repository map. Branch from
`main`; use a focused feature branch and open a PR back to `main`. Release tags
identify immutable packages. A feature branch is not a supported release.


- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` must pass; CI runs them on three OSes.
- The app in `spikes/composite` is not rustfmt-formatted (long lines are
  deliberate there); don't reformat it.
- One change per commit, a commit message that says what and why in prose,
  no "fix", no "wip". Look at `git log` for the voice.
- Icons are Phosphor, never emoji or text glyphs. Colours are the theme's
  tokens, never literals. Anything floating gets a 2px edge and a hard offset
  shadow. Read [docs/DESIGN.md](docs/DESIGN.md) before touching the chrome.
- Use the real thing: a crate that already does it beats code that reimplements
  it (ropey, tree-sitter, lsp-types, the standard OSCs).
- Never launch, drive or screenshot the app from a script that sends input to
  the OS; the app photographs itself (`NUS_SHOT`).

## Conduct

The [Contributor Covenant](CODE_OF_CONDUCT.md) applies. Be kind; "no" from
me is not an invitation to argue, and I'll be kind back.

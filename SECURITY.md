# Security

nus is [@cbassuarez](https://www.github.com/cbassuarez)'s personal browser and terminal project. It runs on your machine, holds no
account, and has no server behind it. Please read this file before proposing any security fixes.

## Reporting something

Use **[private vulnerability reporting](https://github.com/cbassuarez/nus/security/advisories/new)**
on this repository, or email [contact@cbassuarez.com](mailto:contact@cbassuarez.com).
Please do not open a public issue for a vulnerability or post a working exploit
in one.

Tell me what an attacker can do, not only what looks wrong: the version or
commit, the OS, the steps, and what the attacker has to control to get there
(a page you visit, a file you open, a device on your network, another program
on the same machine). I read reports as I can; this is my own personal project and not a funded program. 

Fixes land on `bugfix` and in the next build. There are no backported branches:
**the supported version is the current one.**, for now. I am still working on creating a robust longterm support plan, so until then, support is ad-hoc. Long term multi-version support ships with stable builds, but as of right now, there are only preview bundles to download, so we're staying ad-doc until then.

## What nus is

nus is a user agent. It fetches what you ask for, runs what you tell it to
run, and keeps what you ask it to keep. It is not an intermediary, and it is not centralized; nothing you
do passes through me, there is no account to suspend and no content to
moderate, and nothing is logged anywhere I can read.

What I am responsible for is that nus does what it says, that the dangerous
parts are visible and off until you turn them on, and that a page you visit
cannot reach past the tab it is in.

## The trust model

- **You are trusted.** Anything you can do at your own shell, nus may do for
  you. Removing that would be removing the product.
- **Pages, page text, and anything a command prints are not trusted.** They can
  say whatever they like, including things addressed to an assistant.
- **Other programs on your machine are not trusted.** The control socket is
  authenticated and its token is an owner-only file.
- **Your local network is not trusted.** The phone's page is served over TLS
  and nothing else listens off-machine.

## Invariants

These are properties nus is supposed to have. A change that breaks one is a
security bug, whatever else it improves; each is covered by a test beside the
code named here.

1. **Nothing runs by itself.** A command reaches a shell when you press RUN and
   at no other time. No assistant answer, no skill, no remote request and no
   request from the phone can run a block. One press gives one single-line
   command its Enter; a block of several lines is pasted to be read. When page
   or tab context went into the prompt, RUN asks twice. `ask.rs`
2. **A private window is a smaller app.** It refuses the CLI, shells, hatch,
   phone control, remote control, external debugging and assistants, and no
   preference can switch any of them back on. `private.rs`
3. **A private session leaves nothing behind.** Its root is disposable, and a
   root whose process was killed is removed at the next launch. Downloads, and
   the origin the OS records for them, are the documented exception.
   `private.rs`, `docs/PRIVACY_AND_DIAGNOSTICS.md`
4. **The phone's door is narrow.** Requests from the phone reach `front` and
   `hands-answer` and nothing else, enforced where the request is answered.
   `remote.rs`
5. **Nothing off-machine is served in the clear.** The phone is TLS-only with a
   per-session certificate; a plaintext request is not answered. `phone.rs`
6. **No listener opens by itself.** External debugging requires an environment
   variable and binds to loopback. The control socket is authenticated,
   bounded, and expires idle input. `browser.rs`, `little.rs`, `security.rs`
7. **Site permissions belong to a full origin** — scheme, host and port.
   `sites.rs`
8. **nus sends nothing on its own.** No telemetry, no analytics, no crash
   upload. Bug reports are a draft you review and post yourself. Assistants
   reach a backend only when you send a question, with only the context whose
   chips are lit. `docs/PRIVACY_AND_DIAGNOSTICS.md`

## Known limits

Stated here because they are design positions, not oversights.

- **The phone is trust-on-first-use.** Its certificate is self-signed, so the
  phone asks once whether to trust it. Passive interception is closed; an
  active attacker already in the middle of that first connection, who is then
  accepted at the phone's prompt, is not. Compare the fingerprint shown under
  SYNC · THE PHONE if that matters to you, or leave the phone off.
- **Shell output is not treated as untrusted input to an assistant.** A hostile
  repository or a `curl` can reach it. Nearly every question carries a command
  block, so marking them all would mark everything and mean nothing. Read what
  you run.
- **External debugging is privileged and unauthenticated** by Chromium's
  design. It is off unless you set the variable, and binds to loopback. Do not
  forward it.
- **Incognito does not hide traffic** from websites, your network or an
  employer, and does not erase downloaded files or their recorded origin.
- **Rules, skills and `NUS_ASK_CMD` run what you put in them.** They are your
  code; nus does not sandbox them.
- **Dependencies.** nus embeds Chromium through CEF. A Chromium vulnerability
  is a nus vulnerability until the bundled version moves; report it upstream
  as well.

## Out of scope

Reports that amount to "a user can harm their own machine", "an assistant gave
bad advice", missing hardening with no attack behind it, or scanner output
with no reproduction. So is anything that requires an attacker who already has
your user account on your machine — at that point they have the shell nus
would have given them anyway.

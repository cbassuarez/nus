# Privacy and diagnostics

## Incognito

Use **⇧⌘N** on macOS or **Ctrl+Shift+N** on Windows/Linux, or choose
**File → New incognito window**. The window carries an Incognito label.

Each launch from a regular window starts a separate private process. Tabs and
additional windows opened inside that process share temporary cookies and site
storage. Closing its last window ends that private session. A new private launch
starts empty. Closing the last tab closes its private window; ⇧⌘W / Ctrl+Shift+W
closes the whole private window.

### A private window is a smaller app

Incognito is not only a temporary cookie jar. A private window refuses the
features that reach the disk, the network or another process, and it does so
in the process itself rather than in the interface:

| Refused in a private window | What it would otherwise reach |
| --- | --- |
| The `nus` CLI and launch handoff | A loopback socket other local programs can drive |
| Terminal sessions and background hatch | The shell, the filesystem, long-lived work |
| Phone control | A listener on the local network |
| Remote control | Scripted navigation and questions |
| Assistants | A model backend, local or hosted — see below |
| External debugging (`NUS_REMOTE_DEBUGGING_PORT`) | A privileged, unauthenticated protocol |
| History, session restore, replay, journal, download history, site preferences | The profile on disk |

Preferences cannot switch any of this back on. Appearance is the only thing
that crosses from a regular window, and the private process constrains the
preferences it receives before reading them.

### What still persists

Private browsing does not save browsing history, session restore, replay,
site preferences or download history. Three things outlive the session anyway:

- **Downloaded files, and their origin.** Completed downloads stay where they
  were saved. On macOS the system also records the source URL and referrer in
  the file's extended attributes (`com.apple.quarantine`,
  `com.apple.metadata:kMDItemWhereFroms`), where Spotlight indexes them. nus
  neither writes nor removes those; deleting the file removes them with it.
- **The disposable root, if the session was killed.** A private session runs in
  a temporary directory that it removes on exit. A crash, a kill or a power cut
  leaves that directory behind. The next launch of nus removes any private root
  whose process is gone. Removal is an unlink, not an erase.
- **Anything you copied, typed or uploaded elsewhere.**

Incognito does not hide traffic from websites, your network or an employer.
The private home screen says as much, and that screen — not this file — is the
claim nus makes to the person using it.

## Site permissions

Saved camera, microphone and other permission decisions belong to the full
web origin: its scheme, hostname and port. For example, an allowance for
`http://localhost:3000` does not allow `http://localhost:4000`. IPv6 addresses
remain distinct. The site panel shows the origin alongside its saved decisions.

Older builds stored permissions by shortened hostname. Those broad grants are
not reused; affected pages will ask again. Private permissions stay in memory
and disappear with the private session.

## macOS Keychain

Normal production browsing uses Chromium's macOS Keychain integration. The
test-only mock keychain is no longer enabled on ordinary launches. macOS may ask
for Keychain access. Cookies from older builds that used the mock keychain may
require signing in again; nus does not read or migrate a user's Keychain itself.

Disposable screenshot fixtures may use the mock keychain to avoid permission
dialogs. Set `NUS_TEST_REAL_KEYCHAIN=1` to test the production Keychain path.
Both `NUS_SHOT` and `NUS_SHOT_DIR` must be present for the fixture bypass.

## Assistants

The Ask panel sends a question, and the context you have lit, to a backend. It
is off in private windows. In a regular window, what leaves the machine depends
entirely on which backend answers:

| Backend | Where the question goes |
| --- | --- |
| `claude -p`, `codex exec`, `gh copilot -p` | That tool, which contacts its own vendor under its own account and policy |
| `ollama run` | A model on this machine |
| The Anthropic API via curl (`ANTHROPIC_API_KEY`) | Anthropic |
| `NUS_ASK_CMD`, or `profile/assistants.json` | Wherever you pointed it |

nus adds no backend of its own, holds no key of its own, and sends nothing on
its own initiative: a request leaves only when you send a question.

The chips above the field are the disclosure, and they are accurate. Each one
names what it attaches, and only the lit ones are gathered:

- **shell** — the shell's name, the working directory and the OS
- **block** — the focused command and up to the last 60 lines of its output
- **page** — the title, URL and extracted text of the page beside the shell
- **tabs** — the title and URL of every open tab
- **editor** — the path and full text of the open file
- **memory** — `profile/memory.md`, which grows only when you press REMEMBER

Shell, block and page are lit by default. Page and editor context mean page
text and file contents are sent verbatim; treat a lit chip as consent for that
question, to that backend. Nothing from the panel is written to disk except the
line REMEMBER appends to `profile/memory.md`; the turns themselves live in the
panel and end with it.

### Running what the assistant suggests

A page can write anything, including a line addressed to an assistant, and a
question that carries page or tab context puts that writing in the prompt. So a
suggested command is not only the model's idea — it is an idea a stranger may
have had a hand in. Three rules hold:

- **Nothing runs on its own.** A command reaches the shell when you press RUN,
  and at no other time. No answer, no skill, no remote question and no phone
  request can run a block. A question asked through remote control fills the
  panel; it does not press anything.
- **An answer the web touched asks twice.** When page or tab context went into
  the prompt, the first RUN arms the chip and says so; the second, within a few
  seconds, runs it. The arming belongs to one block and lapses on its own.
- **One press is one command.** Only a single-line block is given its Enter. A
  block of several lines is pasted for you to read — bracketed where the
  program asked for it — so its newlines cannot become a run of commands from
  one press.

Shell output is not counted as web context, though a hostile repository or a
`curl` can reach it. Nearly every question carries a block, so marking those
would mark everything and mean nothing. Read what you run.

## Bug reports and feature requests

Choose **Help → Report a bug / Request a feature**, or use the buttons under
**Settings → Updates**. This opens an editable GitHub issue draft in
`cbassuarez/nus`. Nothing is submitted automatically.

The draft includes the nus version, build revision, Chromium version and OS
family. It does not automatically include visited URLs, paths, logs, cookies,
credentials or profile information. Review anything you attach before posting.
The updated issue forms must be present on GitHub's default branch for their
individual fields to prefill.

## Developer tools and remote control

Built-in DevTools opens Chromium's native tools window using CEF's internal
connection. Ordinary launches do not open an external debugging port.

For a deliberate developer session, set `NUS_REMOTE_DEBUGGING_PORT` to a port
between 1024 and 65535 before launching nus. External debugging binds to loopback
and is disabled in incognito even if that variable is set. The debugging
protocol is privileged and unauthenticated; do not forward that listener to
other machines. Close the developer session and remove the variable afterward.

The separate nus CLI uses authenticated loopback requests. Instance credentials
are random and written atomically with owner-only access. Launch handoff also
requires those credentials; old unauthenticated raw URL/`raise` lines are no
longer accepted. Normal CLI commands retain their JSON protocol.

The optional phone control feature is off by default and unavailable in
incognito. When it is on, this window is served on the local network over
**HTTPS**, using a certificate generated in memory when the server starts.
Nothing is written to disk and nothing is installed on the phone; stopping the
server destroys the certificate and the token with it. A plaintext request to
that port is not answered, so the token does not cross the network in the clear.

The certificate is self-signed, which has a consequence worth stating plainly:
the phone will ask, once, whether to trust it, and trusting it is
trust-on-first-use. That closes passive interception — nobody on the network
can read the token or the page by watching — but it cannot by itself tell nus
apart from someone who got in the middle before the phone ever connected.
SYNC · THE PHONE shows the certificate's SHA-256 so it can be compared with
what the phone displays under the certificate's details. Opening the phone's
own page in a tab here needs no such judgement: nus pins that exact
certificate, by its whole encoding, and trusts nothing else at that address.

Requests that arrive from the phone are limited to the two verbs its page
needs, `front` and `hands-answer`. The limit is enforced where the request is
answered, not where it is sent, so a future endpoint cannot widen it by
accident. Everything else — shells, the filesystem, the assistant, the pages,
the CLI's own verbs — is reachable only through the instance socket, whose
token is an owner-only file on this machine.

## Reproducing the native checks

Build the app and `nus-cli`, then pass a test bundle or executable to:

```sh
NUS_TEST_REAL_KEYCHAIN=1 python3 scripts/check-privacy-security.py /path/to/nus.app
```

The harness uses disposable profiles and a loopback-only HTTP fixture. It checks
cookie isolation and persistence, private disk canaries, multi-window shortcuts,
last-tab/window closure, retained downloads and their macOS provenance
attributes, native DevTools, report controls,
CLI authentication, oversized requests and idle input expiry. On macOS it also
checks that the normal fixture's cookie is encrypted on disk. It never needs
the user's browser profile or a live website.

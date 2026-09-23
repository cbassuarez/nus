# Managed agent runtime design

Status: design for a new opt-in runtime. Existing terminal agents are unmanaged;
this document does not claim they are isolated by the current implementation.

## Trust boundary

Run the agent in a separate restricted process or disposable VM. Give it a
minimal environment, a private temporary home, a read-only snapshot of explicitly
selected project roots, and a writable overlay for proposed changes. Do not mount
the user's home, SSH directory, browser profiles, OS credential APIs, nus profile,
external CLI session stores, Docker socket, or host control socket. Agent children
inherit the same restrictions. Path grants use opened handles/canonical roots,
reject symlink escapes, and are checked again when an operation executes.

The runtime has no direct network access. An authenticated local broker owns all
provider credentials and all egress. It accepts typed, size-bounded requests,
scans complete text payloads in Rust, and sends approved bytes to an allowlisted
provider endpoint. Arbitrary HTTP, DNS, subprocess networking, redirects outside
the allowlist and loopback access are denied. A proxy environment variable alone
cannot enforce this boundary; the OS/VM must deny alternative sockets.

The first implementation should use a nus-controlled provider adapter rather
than assume existing CLI products can run behind this protocol. A CLI is supported
only after its authentication, session, tool and network behavior can be routed
through the broker without granting it raw credentials or independent egress.

## Authorization model

Every action carries a session ID, monotonically increasing sequence, requesting
tool, selected roots, provider, operation and payload hash. Grants are scoped to
one operation or an explicitly visible session policy. Page/repository text is
data and can never create grants. A changed payload invalidates its approval.

Model submission and shell execution have separate controls: **Send context**
approves disclosure to the named provider; **RUN** approves a displayed command
in the named working directory. File changes are proposed in the overlay and
applied through reviewed diffs. Original-secret overrides name the finding,
destination and exact payload and expire after one send. Cancel revokes pending
operations, closes broker channels and terminates the entire process tree.

## Checkpoint and resume

The broker alone writes resumable state through `nus-vault`. Checkpoints contain
the protocol version, agent/adapter version, project identity, overlay digest,
turn history, consumed grants and pending operation IDs. Provider credentials are
references to the credential broker, never serialized tokens. Pause and flush
before an app update; report which jobs cannot checkpoint. Resume revalidates
project roots, provider policy, executable identity and checkpoint integrity.

Never replay an uncertain side effect automatically. Network writes, file commits
and commands need idempotency keys where supported; otherwise an interrupted
operation becomes “outcome unknown” and needs review. Approval and one-time-secret
grants do not survive restart. Encrypted local audit records keep action types,
destinations, hashes and decisions, excluding raw secret matches and payloads.
They remain local unless the user explicitly exports them.

## Platform implementation

| Platform | Isolation candidate | Required enforcement |
| --- | --- | --- |
| macOS | Virtualization.framework Linux VM for arbitrary CLI compatibility | Explicit filesystem shares, no direct guest network, broker-only transport; evaluate a signed sandboxed helper for a narrower native adapter |
| Windows | AppContainer/restricted-token worker plus Job Object; VM fallback | Deny network capabilities, restrict filesystem ACLs and named pipes, kill all descendants on cancellation |
| Linux | User/mount/network namespaces, seccomp, Landlock where available | Minimal mount tree, no host networking, dropped capabilities and no-new-privileges; fail closed if required primitives are unavailable |

Do not silently fall back to an ordinary child process. Show “managed runtime
unavailable” and offer a separately labelled unmanaged terminal only by explicit
choice. Renderer sandboxing and agent sandboxing are different systems.

## Acceptance gates

1. Malicious fixtures cannot read home/keychain/session files, traverse symlinks,
   access host memory, use inherited handles, or escape through child processes.
2. Raw sockets, DNS, loopback, alternate proxies and subprocess networking fail;
   the broker receives all permitted egress. A seeded secret never reaches a
   recording test endpoint without its exact, one-time override.
3. Prompt injection cannot grant tools, change roots or convert an Ask submission
   into RUN. Revoked and replayed capability messages fail.
4. Kill/restart tests at every checkpoint/command boundary preserve ciphertext,
   surface unknown outcomes, and never silently repeat side effects.
5. Native tests run on every supported OS and package configuration, including
   unavailable keychain/sandbox conditions. Independent security review precedes
   enterprise guarantees. Performance tests report scanning, checkpoint and
   broker overhead separately from renderer/frame timings.

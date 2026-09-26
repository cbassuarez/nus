---
{
  "title": "Why the prompt wraps",
  "tags": ["terminal", "reflow"],
  "nus": {
    "schema": 1,
    "id": "2c557afcd3864f9c85f659ea18770ea1",
    "revision": 1,
    "created_at": "2026-09-26T18:00:00Z",
    "updated_at": "2026-09-26T18:00:00Z",
    "filed": true,
    "sources": [
      {
        "id": "d66a5679d5e14ed690867050053bc16e",
        "kind": "terminal",
        "label": "Reflow regression test",
        "captured_at": "2026-09-26T18:00:00Z",
        "command": "cargo test -p nus-vt reflow",
        "shell": "zsh",
        "cwd": {"workspace_id": "nus", "relative": "."},
        "exit": 0,
        "duration_ms": 421,
        "excerpt": "test result: ok. 12 passed; 0 failed",
        "captured_sha256": "11b236e8b34639e55c30fe5663b408cfa6d3814f7528780aa4ea67bd469c0a3c",
        "redaction": {"applied": true, "masked_count": 0},
        "origin": {"journal_key": null, "replay_key": null}
      }
    ],
    "assets": [],
    "aliases": []
  },
  "custom_owner": "example of an unknown field that must survive saves"
}
---

The command is one logical line even when the terminal displays three rows.
Store the selection against the logical command, then map it to visible cells.

<!-- nus:source d66a5679d5e14ed690867050053bc16e -->
```nus-block cmd="cargo test -p nus-vt reflow" exit=0
test result: ok. 12 passed; 0 failed
```

- [ ] Check the resize regression before changing prompt paint.
- [ ] Verify the original command survives a narrower window.

The captured excerpt remains available after replay retention expires.

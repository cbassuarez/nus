#!/usr/bin/env python3
"""Check restored-page sound cues in a packaged app with isolated profiles.

Uses delayed local HTTP pages, a redirect, a split pane, two macOS windows and an
actual relaunch. NUS_SOUND_TRACE records cues selected by the native sound
dispatcher. New navigation and reloads are positive controls; fast loads stay quiet.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    out = args.out.resolve()
    if out.exists():
        parser.error("choose a new output directory to retain earlier evidence")
    profile = out / "profile"
    profile.mkdir(parents=True)
    app = args.app.resolve()
    exe = app / "Contents/MacOS/nus" if app.suffix == ".app" else app
    requests = []
    lock = threading.Lock()
    stop = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            parsed = urlsplit(self.path)
            query = parse_qs(parsed.query)
            delay = float(query.get("delay", ["0"])[0])
            if stop.wait(delay):
                return
            if parsed.path == "/redirect":
                self.send_response(302)
                self.send_header("Location", "/redirected?delay=6")
                self.send_header("Content-Length", "0")
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
            else:
                title = "Session sound " + parsed.path.removeprefix("/")
                body = ("<!doctype html><title>" + title + "</title><h1>" + title + "</h1>").encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                try:
                    self.wfile.write(body)
                except (BrokenPipeError, ConnectionResetError):
                    return
            with lock:
                requests.append({"path": self.path, "completed_at": time.monotonic()})

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f"http://127.0.0.1:{server.server_port}"

    def page(path):
        return {"kind": "page", "url": url + path, "title": "Session sound fixture"}

    restored = ["/fast?delay=0.01", "/restore-a?delay=2", "/restore-b?delay=5",
                "/restore-c?delay=8", "/restore-d?delay=15", "/redirect", "/restore-e?delay=10"]
    tabs = [{"left": page(path)} for path in restored]
    tabs[-1]["right"] = page("/split?delay=8.5")
    session = {"tabs": tabs, "active": 0, "windows": [{"tabs": [
        {"left": page("/window-a?delay=3")}, {"left": page("/window-b?delay=11")}], "active": 0}]}
    (profile / "session.json").write_text(json.dumps(session))
    (profile / "onboarded").write_text("skip")
    (profile / "settings.json").write_text(json.dumps({
        "behavior": {"splash": "None", "then": "Prompt", "remember": True,
                     "new_window": "Prompt", "keep_alive": "Off", "close_asks": False,
                     "update_checks": False, "follow_os_theme": False},
        "motion": {"register": 0.5, "reduce": True},
        "window_rect": [80, 80, 1100, 900], "theme_mode": "paper",
        "sound": {"enabled": True, "volume": 0.6},
    }))

    def run(name, steps):
        evidence = out / name
        evidence.mkdir()
        shot = evidence / "run.shot"
        shot.write_text("\n".join(steps) + "\n")
        env = {key: value for key, value in os.environ.items() if not key.startswith("NUS_")}
        env.update(NUS_SHOT=str(shot), NUS_SHOT_DIR=str(out),
                   NUS_SHOT_OUT=str(evidence / "screens"), NUS_MODE="paper", NUS_SOUND_TRACE="1")
        log = evidence / "run.log"
        with log.open("w") as output:
            proc = subprocess.Popen([str(exe)], cwd=out, env=env, stdout=output,
                                    stderr=subprocess.STDOUT, start_new_session=True)
            try:
                code = proc.wait(timeout=100)
            except BaseException:
                os.killpg(proc.pid, 15)
                proc.wait(timeout=10)
                raise
        text = log.read_text(errors="replace")
        assert code == 0 and "panicked" not in text and "load STUCK" not in text, text[-4000:]
        return text

    def cues(text):
        return sum(line.strip() == "SOUND_EVENT page.ready ready" for line in text.splitlines())

    def phase(text, start, end):
        first = text.index("shot: state " + start)
        last = text.index("shot: state " + end, first)
        return cues(text[first:last])

    try:
        steps = ["wait 1200", "assertwindows 2", "pagestate opening-start",
                 f"tab {url}/manual?delay=2", "awaitpage Session sound manual", "awaitload 8000",
                 "wait 300", "pagestate opening-end", "wait 16000",
                 "asserttabs 9", "assertwindows 2", "pagestate restoration-end"]
        # Check that the actual restored documents finished, including the redirect
        # and both sides of the split, before testing subsequent navigation.
        for index, path in enumerate(restored):
            title = "redirected" if path == "/redirect" else urlsplit(path).path.removeprefix("/")
            steps += [f"benchtab {index}", f"awaitpage Session sound {title}", "awaitload 8000"]
        steps += ["focus page", "awaitpage Session sound split", "awaitload 8000", "focus shell",
                  "benchtab 0", "pagestate navigation-start", f"url {url}/navigate?delay=2",
                  "awaitpage Session sound navigate", "awaitload 8000", "wait 300", "pagestate navigation-end",
                  "key cmd+r", "wait 200", "awaitload 8000", "wait 300", "pagestate reload-end",
                  "key cmd+shift+r", "wait 200", "awaitload 8000", "wait 300", "pagestate hard-reload-end",
                  f"tab {url}/new-fast?delay=0.01", "awaitpage Session sound new-fast", "awaitload 8000",
                  "wait 300", "pagestate fast-end"]
        first = run("first", steps)
        counts = {
            "startup": cues(first[:first.index("shot: state opening-start")]),
            "new_tab_during_restore": phase(first, "opening-start", "opening-end"),
            "remaining_restoration": phase(first, "opening-end", "restoration-end"),
            "restored_document_checks": phase(first, "restoration-end", "navigation-start"),
            "navigation": phase(first, "navigation-start", "navigation-end"),
            "reload": phase(first, "navigation-end", "reload-end"),
            "hard_reload": phase(first, "reload-end", "hard-reload-end"),
            "fast_new_tab": phase(first, "hard-reload-end", "fast-end"),
        }
        expected = {key: int(key in {"new_tab_during_restore", "navigation", "reload", "hard_reload"}) for key in counts}
        assert counts == expected, f"ready cues: {counts}; expected {expected}; inspect {out}"
        assert cues(first) == 4, "unexpected ready cue outside the checked phases"
        # Relaunch the profile written by the native app, including both windows.
        second = run("relaunch", ["wait 21000", "assertwindows 2", "asserttabs 10", "pagestate restarted-end"])
        counts["relaunch"] = cues(second)
        assert counts["relaunch"] == 0, f"relaunch replayed {counts['relaunch']} page cues"
        with lock:
            seen = list(requests)
        for path in restored + ["/redirected?delay=6", "/split?delay=8.5", "/window-a?delay=3", "/window-b?delay=11"]:
            # Navigation replaced the fast first page; the redirect is saved
            # under its final URL. Both other windows and the split still reload.
            expected_requests = 1 if path in {"/fast?delay=0.01", "/redirect"} else 2
            assert sum(record["path"] == path for record in seen) >= expected_requests, f"page did not load: {path}"
        result = {"passed": True, "ready_cues": counts, "requests": seen,
                  "binary_sha256": hashlib.sha256(exe.read_bytes()).hexdigest(),
                  "checks": ["staggered session loads", "redirect", "split pane", "two windows",
                             "new tab during restoration", "navigation", "reload", "hard reload", "actual relaunch"]}
        (out / "results.json").write_text(json.dumps(result, indent=2) + "\n")
        print(f"PASS session sounds: {counts}\nEvidence: {out}", flush=True)
    finally:
        stop.set()
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()

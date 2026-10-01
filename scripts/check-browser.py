#!/usr/bin/env python3
"""Exercise a packaged browser, pixels and native failure/retry UI in an isolated profile.

Requires Pillow for the screenshot oracle. Never passes --no-sandbox. Linux
needs a desktop (or Xvfb), a usable Vulkan driver and a working sandbox.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('native', ROOT / 'scripts/perf-native.py')
native = importlib.util.module_from_spec(spec)
spec.loader.exec_module(native)


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--app', type=Path, required=True)
    ap.add_argument('--out', type=Path, required=True)
    ap.add_argument('--software', action='store_true', help='also exercise BGRA upload on macOS')
    a = ap.parse_args()
    d = a.out.resolve()
    if d.exists():
        ap.error('choose a new output directory to retain earlier evidence')
    p = d / 'profile'
    p.mkdir(parents=True)
    (p / 'onboarded').write_text('skip')
    (p / 'me.json').write_text(json.dumps({'name': 'Browser fixture', 'face': 'Initial', 'created': '2026-09-29'}))
    (p / 'settings.json').write_text(json.dumps({'behavior': {'splash': 'None', 'new_window': 'Prompt', 'then': 'Prompt', 'keep_alive': 'Off', 'close_asks': False, 'update_checks': False}, 'motion': {'register': 0.5, 'reduce': True}, 'window_rect': [60, 60, 1100, 900]}))
    counts = {}
    finished = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            counts[self.path] = counts.get(self.path, 0) + 1
            if self.path == '/stall' and counts[self.path] == 1:
                finished.wait(90)  # no headers: a responsive renderer cannot resolve this navigation
                return
            title = b'nus browser fixture two' if self.path == '/two' else b'nus browser fixture'
            body = b'<!doctype html><title>' + title + b'</title><style>html,body{margin:0;min-height:100vh;background:rgb(17,177,131);color:white;font:24px sans-serif}</style><h1>Browser pixels arrived</h1><input value="editable"><script>window.fixture=6*7</script>'
            self.send_response(200)
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    url = f'http://127.0.0.1:{server.server_port}'
    shot = d / 'run.shot'
    shot.write_text('\n'.join([
        'wait 1200', 'home', f'tab {url}/ok', 'awaitpage nus browser fixture', 'awaitpaint',
        'eval window.fixture', 'awaitreply 42', 'wait 300', 'shot browsing',
        # Back by swipe, in steps big enough that a slow runner's ticks do not
        # matter. Fingers lifted short of the distance let it go: the glide
        # after must not navigate. A full swipe goes back once, and its glide
        # is spent (a second back would close this new tab).
        # Typed, not scripted: Chromium skips history a script adds without a gesture.
        f'url {url}/two', 'awaitpage nus browser fixture two', 'awaitpaint',
        'wheel 0.5 0.5 20 0 start', 'wheel 0.5 0.5 0 0 end', 'wheel 0.5 0.5 300 0', 'wheel 0.5 0.5 300 0',
        'wait 700', 'awaitpage nus browser fixture two',
        'wheel 0.5 0.5 300 0 start', 'awaitpage nus browser fixture',
        'wheel 0.5 0.5 0 0 end', 'wheel 0.5 0.5 300 0', 'wheel 0.5 0.5 300 0', 'wait 700',
        'awaitpage nus browser fixture', 'awaitpaint',
        # No answer in 30 s: nus says so and keeps waiting (↵); retry is the second command.
        f'tab {url}/stall', 'awaittranscript slow', 'wait 200', 'shot timeout',
        'key down', 'key enter', 'awaitpage nus browser fixture', 'awaitpaint',
        'browsercrash', 'awaittranscript crash', 'wait 200', 'shot crash',
        'key enter', 'awaitpage nus browser fixture', 'awaitpaint',
        'eval while(true){}', 'awaittranscript hung', 'wait 200', 'shot hung',
        'key down', 'key enter', 'awaittranscript crash', 'wait 500',
        'key enter', 'awaitpage nus browser fixture', 'awaitpaint',
        'perfstats browser-complete',
    ]) + '\n')
    env = native.run_environment(d, shot)
    env.update(NUS_SHOT_SIZE='1100x900', NUS_MODE='paper', NUS_CEF_LOG=str(d / 'chromium.log'))
    if a.software:
        env['NUS_SOFTWARE_PAINT'] = '1'
    exe = native.executable_for(a.app)
    try:
        with (d / 'run.log').open('w') as log:
            proc = subprocess.Popen(native.launch_command(exe), cwd=d, env=env, stdout=log,
                                    stderr=subprocess.STDOUT, start_new_session=(os.name == 'posix'))
            try:
                code = proc.wait(timeout=150)
            except subprocess.TimeoutExpired:
                native.terminate_run(proc)
                # The step it stalled on and the app's own reason, not just a timeout.
                tail = (d / 'run.log').read_text(errors='replace')[-4000:]
                raise AssertionError(f'packaged browser did not finish within 150 s\n{tail}') from None
            except BaseException:
                native.terminate_run(proc)
                raise
        output = (d / 'run.log').read_text(errors='replace')
        assert code == 0 and 'panicked' not in output, output[-4000:]
        assert any(label == 'browser-complete' for label, _ in native.parse_records(output, 'PERF')), output[-4000:]
        with Image.open(d / 'screens/browsing-paper.png') as im:
            # Actual window pixels, not paint callback counts. An empty imported
            # texture, swapped BGRA channels or a blank frame cannot pass.
            teal = sum(1 for r, g, b in im.convert('RGB').getdata()
                       if r < 40 and 155 < g < 200 and 110 < b < 155)
        assert teal > 20000, f'page pixels missing or wrong channels: {teal}'
        assert counts.get('/stall', 0) >= 3, counts
        (d / 'results.json').write_text(json.dumps({'passed': True, 'requests': counts,
            'page_pixels': teal, 'binary_sha256': native.sha256(exe),
            'checks': ['HTTP', 'JavaScript', 'page pixels', 'slow load notice', 'renderer crash', 'renderer hang', 'native keyboard retry']}, indent=2) + '\n')
        print('PASS: packaged browsing, pixels, native timeout/crash/hang screens and retries')
    finally:
        finished.set()
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()

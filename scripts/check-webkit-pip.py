#!/usr/bin/env python3
"""WebKit PiP regression using only loopback media and a disposable profile.

Usage: python3 scripts/check-webkit-pip.py /path/to/nus.app
Requires macOS desktop access and ffmpeg with libx264. Input is synthesized
through nus's application handlers, not macOS event delivery. The saved PNGs
prove the app's control overlay rendered; they do not capture WKWebView's
separate native video layer or prove first-click/first-responder delivery.
"""
import http.server
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time


def main():
    if sys.platform != 'darwin':
        raise SystemExit('This check requires macOS WebKit and desktop access.')
    bundle = Path(sys.argv[1] if len(sys.argv) > 1 else 'dist/nus.app').resolve()
    executable = bundle / 'Contents/MacOS/nus'
    assert executable.is_file(), f'No app executable: {executable}'
    ffmpeg = shutil.which('ffmpeg')
    assert ffmpeg, 'ffmpeg with libx264 is required for the local fixture'
    root = Path(tempfile.mkdtemp(prefix='nus-webkit-pip-check-'))
    print(f'Evidence: {root}', flush=True)
    movie = root / 'fixture.mp4'
    subprocess.run([ffmpeg, '-nostdin', '-loglevel', 'error', '-f', 'lavfi', '-i',
                    'testsrc2=size=640x360:rate=8', '-t', '70', '-an', '-c:v',
                    'libx264', '-preset', 'ultrafast', '-crf', '30', '-pix_fmt',
                    'yuv420p', '-movflags', '+faststart', str(movie)], check=True)
    media = movie.read_bytes()
    player = b'''<!doctype html><meta charset="utf-8"><title>WebKit PiP fixture</title>
<style>body{margin:0;background:#b84060}.wrapper{margin:50px;transform:translateZ(0);overflow:hidden;width:640px;height:360px}video{width:640px;height:360px;object-fit:contain}</style>
<div class="wrapper"><video src="/fixture.mp4" muted playsinline preload="auto" disablepictureinpicture controlslist="nodownload noplaybackrate"></video></div>
<script>const v=document.querySelector('video');v.addEventListener('loadedmetadata',()=>{v.pause();v.currentTime=30;});</script>'''
    nested = b'''<!doctype html><meta charset="utf-8"><title>Nested WebKit PiP fixture</title>
<style>body{margin:0;background:#40b880}iframe{margin:30px;border:4px solid #ffeebb;width:780px;height:500px}</style><iframe src="/player"></iframe>'''
    requests = []
    began = time.monotonic()

    class Fixture(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            path = self.path.split('?', 1)[0]
            requests.append({'at_ms': round((time.monotonic() - began) * 1000),
                             'path': path, 'user_agent': self.headers.get('User-Agent', ''),
                             'range': self.headers.get('Range')})
            if path == '/player':
                body, content_type = player, 'text/html; charset=utf-8'
            elif path == '/nested':
                body, content_type = nested, 'text/html; charset=utf-8'
            elif path == '/fixture.mp4':
                body, content_type = media, 'video/mp4'
            else:
                self.send_error(404)
                return
            start, end, code = 0, len(body) - 1, 200
            value = self.headers.get('Range')
            if value and path == '/fixture.mp4':
                match = re.fullmatch(r'bytes=(\d+)-(\d*)', value)
                if not match:
                    self.send_error(416)
                    return
                start = int(match[1])
                end = min(int(match[2]) if match[2] else end, end)
                if start > end:
                    self.send_response(416)
                    self.send_header('Content-Range', f'bytes */{len(body)}')
                    self.end_headers()
                    return
                code = 206
            self.send_response(code)
            self.send_header('Content-Type', content_type)
            self.send_header('Accept-Ranges', 'bytes')
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(end - start + 1))
            if code == 206:
                self.send_header('Content-Range', f'bytes {start}-{end}/{len(body)}')
            self.end_headers()
            try:
                self.wfile.write(body[start:end + 1])
            except (BrokenPipeError, ConnectionResetError):
                pass  # WebKit can abandon its initial metadata range.

        def log_message(self, *_):
            pass

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    address = f'http://127.0.0.1:{server.server_port}'
    directory = root / 'run'
    profile = directory / 'profile'
    profile.mkdir(parents=True)
    (profile / 'onboarded').write_text('skip')
    (profile / 'settings.json').write_text(json.dumps({
        'schema': 2,
        'behavior': {'hatch_background': False, 'hatch_status': False,
                     'splash': 'None', 'pip_skip_seconds': 17},
        'motion': {'register': 0.5, 'reduce': True},
    }))
    steps = ['wait 1600', 'foreground', 'wait 150']
    for name in ['player', 'nested']:
        steps += [f'tab {address}/{name}', 'wait 9000', 'assertnativevideo',
                  'videostart', 'wait 800', 'assertvideotime 30',
                  'assertvideostate paused', 'assertvideostate muted',
                  'videocommand __nus.toggle()', 'wait 600', 'assertvideostate playing',
                  'assertautopip ineligible',
                  'toastfixture info|Playing Here|Silent previews stay on the page|tab',
                  'wait 500', 'asserttoastinpage', f'shot webkit-{name}-toast',
                  'toastpress cell', 'asserttoastgone',
                  'videostart', 'wait 500',
                  'pip', 'wait 800', 'assertnativepip on', 'piptransportcheck',
                  f'shotpip webkit-{name}-controls', f'shot webkit-{name}-notice',
                  'pipskipburst', 'wait 700', 'assertvideostate paused', 'assertvideotime 30',
                  'pipclick play', 'wait 200', 'pipskipburst', 'wait 800', 'assertvideostate playing',
                  'videostart', 'wait 500', 'assertvideostate paused', 'assertvideotime 30',
                  'pipclick forward', 'wait 500', 'assertvideotime 47',
                  'pipclick back', 'wait 500', 'assertvideotime 30',
                  'pipkeys right', 'wait 500', 'assertvideotime 47',
                  'pipkeys left', 'wait 500', 'assertvideotime 30',
                  'pipscrub 0.5', 'wait 600', 'assertvideotime 35',
                  'pipclick mute', 'wait 500', 'assertvideostate unmuted',
                  'pipclick mute', 'wait 500', 'assertvideostate muted',
                  'pipclick play', 'wait 600', 'assertvideostate playing',
                  'pipclick play', 'wait 600', 'assertvideostate paused',
                  'videostart', 'wait 500', 'assertvideotime 30',
                  'pipclick return', 'wait 600', 'pipassert closed',
                  'assertnativepip off', 'assertnativevideo', 'assertvideotime 30',
                  'pip', 'wait 600', 'assertnativepip on', 'piptransportcheck',
                  'pipnoticeclick', 'wait 600', 'pipassert closed', 'assertnativepip off', 'assertvideotime 30']
    script = directory / 'check.shot'
    script.write_text('\n'.join(steps) + '\n')
    env = dict(os.environ, NUS_SHOT_DIR=str(directory), NUS_SHOT=str(script),
               NUS_SHOT_OUT=str(directory / 'screens'), NUS_SHOT_SIZE='1200x850',
               NUS_WEBKIT_ALSO='127.0.0.1', NUS_MODE='paper', RUST_BACKTRACE='1',
               RUST_LOG='warn,composite::browser=debug,composite::webkit=debug')
    env.pop('NUS_SHOT2', None)
    log = directory / 'run.log'
    cli = bundle / 'Contents/Resources/bin/nus'
    info = []
    cli_env = dict(env, NUS_INSTANCE=str(profile / 'instance'))
    previous = None
    process = None
    try:
        with log.open('w') as output:
            process = subprocess.Popen([str(executable)], cwd=directory, env=env,
                                       stdout=output, stderr=subprocess.STDOUT)
            deadline = time.monotonic() + 130
            while process.poll() is None:
                if time.monotonic() > deadline:
                    raise TimeoutError(f'WebKit check timed out; see {log}')
                if cli.is_file() and (profile / 'instance').is_file():
                    try:
                        reply = subprocess.run([str(cli), 'page', 'info', '--json'],
                                               cwd=directory, env=cli_env, capture_output=True,
                                               text=True, timeout=2)
                        try:
                            state = json.loads(reply.stdout)
                        except json.JSONDecodeError:
                            state = {'code': reply.returncode, 'error': reply.stderr[-1000:]}
                    except subprocess.TimeoutExpired:
                        state = {'error': 'page info timed out'}
                    if state != previous:
                        previous = state
                        info.append({'at_ms': round((time.monotonic() - began) * 1000), 'page': state})
                        (root / 'page-info.json').write_text(json.dumps(info, indent=2) + '\n')
                time.sleep(0.4)
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait()
        server.shutdown()
        server.server_close()
        (root / 'requests.json').write_text(json.dumps(requests, indent=2) + '\n')
        (root / 'page-info.json').write_text(json.dumps(info, indent=2) + '\n')
    text = log.read_text()
    assert process.returncode == 0 and 'panicked' not in text, f'{log}\n{text[-6000:]}'
    screenshots = sorted((directory / 'screens').glob('webkit-*-controls-*.png'))
    assert len(screenshots) == 2, f'Missing overlay images: {screenshots}'
    for shot in screenshots:
        image = shot.read_bytes()
        assert image.startswith(b'\x89PNG\r\n\x1a\n')
        width, height = struct.unpack('>II', image[16:24])
        assert width > 100 and height > 70 and b'IDAT' in image
    report = {
        'status': 'passed',
        'engine': 'WebKit (assertnativevideo)',
        'fixtures': ['top document', 'same-origin nested frame'],
        'asserted': ['native PiP attachment/return', 'control geometry and rendered overlay',
                     'muted playback excluded from automatic PiP', 'toast inside page viewport',
                     'source pane PiP notice captured and clicked to return',
                     '40 rapid skip presses preserve playing and paused states', 'play/pause', '17-second click/key seeks', 'scrub', 'mute/unmute',
                     'reopen/close preserving playback position'],
        'screenshots': [str(path) for path in screenshots],
        'scope': 'Application-level synthetic input and GPU control-overlay capture. '
                 'Does not verify macOS first-click/key delivery, WK native layer composition, '
                 'cross-origin frames, DRM, or any streaming service.',
    }
    (root / 'validation.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2), flush=True)


if __name__ == '__main__':
    main()

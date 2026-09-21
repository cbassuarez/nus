#!/usr/bin/env python3
"""Run the ACTUAL injected extraction/capture sources in a local Chromium.

Uses no Internet pages, never reads a real nus profile, and starts only temporary
loopback fixture servers. Requires Python playwright and Pillow plus Chromium.
No native Rust/CEF/GPU pass is implied by these browser-fixture results.
"""
from __future__ import annotations
import argparse
import base64
import io
import json
import random
import threading
import traceback
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse
from PIL import Image
from playwright.sync_api import sync_playwright, Error as BrowserError

ROOT = Path(__file__).resolve().parents[2]
ART = ROOT / 'spikes/composite/assets/library'
ARTICLE = (ART / 'article.js').read_text()
CAPTURE = (ART / 'capture.js').read_text()
TOKEN = 'a' * 32
CODE = 'function example() {\n\treturn "exact spacing";  \n}\n'
PROSE = ('This article exists to test saved reading without depending on a live external website. '
         'The original document, code, links and figures must survive extraction without hidden content. ')

def png(width=160, height=80, noise=False):
    if noise:
        rng = random.Random(187)
        image = Image.frombytes('RGB', (width, height), rng.randbytes(width * height * 3))
    else:
        image = Image.new('RGB', (width, height), (35, 46, 61))
    out = io.BytesIO(); image.save(out, 'PNG'); return out.getvalue()

class Fixtures:
    def __init__(self):
        self.requests = []
        self.html = ''
        self.images = {'/figure.png': png(), '/large.png': png(600,350,True)}
        owner = self
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                path = urlparse(self.path).path
                owner.requests.append(self.path)
                if path in owner.images:
                    body, mime, status = owner.images[path], 'image/png', 200
                elif path == '/missing.png':
                    body, mime, status = b'missing', 'text/plain', 404
                else:
                    body, mime, status = owner.html.encode(), 'text/html; charset=utf-8', 200
                self.send_response(status); self.send_header('Content-Type', mime)
                self.send_header('Content-Length', str(len(body))); self.end_headers()
                self.wfile.write(body)
            def log_message(self, *_): pass
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.server.daemon_threads = True
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        self.url = f'http://127.0.0.1:{self.server.server_port}'
    def close(self): self.server.shutdown(); self.server.server_close(); self.thread.join()

def main():
    ap=argparse.ArgumentParser(); ap.add_argument('--chromium',default='/usr/bin/chromium')
    ap.add_argument('--report',type=Path); ap.add_argument('--offline-dom',action='store_true',help='Use set_content/data images when managed browser policy prohibits loopback navigation; real cross-origin test is skipped.'); args=ap.parse_args()
    local, foreign = Fixtures(), Fixtures()
    tests=[]
    try:
        with sync_playwright() as pw:
            browser=pw.chromium.launch(executable_path=args.chromium, headless=True,
                                      args=['--no-sandbox','--disable-background-networking'])
            context=browser.new_context()
            context.route('**/*',lambda route: route.continue_() if urlparse(route.request.url).hostname=='127.0.0.1' else route.abort())
            page=context.new_page()
            network_requests=[]
            page.on('request', lambda request: network_requests.append(request.url))
            def load(body):
                local.html='<!doctype html><meta charset="utf-8"><title>Fixture</title>'+body
                if args.offline_dom:
                    body = body.replace('src="/figure.png"','src="data:image/png;base64,'+base64.b64encode(local.images['/figure.png']).decode()+'"')
                    body = body.replace('src="/large.png"','src="data:image/png;base64,'+base64.b64encode(local.images['/large.png']).decode()+'"')
                    page.set_content('<!doctype html><meta charset="utf-8"><base href="'+local.url+'/"><title>Fixture</title>'+body,wait_until='load')
                else:
                    # Repeating a URL with a fragment can be a same-document
                    # navigation: the next case would silently test the previous
                    # fixture. Navigate away before loading the changed body.
                    page.goto('about:blank')
                    page.goto(local.url+'/article?q=1#section',wait_until='networkidle')
            def extract(): return json.loads(page.evaluate(ARTICLE))
            def capture(): return json.loads(page.evaluate(f'({CAPTURE})({json.dumps(TOKEN)},({ARTICLE}))'))
            def case(name,fn):
                try:
                    fn()
                except Exception as error:
                    tests.append({'name':name,'status':'FAIL','error':str(error)})
                    print('FAIL',name,flush=True)
                    traceback.print_exc()
                else:
                    tests.append({'name':name,'status':'PASS'})
                    print('PASS',name,flush=True)
            def basic():
                load(f'<article><h1>Saved reading fixture</h1><p>{PROSE}</p><pre>{CODE.replace("&","&amp;").replace("<","&lt;")}</pre></article>')
                a=extract(); assert a['title']=='Saved reading fixture'
                assert next(b['x'] for b in a['blocks'] if b['t']=='pre')==CODE
            case('preserves title and exact code whitespace',basic)
            def hidden():
                load(f'<article><h1>Visible</h1><p>{PROSE}<span hidden>SECRET_HIDDEN</span><span style="display:none">SECRET_CSS</span><span aria-hidden="true">SECRET_ARIA</span><input value="SECRET_INPUT"></p><div hidden><p>SECRET_PARENT {PROSE}</p></div></article>')
                assert 'SECRET_' not in json.dumps(extract())
            case('excludes hidden descendants and input fields',hidden)
            def hidden_ancestry():
                for hidden in ['hidden', 'style="display:none"', 'aria-hidden="true"']:
                    load(f'<main><h1>Visible article</h1><p>{PROSE}</p></main><div {hidden}><article><h1>Hidden draft</h1><p>{("PRIVATE_UNPUBLISHED " + PROSE) * 4}</p></article></div>')
                    article=extract()
                    assert article['title']=='Visible article' and 'PRIVATE_UNPUBLISHED' not in json.dumps(article), article
                load(f'<div hidden><h1>Hidden fallback title</h1></div><article><p>{PROSE}</p></article>')
                assert extract()['title']=='Fixture'
            case('excludes hidden ancestor drafts and hidden fallback titles',hidden_ancestry)
            def references():
                load(f'<article><p>{PROSE}<a href="/docs?q=7#usage">Documentation</a><a href="javascript:alert(1)">Unsafe script</a><a href="https://u:p@example.com/">Credentials</a><a href="data:text/plain,secret">Inline</a></p></article>')
                links=[b for b in extract()['blocks'] if b['t']=='link']
                assert len(links)==1 and links[0]['src']==local.url+'/docs?q=7#usage', links
            case('keeps query and fragment; rejects unsafe link schemes and credentials',references)
            def images():
                load(f'<article><p>{PROSE}</p><img src="/figure.png" alt="Diagram"><img src="{foreign.url}/figure.png" alt="Foreign"><img src="/missing.png" width="160" alt="Missing"></article>')
                result=capture(); assert result['token']==TOKEN and result['url']==local.url+'/article?q=1#section'
                assert len(result['media'])==1 and result['missing_images']==2, {
                    'media': len(result['media']), 'missing': result['missing_images'],
                    'blocks': result['document']['blocks'],
                    'images': page.evaluate('Array.from(document.images, i => ({html:i.outerHTML,width:i.width,naturalWidth:i.naturalWidth,display:getComputedStyle(i).display}))')}
                m=result['media'][0]; img=Image.open(io.BytesIO(bytes.fromhex(m['png'])))
                assert img.size==(160,80) and m['width']==160 and m['height']==80
                assert [b['src'] for b in result['document']['blocks'] if b['t']=='img']==['image-0','','']
            if args.offline_dom:
                tests.append({'name':'captures origin-clean images; identifies tainted and missing images','status':'SKIP','reason':'Managed loopback navigation unavailable; offline DOM mode cannot establish independent HTTP origins.'})
                def inline_images():
                    load(f'<article><p>{PROSE}</p><img src="/figure.png" alt="Diagram"></article>')
                    result=capture(); assert result['url']=='about:blank' and result['token']==TOKEN
                    assert len(result['media'])==1 and result['missing_images']==0
                    image=Image.open(io.BytesIO(bytes.fromhex(result['media'][0]['png'])))
                    assert image.size==(160,80)
                case('captures a loaded origin-clean inline PNG',inline_images)
            else:
                case('captures origin-clean images; identifies tainted and missing images',images)
            def unavailable_alt():
                load(f'<article><p>{PROSE}</p><img src="/missing.png" alt="Unavailable diagram: a cycle connecting three stages."></article>')
                result=capture()
                figures=[b for b in result['document']['blocks'] if b['t']=='img']
                assert len(figures)==1 and figures[0]['x']=='Unavailable diagram: a cycle connecting three stages.' and figures[0]['src']=='', figures
                assert result['missing_images']==1 and not result['media']
            case('retains meaningful alternative text when image dimensions are unavailable',unavailable_alt)
            def large():
                load(f'<article><p>{PROSE}</p><img src="/large.png" alt="Large transport fixture"></article>')
                result=capture(); assert result['document']['blocks'][0]['x']==PROSE.strip()
                assert len(result['media'])==0 and result['missing_images']==1
                wire=json.dumps({'result':{'type':'string','value':json.dumps(result,separators=(',',':'),ensure_ascii=False)}},separators=(',',':'),ensure_ascii=False)
                assert len(wire.encode())<900000
            case('omits oversized images without increasing the existing CDP limit',large)
            def epoch():
                load(f'<article><p>{PROSE}</p></article>')
                result=capture()
                assert isinstance(result['epoch'],(float,int)) and result['epoch']>0
                assert result['epoch']==page.evaluate('performance.timeOrigin')
            case('binds each capture to its document epoch',epoch)
            def deep_image():
                body=png(2049,2)
                source='data:image/png;base64,'+base64.b64encode(body).decode()
                load(f'<article><p>{PROSE}</p><img src="{source}"></article>')
                result=capture(); assert not result['media'] and result['missing_images']==1
            case('refuses images exceeding dimension limits',deep_image)
            def dedupe():
                load(f'<article><p>{PROSE}</p><img src="/figure.png"><img src="/figure.png"></article>')
                c=capture(); assert len(c['media'])==1
                assert [b['src'] for b in c['document']['blocks'] if b['t']=='img']==['image-0','image-0']
            case('deduplicates repeated image payloads',dedupe)
            def nonmutating():
                load(f'<article><p>{PROSE}</p><img src="/figure.png"></article>')
                before=page.content(); n=len(network_requests)
                page.evaluate('window.__mutations=0; new MutationObserver(r=>window.__mutations+=r.length).observe(document,{childList:true,subtree:true,attributes:true,characterData:true});')
                capture(); page.wait_for_timeout(100)
                assert page.content()==before and page.evaluate('window.__mutations')==0
                assert len(network_requests)==n
            case('does not mutate the live DOM or initiate network requests',nonmutating)
            def table():
                load(f'<article><p>{PROSE}</p><table><tr><th>Name</th><th>Value</th></tr><tr><td>A</td><td>42</td></tr><tr><td colspan="2">Merged content</td></tr></table></article>')
                blocks=extract()['blocks']; text='\n'.join(b['x'] for b in blocks)
                assert all(s in text for s in ['Name','Value','42','Merged content','merged cells'])
                assert any(b['t']=='pre' and ' | ' in b['x'] for b in blocks)
            case('preserves table cells and discloses merged-cell layout fallback',table)
            def multilingual():
                phrase='Caracas: acción y lectura. 日本語の文章。 العربية تبقى مقروءة.'
                load(f'<article><p>{PROSE}{phrase}</p></article>')
                assert phrase in extract()['blocks'][0]['x']
            case('retains accented, CJK and RTL text in the captured document',multilingual)
            def nested():
                load(f'<article><p>{PROSE}</p><ul><li>Parent<ul><li>Child</li></ul></li></ul></article>')
                items=[b['x'] for b in extract()['blocks'] if b['t']=='li']; assert items==['Parent','Child']
            case('does not duplicate nested list text',nested)
            def rejects(body):
                load(body)
                try: extract()
                except BrowserError: return
                raise AssertionError('Expected an explicit extraction failure, not a partial article')
            case('rejects non-article pages',lambda:rejects('<main><p>Hello</p></main>'))
            case('rejects sign-in surfaces',lambda:rejects(f'<main><p>{PROSE}</p><form><input type="password"></form></main>'))
            case('rejects block overflow instead of truncating',lambda:rejects('<article>'+('<p>'+PROSE+'</p>')*4097+'</article>'))
            case('rejects node overflow',lambda:rejects('<article><p>'+PROSE+'</p>'+('<b>x</b>'*20001)+'</article>'))
            case('rejects text overflow',lambda:rejects('<article><p>'+('a '*(1024*1024+2))+'</p></article>'))
            case('rejects link overflow',lambda:rejects('<article><p>'+PROSE+''.join(f'<a href="/x{i}">link {i}</a>' for i in range(257))+'</p></article>'))
            def fallback():
                load(f'<article><p>{PROSE}</p><video></video><math><mi>x</mi></math></article>')
                assert any('not captured' in b['x'] for b in extract()['blocks'])
            case('labels unsupported media rather than silently dropping it',fallback)
            browser.close()
    finally: local.close(); foreign.close()
    report={'suite':'real Chromium extraction/capture fixtures','mode':'offline DOM' if args.offline_dom else 'loopback HTTP','tests':tests,'passed':sum(t['status']=='PASS' for t in tests),'failed':sum(t['status']=='FAIL' for t in tests),'skipped':sum(t['status']=='SKIP' for t in tests),
            'native_rust_build':'NOT RUN by this script','native_ui':'NOT RUN by this script'}
    if args.report: args.report.parent.mkdir(parents=True,exist_ok=True); args.report.write_text(json.dumps(report,indent=2)+'\n')
    print(f"{report['passed']} browser-fixture tests passed; {report['failed']} failed; {report['skipped']} skipped.")
    if report['failed']: raise SystemExit(1)
if __name__=='__main__': main()

#!/usr/bin/env python3
"""Real CEF page zoom and raster-density checks, in a disposable profile."""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split('base=run(')[0], 'check-settings.py', 'exec'))
fixture=root/'zoom.md'
fixture.write_text('# Sharp at every size\n\nFreshly rendered **text**, curves, and a linked image.\n\n![Vector illustration](zoom.svg)\n\n```rust\nfn main() { println!("nus"); }\n```\n')
(root/'zoom.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="720" height="240" viewBox="0 0 720 240"><rect width="720" height="240" fill="#eadcc0"/><circle cx="140" cy="120" r="88" fill="#b34431"/><path d="M280 200 360 40 440 200 520 40 600 200" stroke="#333129" stroke-width="3" fill="none"/></svg>')
prefs={'window_rect':[60,60,1100,900],'motion':{'reduce':False,'register':.5},'behavior':{'splash':'None','update_checks':False}}
try:
    run('page-zoom',f'''tab {fixture.as_uri()}
wait 1800
assertzoom 100
eval document.querySelector('article p').scrollIntoView(); 'ready'
wait 250
assertreply ready
shot zoom-100
zoomprobe start
wait 180
zoomprobe middle
wait 1200
zoomprobe end
assertchrome
eval document.querySelector('article p').scrollIntoView(); 'ready'
wait 250
assertreply ready
shot zoom-150
zoomprobe reduce
assertzoom 100
zoomprobe dpi
wait 500
eval document.querySelector('img').complete && document.querySelector('img').naturalWidth === 720 && !document.documentElement.style.transform && !document.body.style.transform
wait 300
assertreply true
key cmd+0
wait 400
assertzoom 100
''',prefs)
finally:
    for marker in root.glob('*/profile/.vault-id'):
        subprocess.run(['/usr/bin/security','delete-generic-password','-s','dev.nus.local-state.v1','-a',marker.read_text()],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)

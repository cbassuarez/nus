#!/usr/bin/env python3
"""Native Dock readback: promo frames, theme colours, and reduced motion.

Uses isolated app profiles. NUS_DOCK_TRACE records NSApplication's installed
NSImage as TIFF, so these checks exercise the native icon setter too.
"""
from pathlib import Path
exec(compile(Path(__file__).with_name('check-settings.py').read_text().split("base=run(")[0], 'check-settings.py', 'exec'))
import plistlib
import time
os.environ['NUS_SHOT_INTERACTIVE']='1'
for app in [bundle,*bundle.glob('Contents/Frameworks/*Helper*.app')]:
    plist=plistlib.loads((app/'Contents/Info.plist').read_bytes())
    icon=app/'Contents/Resources'/plist['CFBundleIconFile']
    assert icon.read_bytes()[:4]==b'icns',icon

def events(name):
    return [json.loads(row) for row in (root/name/'dock/events.jsonl').read_text().splitlines()]

os.environ['NUS_DOCK_TRACE']=str(root/'theme-cycle/dock')
os.environ['NUS_DOCK_LAUNCH_TEST_MS']='1400'
profile=run('theme-cycle','wait 900\ntheme nord\nwait 1100\ntheme dracula\nwait 1100\ntheme broadsheet\nwait 1100',{'motion':{'register':0.5,'reduce':False}})
os.environ.pop('NUS_DOCK_LAUNCH_TEST_MS')
rows=events('theme-cycle')
expected=['Plex','Silkscreen','Plex Italic','Bungee','Rubik Mono','Newsreader']
assert rows[0]['event']=='bootstrap' and rows[0]['face']=='Newsreader',rows[0]
launch=[row for row in rows if row['event']=='launch-frame']
assert [row['face'] for row in launch][:12]==expected*2,launch
assert [row['face'] for row in launch]==[expected[i%6] for i in range(len(launch))],launch
ready=next(i for i,row in enumerate(rows) if row['event']=='ready')
settled_at=next(i for i,row in enumerate(rows) if row['event']=='launch-settled')
assert settled_at>=ready,rows
assert not any(row['event']=='launch-frame' for row in rows[settled_at+1:]),rows
# Fast launch, without the old artificial run-loop hold, must show all six faces.
os.environ['NUS_DOCK_TRACE']=str(root/'fast-launch/dock')
run('fast-launch','wait 1400',{'motion':{'register':0.5,'reduce':False}})
fast=events('fast-launch')
faces=[row['face'] for row in fast if row['event']=='launch-frame']
assert faces[:6]==expected,faces
assert all(face==expected[i%6] for i,face in enumerate(faces)),faces
assert not any(row['event']=='attention' for row in fast),fast
assert not any(row['event']=='attention' for row in rows),rows
settled=[row for row in rows if row['face']=='Newsreader']
colours={tuple(round(v*255) for v in row['signal'][:3]) for row in settled}
assert {(136,192,208),(189,147,249),(200,16,46)}<=colours,colours
assert any(row['event']=='bundle' for row in rows),rows
for colour in ['c8102e','88c0d0','bd93f9']:
    assert (root/'theme-cycle/dock'/f'menu-{colour}.tiff').is_file(),colour
for path in (root/'theme-cycle/dock').glob('*.tiff'):
    data=path.read_bytes()
    assert data[:4] in (b'II*\0',b'MM\0*') and len(data)>1000,path
prefs=json.loads((profile/'settings.json').read_text())
# Request attention while our own app is hidden. It must cycle more than once,
# coalesce a second request, stop promptly on focus, and restore the final face.
os.environ['NUS_DOCK_TRACE']=str(root/'attention-loop/dock')
run('attention-loop','wait 1100\nbackground\nwait 200\ndockattention\nwait 500\ndockattention\nwait 1600',prefs)
attention=events('attention-loop')
begin=next(i for i,row in enumerate(attention) if row['event']=='attention')
frames=[row['face'] for row in attention[begin:] if row['event']=='frame']
assert frames[:12]==expected*2,frames
assert frames[-1]=='Newsreader',frames
assert sum(row['event']=='attention' for row in attention)==1,attention
assert sum(row['event']=='attention-ended' for row in attention)==1,attention
os.environ['NUS_DOCK_TRACE']=str(root/'attention-focus/dock')
run('attention-focus','wait 1100\nbackground\nwait 200\ndockattention\nwait 300\nforeground\nwait 650',prefs)
focus=events('attention-focus')
start=next(row['ms'] for row in focus if row['event']=='attention')
end=next(row['ms'] for row in focus if row['event']=='attention-ended')
assert end-start<850,(start,end)
assert [row['face'] for row in focus if row['event']=='frame'][-1]=='Newsreader',focus
prefs['motion']['reduce']=True
os.environ['NUS_DOCK_TRACE']=str(root/'reduced/dock')
run('reduced','wait 900\ntheme nord\nwait 1100\ndockattention\nwait 800',prefs)
assert all(row['face']=='Newsreader' for row in events('reduced') if row['face']),events('reduced')
assert not any(row['event']=='launch-frame' for row in events('reduced'))
assert not any(row['event']=='attention' for row in events('reduced'))
prefs['behavior'].update(hatch_notify=True,hatch_status=False,hatch_background=True)
os.environ['NUS_DOCK_TRACE']=str(root/'notice/dock')
run('notice','theme nord\nnewshell\nwait 900\nshell sleep 2; false\nhome\nwait 2800\nhatchassertnotice\nhatchbadgeshot themed-notice',prefs)
# Native macOS Quit can terminate inside the event pump without returning to
# main's ordinary cleanup. Exercise that separate callback with our own child.
directory=root/'native-quit'
(directory/'profile').mkdir(parents=True)
(directory/'profile/onboarded').write_text('skip')
(directory/'profile/settings.json').write_text(json.dumps(prefs))
(directory/'check.shot').write_text('wait 60000\n')
env=dict(os.environ,NUS_SHOT=str(directory/'check.shot'),NUS_SHOT_DIR=str(directory),
    NUS_DOCK_TRACE=str(directory/'dock'),NUS_SHOT_OUT=str(directory/'screens'))
with (directory/'run.log').open('w') as log:
    child=subprocess.Popen([str(bundle/'Contents/MacOS/nus')],env=env,stdout=log,stderr=subprocess.STDOUT)
    try:
        deadline=time.monotonic()+20
        while not (directory/'dock/events.jsonl').exists() or not any(row['event']=='ready' for row in events('native-quit')):
            assert child.poll() is None and time.monotonic()<deadline,'native Quit test did not become ready'
            time.sleep(0.05)
        subprocess.run(['swift','-module-cache-path',str(root/'swift-cache'),
            str(Path(__file__).with_name('check-dock-icon.swift')),'--quit',str(child.pid)],check=True)
        assert child.wait(timeout=15)==0
    finally:
        if child.poll() is None:
            child.terminate()
            child.wait(timeout=10)
quit_icons=[]
for name in ['theme-cycle','attention-loop','attention-focus','reduced','notice','native-quit']:
    row=events(name)[-1]
    assert row['event']=='quit-default' and row['face']=='Newsreader',row
    quit_icons.append(str(root/name/'dock'/f"{row['ms']}-5.tiff"))
subprocess.run(['swift','-module-cache-path',str(root/'swift-cache'),
    str(Path(__file__).with_name('check-dock-icon.swift')),str(bundle),
    str(Path(__file__).resolve().parents[1]/'assets/icon/nus-512-ink.png'),*quit_icons],check=True)
print('Dock launch, quit, theme, native image readback, and motion checks passed.',flush=True)
subprocess.run(['codesign','--verify','--deep','--strict',str(bundle)],check=True)

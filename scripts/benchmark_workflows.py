"""Real NUS-owned action sequences; these are not simulated-human timings."""
from pathlib import Path
import json
import shlex
import socket
import sys
import urllib.request
import uuid

METRICS={'edit-verify':'workflow_edit_verify_v1','port-conflict':'workflow_port_conflict_v1','project-switch':'workflow_project_switch_v1'}


def available_port():
    with socket.socket() as s:
        s.bind(('127.0.0.1',0));return s.getsockname()[1]


def scenario_script(name,directory):
    fixture=Path(__file__).with_name('benchmark_fixture.py').resolve()
    ports=[available_port(),available_port()]
    while ports[1]==ports[0]:ports[1]=available_port()
    token=uuid.uuid4().hex
    (directory/'token').write_text(token)
    (directory/'fixture.json').write_text(json.dumps({'ports':ports,'token':token}))
    for item in ['A','B','U']:(directory/(item+'.txt')).write_text('before')
    def server(item,port,load=False):
        return 'shell '+' '.join(shlex.quote(str(x)) for x in [sys.executable,fixture,directory,item,port])+(' --load' if load else '')
    def page(item,port,text='before'):
        return [f'tab http://127.0.0.1:{port}/?value={text}',f'awaitpage NUS-BENCH-{item}-{text}',"eval document.body.dataset.value",f'awaitreply {text}']
    ready=['awaitperf startup_first_present','assertpresented','asserttabs 1','newshell',server('A',ports[0],name=='project-switch'),f'awaitfile {directory}/A.pid']
    save='cmd+s' if sys.platform=='darwin' else 'ctrl+s'
    if name=='edit-verify':
        steps=ready+['perfreset','benchbegin edit-verify']+page('A',ports[0])+[f'openfile {directory}/A.txt','awaitperf file_open_submit',f'asserteditorpath {directory}/A.txt','asserteditorready 6','editorcursor end','key x',f'key {save}','asserteditorready 7']+page('A',ports[0],'beforex')
    elif name=='port-conflict':
        # Home=0, A=1, unrelated U=2, contender B=3. No fake port rows.
        steps=ready+['newshell',server('U',ports[1]),f'awaitfile {directory}/U.pid','newshell','perfreset','benchbegin port-conflict',server('B',ports[0]),f'awaitfile {directory}/B.conflict','board',f'awaitportowner {ports[0]} 1 {directory}/A.pid',f'benchportjump {ports[0]}','assertpane term','ctrlc',f'awaitfile {directory}/A.stopped','benchtab 3',server('B',ports[0]),f'awaitfile {directory}/B.pid','board',f'awaitportowner {ports[0]} 3 {directory}/B.pid','close']+page('B',ports[0])+page('U',ports[1])
    elif name=='project-switch':
        # Two distinct project buffers; A emits real PTY output throughout.
        steps=ready+[f'awaitfile {directory}/A.load',f'openfile {directory}/A.txt','awaitperf file_open_submit',f'asserteditorpath {directory}/A.txt','editorcursor end','key x','perfreset',f'benchbegin project-switch {directory}/A.load','benchtab 0',f'openfile {directory}/B.txt','awaitperf file_open_submit',f'asserteditorpath {directory}/B.txt','editorcursor end','key y',f'key {save}','benchtab 2',f'asserteditorpath {directory}/A.txt','asserteditorready 7',f'key {save}']+page('A',ports[0],'beforex')
    else:raise ValueError('unknown workflow')
    return '\n'.join(steps+[f'benchend {name}',f'perfstats {name}',f'memory {name}'])+'\n'


def validate(name,directory):
    if name in ('edit-verify','project-switch'):
        if (directory/'A.txt').read_text()!='beforex':raise ValueError('wrong saved project A contents')
    if name=='project-switch':
        if (directory/'B.txt').read_text()!='beforey':raise ValueError('wrong saved project B contents')
        if int((directory/'A.load').read_text())<10:raise ValueError('background load did not run')
    if name=='port-conflict':
        for f in ('B.conflict','B.pid','A.stopped','U.pid'):
            if not (directory/f).is_file():raise ValueError('missing conflict/continuity evidence: '+f)
    return {'schema':1,'scenario':name,'passed':True}


def cleanup(directory):
    data=json.loads((directory/'fixture.json').read_text())
    for port in data['ports']:
        try:
            with urllib.request.urlopen(f'http://127.0.0.1:{port}/stop/{data["token"]}',timeout=1):pass
        except (OSError,TimeoutError):pass

import json,os,sys,time
from pathlib import Path
p=Path(sys.argv[1]);print('NUS benchmark fixture: terminal ready',flush=True);(p/'terminal-ready.json').write_text(json.dumps({'pid':os.getpid()}))
end=time.monotonic()+180
while time.monotonic()<end and not (p/'stop').exists():time.sleep(.1)

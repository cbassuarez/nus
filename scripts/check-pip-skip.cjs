// The real injected skip queue with a deliberately lagging/pausing player.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('spikes/composite/assets/video.js','utf8');
function fixture(service=false) {
  let now=0,id=0,time=30,paused=false,playCalls=0,failSeek=false;
  const timers=new Map(),events={},docEvents={},seeks=[],reports=[];
  const v={tagName:'VIDEO',isConnected:true,readyState:4,ended:false,muted:false,volume:1,
    currentSrc:'movie-a',duration:1000,seekable:{length:0},
    get currentTime(){return time},set currentTime(t){if(failSeek)throw new Error('seek failed');seeks.push(t);paused=true},
    get paused(){return paused},play(){playCalls++;paused=false;return Promise.resolve()},pause(){paused=true},
    getBoundingClientRect:()=>({width:640,height:360,left:0,top:0}),
    addEventListener:(k,f)=>events[k]=f,removeEventListener:(k,f)=>{if(events[k]===f)delete events[k]}};
  const document={querySelectorAll:()=>[v],querySelector:()=>v,
    addEventListener:(k,f)=>docEvents[k]=f,removeEventListener:(k,f)=>{if(docEvents[k]===f)delete docEvents[k]}};
  v.ownerDocument=document;
  const window={nusVideo:s=>reports.push(JSON.parse(s))};window.top=window;
  const context={window,document,Date:{now:()=>now},getComputedStyle:()=>({}),innerWidth:640,innerHeight:360,scrollX:0,scrollY:0,
    addEventListener(){},setInterval(){},setTimeout(fn,delay){timers.set(++id,{fn,at:now+delay});return id},clearTimeout:id=>timers.delete(id)};
  vm.createContext(context);vm.runInContext(source,context);
  if(service){
    const p={isPaused:()=>paused,play:()=>v.play(),pause:()=>v.pause(),getCurrentTime:()=>time*1000,getDuration:()=>1000000,
      seek:t=>{if(failSeek)throw new Error('seek failed');seeks.push(t/1000);paused=true}};
    const videoPlayer={getAllPlayerSessionIds:()=>['watch-fixture'],getVideoPlayerBySessionId:()=>p};
    window.netflix={appContext:{state:{playerApp:{getAPI:()=>({videoPlayer})}}}};
    const suffix=fs.readFileSync('spikes/composite/src/webkit.rs','utf8').match(/r#"(\(\(\)=>\{const n=window\.__nus;[\s\S]*?)const ask=/)[1];
    vm.runInContext(suffix+'})();',context);
  }
  function advance(ms){const end=now+ms;for(;;){const next=[...timers].filter(([,t])=>t.at<=end).sort((a,b)=>a[1].at-b[1].at)[0];if(!next)break;now=next[1].at;timers.delete(next[0]);next[1].fn();}now=end;}
  return {n:window.__nus,v,seeks,reports,advance,paused:()=>paused,plays:()=>playCalls,
    fail(){failSeek=true;},settle(){time=seeks.at(-1);events.seeked?.();},input(){docEvents.pointerdown?.({isTrusted:true})}};
}
for(const service of [false,true]) {
  const f=fixture(service);
  for(let i=0;i<20;i++)f.n.skip(10);
  assert.equal(f.seeks.length,0,'burst must not flood the player');f.advance(100);
  assert.deepEqual(f.seeks,[230]);assert.equal(f.paused(),false,'seek-induced pause restored');
  // The player deliberately still reports 30. Accumulate from 230 instead.
  f.n.skip(10);f.n.skip(-10);f.n.skip(-10);f.advance(100);
  assert.deepEqual(f.seeks,[230,220]);assert.equal(f.paused(),false);
  f.settle();f.n.toggle();assert.equal(f.paused(),true);
  const plays=f.plays();for(let i=0;i<12;i++)f.n.skip(10);f.advance(100);f.settle();
  assert.equal(f.paused(),true);assert.equal(f.plays(),plays,'paused skipping never starts playback');
  f.n.toggle();f.n.skip(10);f.n.toggle();f.advance(200);assert.equal(f.paused(),true,'explicit pause wins');
  const before=f.seeks.length;f.n.skip(10);f.v.currentSrc='movie-b';f.advance(100);
  assert.equal(f.seeks.length,before,'source replacement invalidates queued seek');
  f.n.skip(10);f.input();f.advance(100);assert.equal(f.seeks.length,before,'page interaction wins');
  f.n.skip(10);f.n.seekTo(.5);f.advance(200);assert.equal(f.seeks.at(-1),500,'scrub supersedes queued skip');
  const broken=fixture(service);broken.fail();broken.n.skip(10);broken.advance(100);
  assert.equal(broken.reports.at(-1).controlError,'seek-failed');assert.equal(broken.plays(),0);
  const g=fixture(service);for(let i=0;i<30;i++){g.n.skip(10);g.advance(30);}g.advance(100);
  assert.equal(g.seeks.at(-1),330);assert.ok(g.seeks.length<=4,'sustained repeat bounded to four seeks per second');
  console.log(`PASS ${service?'WebKit service adapter':'HTML media'}: bursts, stale playhead, playing/paused, cancellation, source change, sustained repeat`);
}

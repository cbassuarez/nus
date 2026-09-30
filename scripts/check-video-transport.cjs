// Real injected transport against deterministic media elements, including pages
// that hide controls and request PiP suppression. No network or media service.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('spikes/composite/assets/video.js','utf8');
let videos=[],reports=[],listeners={};
const context={window:{nusVideo:s=>reports.push(JSON.parse(s))},document:{querySelector:()=>videos[0]||null,querySelectorAll:()=>videos,addEventListener:(event,fn)=>listeners[event]=fn},getComputedStyle:v=>({objectFit:'contain',objectPosition:'50% 50%',...v.style}),innerWidth:960,innerHeight:540,scrollX:0,scrollY:0,addEventListener:(event,fn)=>listeners[event]=fn,setInterval:()=>{}};
context.window.top=context.window;
vm.runInNewContext(source,context);
assert.equal(reports.length,1,'report immediately, without waiting for the polling interval');
function video(extra={}) {return Object.defineProperties({getBoundingClientRect:()=>({width:960,height:540,left:0,top:0}),readyState:4,isConnected:true,controls:false,disablePictureInPicture:true,controlsList:'nodownload noplaybackrate',currentTime:30,duration:120,seekable:{length:0},currentSrc:'https://example.test/movie.mp4',tagName:'VIDEO',paused:true,muted:false,volume:1,ended:false,play(){this.paused=false;return Promise.resolve();},pause(){this.paused=true;},scrollIntoView(){}},Object.getOwnPropertyDescriptors(extra));}
const transport=context.window.__nus;
for(let i=0;i<100;i++)transport.report();
assert.equal(reports.length,1,'unchanged reports must not cross IPC repeatedly');
assert.equal(reports[0].sleepSafe,true);
context.scrollY=80;transport.report();assert.equal(reports.length,2);assert.equal(reports.at(-1).scrollY,80);
listeners.input();assert.equal(reports.at(-1).sleepSafe,false,'edits stay protected even after a control disappears');
const queryAll=context.document.querySelectorAll;
context.document.querySelectorAll=()=>{throw new Error('repeated input must not scan the document again');};
for(let i=0;i<100;i++)listeners.input();
context.document.querySelectorAll=queryAll;

let v=video();videos=[v];transport.seek(17);assert.equal(v.currentTime,47);transport.seek(-17);assert.equal(v.currentTime,30);
transport.seek(-120);assert.equal(v.currentTime,0);transport.seek(999);assert.equal(v.currentTime,120);
v.duration=Infinity;v.currentTime=40;transport.seek(15);assert.equal(v.currentTime,55);assert.equal(reports.at(-1).v.dur,0);
v.duration=NaN;transport.seek(-7);assert.equal(v.currentTime,48);
v.seekable={length:2,start:i=>[100,150][i],end:i=>[130,200][i]};v.currentTime=125;transport.seek(10);assert.equal(v.currentTime,150);transport.seek(-10);assert.equal(v.currentTime,130);transport.seek(-200);assert.equal(v.currentTime,100);transport.seek(999);assert.equal(v.currentTime,200);
v.isConnected=false;v=video({currentTime:3});videos=[v];transport.seek(9);assert.equal(v.currentTime,12,'replace a removed video without waiting 100 ms');
const before=v.currentTime;transport.seek(NaN);assert.equal(v.currentTime,before);
v=video({get currentTime(){return 10},set currentTime(_){throw new Error('metadata changing')}});videos=[v];listeners.emptied();assert.doesNotThrow(()=>transport.seek(10));
v.currentSrc='toString';transport.report();assert.equal(reports.at(-1).media.length,1,'object prototype names are valid media sources');
console.log('PASS transport: immediate discovery, hidden controls, configurable seeks, finite/unknown/DVR ranges, source replacement, metadata races');
v=video({videoWidth:1080,videoHeight:1920});videos=[v];transport.report();
let report=reports.at(-1).v;
assert.equal(report.videoWidth/report.videoHeight,9/16,'stream dimensions are independent of the page box');
assert.equal(report.w/report.h,9/16,'exclude pillarboxing from the sampled texture');
assert.equal(report.x,(960-540*9/16)/2);
v.videoWidth=1440;v.videoHeight=1080;listeners.resize();
report=reports.at(-1).v;assert.equal(report.w/report.h,4/3,'stream resize reports immediately');
v.style={objectPosition:'0% 100%'};transport.report();assert.equal(reports.at(-1).v.x,0);
v.style={objectPosition:'calc(100% - 10px) 50%'};transport.report();assert.equal(reports.at(-1).v.x,230);
v.style={objectFit:'cover'};transport.report();assert.equal(reports.at(-1).v.w,960);assert.equal(reports.at(-1).v.h,540);
report=reports.at(-1).v;assert.equal(report.dw,1);assert.equal(report.dh,0.75);assert.equal(report.dy,0.125,'cover crop retains its position in the full stream without stretching');
v.style={objectFit:'contain',paddingLeft:'10px',paddingRight:'10px',borderTopWidth:'2px',borderBottomWidth:'2px'};transport.report();
report=reports.at(-1).v;assert.equal(report.h,536);assert.equal(report.y,2);assert.ok(Math.abs(report.w/report.h-4/3)<1e-12);
v.videoWidth=0;v.videoHeight=0;transport.report();assert.equal(reports.at(-1).v.w,940,'metadata gaps keep finite CSS bounds');
console.log('PASS picture geometry: intrinsic ratio, portrait pillarboxing, source resize, object position, cover, padding, missing metadata');
v=video();videos=[v];transport.report();assert.equal(reports.at(-1).drm,false,'a clear video is not protected');
const keyed=video({mediaKeys:{},readyState:0,getBoundingClientRect:()=>({width:40,height:30,left:0,top:0})});videos=[v,keyed];transport.report();
assert.equal(reports.at(-1).v.w,960,'the keyed video is too small to be the picked one');
assert.equal(reports.at(-1).drm,true,'MediaKeys on any video marks the page, not just the picked one');
keyed.mediaKeys=null;transport.report();assert.equal(reports.at(-1).drm,false,'each scan reports what it sees; nus holds the flag for the document');
console.log('PASS protected video: MediaKeys on any video, picked or not');

v=video({duration:200,seekable:{length:2,start:i=>[100,150][i],end:i=>[130,200][i]}});videos=[v];transport.report();
transport.seekTo(.7);assert.equal(v.currentTime,150,'absolute seeks respect DVR gaps too');
transport.step(-9000);assert.equal(v.currentTime,100);assert.equal(v.paused,true);
transport.mute();assert.equal(v.muted,true);assert.equal(reports.at(-1).v.muted,true,'mute reports immediately');
transport.vol(-.3);assert.ok(Math.abs(v.volume-.7)<1e-12);transport.vol(Infinity);assert.ok(Math.abs(v.volume-.7)<1e-12);
const active=video({paused:false,getBoundingClientRect:()=>({left:0,top:0,width:320,height:180})});
videos=[v,active];transport.report();assert.equal(transport.selectedVideo(),active,'playing video wins over a larger paused decoration');
active.style={visibility:'hidden'};transport.report();assert.equal(transport.selectedVideo(),v,'hidden videos do not steal the player');
console.log('PASS controls: absolute DVR gap seeks, bounded frame steps, immediate mute, invalid volume, visible active selection');

// The main tracker also serves WebKit, where injection is main-frame only.
// It must reach same-origin child media and report top-page crop coordinates.
const top=context.window;top.innerWidth=960;top.innerHeight=540;
const child={top,parent:top}, grandchild={top,parent:child};
const inner={tagName:'IFRAME',offsetWidth:300,offsetHeight:200,clientLeft:0,clientTop:0,clientWidth:300,clientHeight:200,
  getBoundingClientRect:()=>({left:30,top:20,width:300,height:200})};
const outer={tagName:'IFRAME',offsetWidth:640,offsetHeight:400,clientLeft:2,clientTop:2,clientWidth:636,clientHeight:396,
  getBoundingClientRect:()=>({left:100,top:50,width:640,height:400})};
child.frameElement=outer;grandchild.frameElement=inner;
const nested=video({ownerDocument:{defaultView:grandchild},videoWidth:200,videoHeight:100,
  getBoundingClientRect:()=>({left:10,top:5,width:200,height:100})});
inner.contentDocument={querySelectorAll:()=>[nested]};outer.contentDocument={querySelectorAll:()=>[inner]};
videos=[outer];transport.report();report=reports.at(-1).v;
assert.equal(transport.selectedVideo(),nested);assert.deepEqual([report.x,report.y,report.w,report.h,report.vw,report.vh],[142,77,200,100,960,540]);
transport.seek(7);assert.equal(nested.currentTime,37,'main-world command reaches selected nested video');
outer.getBoundingClientRect=()=>({left:100,top:50,width:1280,height:800});transport.report();report=reports.at(-1).v;
assert.deepEqual([report.x,report.y,report.w,report.h],[184,104,400,200],'frame borders and CSS scaling compose');
videos=[video()];assert.notEqual(transport.selectedVideo(),nested,'detached iframe cannot keep a still-connected child document selected');
const inaccessible={tagName:'IFRAME',get contentDocument(){throw new Error('cross origin');}};
videos=[inaccessible];assert.doesNotThrow(()=>transport.report());assert.equal(reports.at(-1).v,null);
v=video({ownerDocument:{defaultView:{top,parent:top,frameElement:null}},paused:false,mediaKeys:{}});videos=[v];transport.report();
assert.equal(reports.at(-1).v,null,'unknown frame offset never crops unrelated top-page pixels');
assert.equal(reports.at(-1).playing,true);assert.equal(reports.at(-1).drm,true,'unmapped crop still reports protection and playback');
console.log('PASS nested frames: same-origin discovery, command target, composed crop geometry, detach and cross-origin boundary');

// Automatic PiP eligibility excludes silent preview rollers; manual PiP still
// uses the same selected video and transport.
v=video({paused:false,muted:true});videos=[v];transport.report();
assert.equal(reports.at(-1).v.audible,false);
v.muted=false;v.volume=0;transport.report();assert.equal(reports.at(-1).v.audible,false);
v.volume=1;transport.report();assert.equal(reports.at(-1).v.audible,true);
v.webkitAudioDecodedByteCount=0;transport.report();assert.equal(reports.at(-1).v.audible,false);
v.webkitAudioDecodedByteCount=1024;transport.report();assert.equal(reports.at(-1).v.audible,true);
console.log('PASS automatic PiP excludes muted, zero-volume and known audio-free previews');

(async()=>{
  v=video({play(){return Promise.reject(Object.assign(new Error('denied'),{name:'NotAllowedError'}));}});videos=[v];transport.report();
  transport.toggle();await Promise.resolve();assert.equal(reports.at(-1).controlError,'play-blocked','play rejection reaches native UI');
  v.play=function(){this.paused=false;return Promise.resolve();};transport.toggle();await Promise.resolve();
  assert.equal(reports.at(-1).controlError,null);assert.equal(reports.at(-1).v.paused,false);
  let reject;
  v.paused=true;v.play=function(){this.paused=false;return new Promise((_,no)=>{reject=no;});};transport.toggle();
  transport.toggle();reject(Object.assign(new Error('interrupted'),{name:'AbortError'}));await Promise.resolve();
  assert.equal(v.paused,true);assert.equal(reports.at(-1).controlError,null,'old play rejection cannot overwrite a newer pause');
  v.play=()=>{throw new Error('player gone');};transport.toggle();assert.equal(reports.at(-1).controlError,'play-failed');
  v=video({get currentTime(){return 10},set currentTime(_){throw new Error('metadata changing')}});videos=[v];transport.report();
  assert.doesNotThrow(()=>transport.seekTo(.5));assert.equal(reports.at(-1).controlError,'seek-failed');
  assert.doesNotThrow(()=>transport.step(1));assert.equal(reports.at(-1).controlError,'seek-failed');
  console.log('PASS failure reporting: blocked/synchronous play, recovery, superseded promises, seek/step metadata races');

  const diagnosticReports=[],configurations=[{secretLicenseHint:'never serialize me'}];let received,resolveAccess,rejectAccess;
  let access=new Promise((yes,no)=>{resolveAccess=yes;rejectAccess=no;});
  const navigator={requestMediaKeySystemAccess:function(){received={owner:this,args:[...arguments]};return access;}};
  const failure=video({readyState:0,networkState:3,error:{code:4,message:'secret service message'}});
  const diagnosticContext={...context,navigator,window:{__nusMediaDiagnostics:true,nusVideo:s=>diagnosticReports.push(JSON.parse(s))},
    document:{querySelector:()=>failure,querySelectorAll:()=>[failure],addEventListener(){}}};
  diagnosticContext.window.top=diagnosticContext.window;
  vm.runInNewContext(source,diagnosticContext);
  assert.equal(diagnosticReports.at(-1).v,null);assert.equal(diagnosticReports.at(-1).diagnostic.mediaError,4,'pre-metadata playback failures remain diagnosable');
  assert.equal(diagnosticReports.at(-1).diagnostic.networkState,3);
  const receiver={};
  const returned=navigator.requestMediaKeySystemAccess.call(receiver,'com.apple.fps',configurations);
  assert.equal(returned,access,'instrumentation returns the original Promise');
  assert.equal(received.owner,receiver);assert.equal(received.args[1],configurations,'original arguments and receiver are preserved');
  resolveAccess({});await Promise.resolve();
  assert.equal(diagnosticReports.at(-1).diagnostic.eme[0].status,'granted');
  access=new Promise((yes,no)=>{resolveAccess=yes;rejectAccess=no;});
  navigator.requestMediaKeySystemAccess('com.widevine.alpha',configurations);
  rejectAccess(Object.assign(new Error('secret license failure'),{name:'NotSupportedError'}));await Promise.resolve();
  const diagnostic=diagnosticReports.at(-1).diagnostic;
  assert.equal(diagnostic.eme[1].status,'denied');assert.equal(diagnostic.eme[1].error,'NotSupportedError');
  assert.ok(!JSON.stringify(diagnosticReports).includes('secret'),'configuration and error messages are not reported');
  const untouched=()=>Promise.resolve({}), ordinaryContext={...diagnosticContext,navigator:{requestMediaKeySystemAccess:untouched},window:{nusVideo(){}}};
  ordinaryContext.window.top=ordinaryContext.window;vm.runInNewContext(source,ordinaryContext);
  assert.equal(ordinaryContext.navigator.requestMediaKeySystemAccess,untouched,'normal pages preserve the EME method identity');
  console.log('PASS diagnostics: pre-metadata errors, original EME Promise/receiver/arguments, grant/rejection without license details');
})().catch(error=>{console.error(error);process.exitCode=1;});

// Execute the actual WebKit-only suffix. This tests DOM isolation and the
// service adapter without a browser, network, account, or protected media.
{
  const nativeSource=fs.readFileSync('spikes/composite/src/webkit.rs','utf8').match(/r#"(\(\(\)=>\{const n=window\.__nus;[\s\S]*?\}\)\(\);)"#/)[1];
  class Element {
    constructor(doc,parent=null){this.ownerDocument=doc;this.parentElement=parent;this.isConnected=true;this.style={position:'relative',width:'73%'};const classes=new Set();this.classList={contains:n=>classes.has(n),add:n=>classes.add(n),remove:n=>classes.delete(n)};}
    appendChild(node){this.ownerDocument.styles.push(node);}
  }
  function doc(frame=null){const d={styles:[],defaultView:{frameElement:frame},addEventListener(){},createElement(){return new Element(d)},getElementById(id){return d.styles.find(s=>s.id===id)||null}};d.documentElement=new Element(d);d.head=new Element(d,d.documentElement);d.body=new Element(d,d.documentElement);return d;}
  class NativeVideo extends Element {}
  const topDoc=doc(), wrapper=new Element(topDoc,topDoc.body), first=new NativeVideo(topDoc,wrapper), second=new NativeVideo(topDoc,wrapper);
  wrapper.classList.add('__nus-pip-branch');
  let selected=second, reports=0, delegated=[],fallbacks=[],failures=[];
  const native={selectedVideo:()=>selected,report:()=>reports++,cancelSkip(){},command(action,failure){delegated.push(failure);try{return action(selected)}catch(_){failures.push(failure)}}};
  for(const name of ['toggle','seek','seekTo','step']) native[name]=(...args)=>fallbacks.push([name,...args]);
  const window={__nus:native};
  vm.runInNewContext(nativeSource,{window,document:topDoc,HTMLVideoElement:NativeVideo,DOMException});
  window.__nusPip(true);
  assert.equal(first.classList.contains('__nus-pip-video'),false,'never isolate an arbitrary first video');
  assert.equal(second.classList.contains('__nus-pip-video'),true);
  assert.equal(topDoc.documentElement.classList.contains('__nus-pip'),true);
  assert.deepEqual(second.style,{position:'relative',width:'73%'},'inline site styles are untouched');
  assert.equal(topDoc.styles.length,1);
  window.__nusPip(true);assert.equal(topDoc.styles.length,1,'polling does not accumulate styles');
  second.classList.remove('__nus-pip-video');window.__nusPip(true);
  assert.equal(second.classList.contains('__nus-pip-video'),true,'page className updates cannot permanently hide the PiP picture');
  selected=first;window.__nusPip(true);
  assert.equal(second.classList.contains('__nus-pip-video'),false,'a replacement selection releases the old player');
  assert.equal(first.classList.contains('__nus-pip-video'),true);
  window.__nusPip(false);
  assert.equal(topDoc.documentElement.classList.contains('__nus-pip'),false);
  assert.equal(first.classList.contains('__nus-pip-video'),false);
  assert.equal(wrapper.classList.contains('__nus-pip-branch'),true,'only our added classes are removed');
  assert.ok(topDoc.styles[0].textContent.includes('object-fit:contain!important'));

  const frame=new Element(topDoc,wrapper), childDoc=doc(frame), childWrapper=new Element(childDoc,childDoc.body), nestedVideo=new NativeVideo(childDoc,childWrapper);
  selected=nestedVideo;window.__nusPip(true);
  assert.equal(childDoc.documentElement.classList.contains('__nus-pip'),true);
  assert.equal(topDoc.documentElement.classList.contains('__nus-pip'),true);
  assert.equal(frame.classList.contains('__nus-pip-frame'),true,'same-origin frame viewport is also expanded');
  assert.equal(nestedVideo.classList.contains('__nus-pip-video'),true);
  window.__nusPip(false);
  assert.equal(frame.classList.contains('__nus-pip-frame'),false);
  assert.equal(childDoc.documentElement.classList.contains('__nus-pip'),false);
  assert.equal(nestedVideo.classList.contains('__nus-pip-video'),false);
  selected=null;assert.doesNotThrow(()=>window.__nusPip(true));

  selected=first;
  native.seek(3);assert.deepEqual(fallbacks.at(-1),['seek',3],'non-Netflix transport uses the common implementation');
  let paused=true,time=30000;
  const player={isPaused:()=>paused,play:()=>{paused=false},pause:()=>{paused=true},getCurrentTime:()=>time,getDuration:()=>70000,seek:t=>{time=t}};
  const videoPlayer={getAllPlayerSessionIds:()=>['preview','watch-player'],getVideoPlayerBySessionId:id=>id==='watch-player'?player:null};
  window.netflix={appContext:{state:{playerApp:{getAPI:()=>({videoPlayer})}}}};
  native.toggle();assert.equal(paused,false);assert.equal(delegated.at(-1),'play-failed');
  native.seek(17);assert.equal(time,47000);native.seek(-100);assert.equal(time,0);
  native.seekTo(2);assert.equal(time,70000);native.step(-30);assert.equal(time,69000);assert.equal(paused,true);
  native.seek(NaN);assert.equal(time,69000);
  player.seek=()=>{throw new Error('session changed')};assert.doesNotThrow(()=>native.seek(1));assert.equal(failures.at(-1),'seek-failed');
  console.log('PASS WebKit adapter: selected-video isolation, restoration, nested frames, Netflix native time units, clamping and failure delegation');
}

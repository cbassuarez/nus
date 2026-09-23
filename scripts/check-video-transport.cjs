// Real injected transport against deterministic media elements, including pages
// that hide controls and request PiP suppression. No network or media service.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('spikes/composite/assets/video.js','utf8');
let videos=[],reports=[],listeners={};
const context={window:{nusVideo:s=>reports.push(JSON.parse(s))},document:{querySelector:()=>videos[0]||null,querySelectorAll:()=>videos,addEventListener:(event,fn)=>listeners[event]=fn},getComputedStyle:v=>({objectFit:'contain',objectPosition:'50% 50%',...v.style}),innerWidth:960,innerHeight:540,scrollX:0,scrollY:0,addEventListener:(event,fn)=>listeners[event]=fn,setInterval:()=>{}};
context.window.top=context.window;
vm.runInNewContext(source,context);
assert.equal(reports.length,1,'report immediately, without waiting for the polling interval');
function video(extra={}) {return {getBoundingClientRect:()=>({width:960,height:540,left:0,top:0}),readyState:4,isConnected:true,controls:false,disablePictureInPicture:true,controlsList:'nodownload noplaybackrate',currentTime:30,duration:120,seekable:{length:0},currentSrc:'https://example.test/movie.mp4',tagName:'VIDEO',paused:true,muted:false,ended:false,scrollIntoView(){},...extra};}
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

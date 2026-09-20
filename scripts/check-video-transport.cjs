// Real injected transport against deterministic media elements, including pages
// that hide controls and request PiP suppression. No network or media service.
const fs=require('node:fs'),vm=require('node:vm'),assert=require('node:assert/strict');
const source=fs.readFileSync('spikes/composite/assets/video.js','utf8');
let videos=[],reports=[],listeners={};
const context={window:{nusVideo:s=>reports.push(JSON.parse(s))},document:{querySelectorAll:()=>videos,addEventListener:(event,fn)=>listeners[event]=fn},innerWidth:960,innerHeight:540,setInterval:()=>{}};
vm.runInNewContext(source,context);
assert.equal(reports.length,1,'report immediately, without waiting for the polling interval');
function video(extra={}) {return {getBoundingClientRect:()=>({width:960,height:540,left:0,top:0}),readyState:4,isConnected:true,controls:false,disablePictureInPicture:true,controlsList:'nodownload noplaybackrate',currentTime:30,duration:120,seekable:{length:0},currentSrc:'https://example.test/movie.mp4',tagName:'VIDEO',paused:true,muted:false,ended:false,scrollIntoView(){},...extra};}
const transport=context.window.__nus;
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

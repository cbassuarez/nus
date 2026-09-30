(() => {
  if (window.__nus) return;
  const st = { best: null, error: null, command: 0, eme: new Map() };
  let edited = false, lastReport = "";
  // Editing is sticky for this document. Re-scanning media on every input
  // adds page-sized work to typing and bulk form updates for no state change.
  addEventListener("input", () => {
    if (edited) return;
    edited = true; report();
  }, { capture: true, once: true });
  function mediaElements() {
    const media=[], docs=[document], seen=new Set();
    while (docs.length && seen.size < 256) {
      const doc=docs.pop(); if (seen.has(doc)) continue; seen.add(doc);
      for (const el of doc.querySelectorAll('video,audio,iframe')) {
        if (el.tagName === 'IFRAME') {
          // Only browser-permitted same-origin access. No page messages or
          // cross-origin content are trusted as transport commands.
          try {if (el.contentDocument) docs.push(el.contentDocument);} catch (_) {}
        } else media.push(el);
      }
    }
    return media;
  }
  function pick(media=mediaElements()) {
    let best = null, area = 0, playing = false;
    for (const v of media) {
      if (v.tagName !== 'VIDEO') continue;
      const r = v.getBoundingClientRect();
      const a = r.width * r.height, active = !v.paused && !v.ended, style = getComputedStyle(v);
      if (v.readyState <= 0 || r.width <= 80 || r.height <= 0 || style.visibility === 'hidden' || style.display === 'none') continue;
      if (!best || (active && !playing) || (active === playing && (a > area || (a === area && v === st.best)))) {
        area = a; best = v; playing = active;
      }
    }
    return best;
  }
  function report() {
    const elements=mediaElements(), v = pick(elements); st.best = v;
    let p = null;
    if (v) {
      const r = topBounds(pictureBounds(v),v.ownerDocument?.defaultView || window);
      if (r) p = { x: r.x, y: r.y, w: r.w, h: r.h, vw: r.vw, vh: r.vh,
            dx: r.dx || 0, dy: r.dy || 0, dw: r.dw ?? 1, dh: r.dh ?? 1,
            videoWidth: v.videoWidth, videoHeight: v.videoHeight,
            paused: v.paused, ended: v.ended, muted: v.muted, audible: !v.muted && v.volume > 0 && (v.webkitAudioDecodedByteCount === undefined || v.webkitAudioDecodedByteCount > 0 || v.audioTracks?.length > 0), t: v.currentTime, dur: Number.isFinite(v.duration) ? v.duration : 0 };
    }
    const media = [], seen = new Set();
    let playing = false;
    for (const m of elements) {
      if (!m.paused && !m.ended) playing = true;
      const src = m.currentSrc || m.src || '';
      if (!src || seen.has(src)) continue;
      seen.add(src);
      media.push({ k: m.tagName.toLowerCase(), src, w: m.videoWidth || 0, h: m.videoHeight || 0, blob: /^(blob:|mediasource:)/.test(src) });
    }
    // A video with MediaKeys can be showing decrypted frames (EME); nus
    // keeps those pixels off disk.
    let drm = false;
    for (const m of elements) if (m.tagName === 'VIDEO' && m.mediaKeys) { drm = true; break; }
    // A failed element can have no metadata and therefore no PiP candidate.
    const diagnosticVideo=v || elements.find(m=>m.tagName==='VIDEO' && m.error) || elements.find(m=>m.tagName==='VIDEO');
    const diagnostic=diagnosticVideo || st.eme.size ? {
      mediaError:diagnosticVideo?.error?.code || 0, readyState:diagnosticVideo?.readyState ?? 0,
      networkState:diagnosticVideo?.networkState ?? 0, eme:[...st.eme.values()].map(({keySystem,status,error})=>({keySystem,status,error}))
    } : null;
    const payload = JSON.stringify({ v: p, media, playing, drm, controlError: st.error, diagnostic, top: window === window.top, scrollX, scrollY,
      sleepSafe: !edited && !document.querySelector("input,textarea,select,[contenteditable],video,audio,iframe") });
    if (window.nusVideo && payload !== lastReport) {
      window.nusVideo(payload); lastReport = payload;
    }
  }
  // CEF samples the top page's texture. Frame-local coordinates would crop
  // unrelated pixels; only expose a crop when the entire ancestor path can
  // be mapped. Cross-origin frames still report playback/media/DRM state.
  function topBounds(box, w) {
    const r = {dx:0,dy:0,dw:1,dh:1,...box};
    try {
      while (w !== w.top) {
        const f = w.frameElement; if (!f) return null;
        const b = f.getBoundingClientRect(), sx = b.width / f.offsetWidth, sy = b.height / f.offsetHeight;
        if (!(sx > 0 && sy > 0)) return null;
        const left = b.left + f.clientLeft*sx, top = b.top + f.clientTop*sy;
        const x = left+r.x*sx, y = top+r.y*sy, width = r.w*sx, height = r.h*sy;
        const cx = Math.max(left,x), cy = Math.max(top,y);
        const cw = Math.max(0,Math.min(left+f.clientWidth*sx,x+width)-cx);
        const ch = Math.max(0,Math.min(top+f.clientHeight*sy,y+height)-cy);
        if (!(width > 0 && height > 0 && cw > 0 && ch > 0)) return null;
        r.dx += (cx-x)/width*r.dw; r.dy += (cy-y)/height*r.dh;
        r.dw *= cw/width; r.dh *= ch/height;
        r.x=cx; r.y=cy; r.w=cw; r.h=ch;
        w=w.parent;
      }
      return {...r,vw:w.innerWidth || innerWidth,vh:w.innerHeight || innerHeight};
    } catch (_) { return null; }
  }
  // Sample the picture, excluding CSS borders, padding and object-fit bars.
  // The intrinsic dimensions remain separate: a page's player box is not
  // necessarily the same shape as the stream (portrait videos in particular).
  function pictureBounds(v) {
    const r = v.getBoundingClientRect(), s = getComputedStyle(v);
    const sx = v.offsetWidth ? r.width / v.offsetWidth : 1;
    const sy = v.offsetHeight ? r.height / v.offsetHeight : 1;
    const n = key => parseFloat(s[key]) || 0;
    const left = (n('borderLeftWidth') + n('paddingLeft')) * sx;
    const top = (n('borderTopWidth') + n('paddingTop')) * sy;
    const box = {x:r.left+left, y:r.top+top,
      w:r.width-left-(n('borderRightWidth')+n('paddingRight'))*sx,
      h:r.height-top-(n('borderBottomWidth')+n('paddingBottom'))*sy};
    if (!(v.videoWidth > 0 && v.videoHeight > 0 && box.w > 0 && box.h > 0)) return box;
    const iw=v.videoWidth*sx, ih=v.videoHeight*sy;
    let scale;
    switch (s.objectFit) {
      case 'contain': scale=Math.min(box.w/iw,box.h/ih); break;
      case 'scale-down': scale=Math.min(1,box.w/iw,box.h/ih); break;
      case 'none': scale=1; break;
      case 'cover': scale=Math.max(box.w/iw,box.h/ih); break;
      default: return box;
    }
    const [px='50%',py='50%'] = (s.objectPosition || '50% 50%').match(/calc\([^)]*\)|\S+/g);
    const offset = (value, free, scale) => {
      const calc=value.match(/^calc\(([-\d.]+)%\s*([+-])\s*([\d.]+)px\)$/);
      const result=calc ? free*Number(calc[1])/100+(calc[2]==='-'?-1:1)*Number(calc[3])*scale
        : value.endsWith('%') ? free*parseFloat(value)/100 : value.endsWith('px') ? parseFloat(value)*scale : free/2;
      return Number.isFinite(result) ? result : free/2;
    };
    const w=iw*scale,h=ih*scale;
    const x=box.x+offset(px,box.w-w,sx),y=box.y+offset(py,box.h-h,sy);
    // Cropped object-fit modes cannot reveal pixels outside the page texture.
    const cx=Math.max(box.x,x),cy=Math.max(box.y,y);
    const cw=Math.max(0,Math.min(box.x+box.w,x+w)-cx),ch=Math.max(0,Math.min(box.y+box.h,y+h)-cy);
    return {x:cx,y:cy,w:cw,h:ch,dx:(cx-x)/w,dy:(cy-y)/h,dw:cw/w,dh:ch/h};
  }
  const V = () => {
    const elements=mediaElements();
    return st.best?.isConnected && st.best.readyState > 0 && elements.includes(st.best) ? st.best : (st.best=pick(elements));
  };
  function command(action, failure) {
    const v=V(), id=++st.command; st.error=null;
    if (!v) { st.error='unavailable'; report(); return; }
    const failed = e => {
      // A pause or newer command can reject an older pending play request.
      if (id !== st.command || !v.isConnected) return;
      st.error=failure === 'play-failed' && e?.name === 'NotAllowedError' ? 'play-blocked' : failure;
      report();
    };
    try {
      const result=action(v);
      if (result?.then) result.then(() => {if(id === st.command) report();},failed);
    } catch (e) { failed(e); }
    report();
  }
  // Browser-owned transport ignores page control visibility and PiP hints.
  // Clamp to the seekable range for DVR streams, including gaps in a range.
  function clampSeek(v, target, direction) {
    const ranges=v.seekable;
    if (ranges?.length) {
      target=Math.max(ranges.start(0),Math.min(ranges.end(ranges.length-1),target));
      for(let i=0;i<ranges.length-1;i++) {
        if(target>ranges.end(i) && target<ranges.start(i+1)) {
          target=direction<0 ? ranges.end(i) : ranges.start(i+1); break;
        }
      }
    } else {
      target=Math.max(0,Number.isFinite(v.duration) ? Math.min(v.duration,target) : target);
    }
    return target;
  }
  function seekTarget(v,target,direction) { v.currentTime=clampSeek(v,target,direction); }
  // PiP skip bursts carry their intended destination while the player's
  // reported clock catches up. Never flood a buffering player with clicks.
  let skipQueue=null;
  function cancelSkip() {
    const q=skipQueue; skipQueue=null;
    if (!q) return;
    clearTimeout(q.timer); clearTimeout(q.expiry);
    q.v.removeEventListener?.('seeked',q.settled);
    q.doc?.removeEventListener?.('pointerdown',q.interrupted,true);
    q.doc?.removeEventListener?.('keydown',q.interrupted,true);
  }
  function skip(delta) {
    if (!Number.isFinite(delta) || delta===0) return;
    const v=V(); if (!v) { cancelSkip(); return; }
    const driver=window.__nus.skipPlayer?.(v) || {
      key:v, time:()=>v.currentTime, paused:()=>v.paused,
      clamp:(t,d)=>clampSeek(v,t,d), seek:t=>{v.currentTime=t;}, play:()=>v.play(), valid:()=>true,
    };
    const src=v.currentSrc || v.src || '', now=Date.now();
    let q=skipQueue;
    if (!q || q.v!==v || q.driver.key!==driver.key || q.src!==src) {
      cancelSkip();
      q={v,src,driver,target:Number.isFinite(driver.time())?driver.time():0,
         resume:!driver.paused()&&!v.ended,first:now,timer:null,expiry:null,doc:v.ownerDocument || document};
      skipQueue=q;
      q.valid=()=>skipQueue===q && v.isConnected && V()===v && (v.currentSrc || v.src || '')===src && driver.valid();
      q.restore=()=>{
        if (q.valid() && q.resume && !q.restored && !v.ended && driver.paused()) {
          q.restored=true;
          command(()=>driver.play(),'play-failed');
        }
      };
      q.settled=()=>{if(skipQueue!==q)return;if (!q.valid()) {cancelSkip();return;} q.restore(); if(!q.timer)cancelSkip();};
      // A real page interaction or an explicit nus transport command wins
      // over any delayed resume from a previous skip.
      q.interrupted=e=>{if(e.isTrusted)cancelSkip();};
      v.addEventListener?.('seeked',q.settled);
      q.doc?.addEventListener?.('pointerdown',q.interrupted,true);
      q.doc?.addEventListener?.('keydown',q.interrupted,true);
    }
    try { q.target=driver.clamp(q.target+delta,delta); }
    catch (_) { cancelSkip(); command(()=>{throw new Error('seek target unavailable');},'seek-failed'); return; }
    clearTimeout(q.timer);
    q.timer=setTimeout(()=>{
      q.timer=null;
      if(!q.valid()){cancelSkip();return;}
      q.first=Date.now(); q.restored=false;
      clearTimeout(q.expiry);q.expiry=setTimeout(()=>{if(skipQueue===q)cancelSkip();},10000);
      command(()=>{
        try {
          const result=driver.seek(q.target);
          if(result?.then)return result.then(()=>q.restore(),error=>{if(skipQueue===q)cancelSkip();throw error;});
          q.restore();
        } catch(error) { if(skipQueue===q)cancelSkip(); throw error; }
      },'seek-failed');
    },Math.max(0,Math.min(100,250-(now-q.first))));
  }
  function seek(delta) {
    if (!Number.isFinite(delta)) return;
    cancelSkip();
    command(v => seekTarget(v,(Number.isFinite(v.currentTime) ? v.currentTime : 0)+delta,delta),'seek-failed');
  }
  // Observe access requests only: an access grant does not establish that a
  // license or stream will work. Never inspect configurations or license data.
  function watchKeySystemAccess() {
    if (window.__nusMediaDiagnostics !== true || typeof navigator === 'undefined') return;
    const original=navigator.requestMediaKeySystemAccess, then=Promise.prototype.then;
    if (typeof original !== 'function') return;
    const errors=new Set(['NotSupportedError','SecurityError','NotAllowedError','InvalidStateError','TypeError','AbortError','QuotaExceededError']);
    const request=function requestMediaKeySystemAccess(keySystem, supportedConfigurations) {
      const key=typeof keySystem==='string' && /^[A-Za-z0-9.-]{1,80}$/.test(keySystem) ? keySystem : 'unknown';
      const entry={keySystem:key,status:'requested',error:null};
      if (!st.eme.has(key) && st.eme.size>=8) st.eme.delete(st.eme.keys().next().value);
      st.eme.set(key,entry);
      const settle=(status,error)=>{
        if(st.eme.get(key)!==entry)return;
        try {entry.status=status;entry.error=errors.has(error?.name) ? error.name : null;report();} catch(_) {}
      };
      let result;
      try {result=Reflect.apply(original,this,arguments);}
      catch(error){settle('denied',error);throw error;}
      try {then.call(result,()=>settle('granted'),error=>settle('denied',error));} catch(_) {}
      try {report();} catch(_) {}
      return result;
    };
    try {Object.defineProperty(navigator,'requestMediaKeySystemAccess',{value:request,configurable:true,writable:true});} catch(_) {}
  }
  window.__nus = {
    report,
    selectedVideo: V,
    command,
    seek,
    skip,
    cancelSkip,
    seekTo(f) { cancelSkip(); if (Number.isFinite(f)) command(v => {
      if (!(Number.isFinite(v.duration) && v.duration > 0)) return;
      const target=Math.max(0,Math.min(1,f))*v.duration;
      seekTarget(v,target,target-v.currentTime);
    },'seek-failed'); },
    toggle() { cancelSkip(); command(v => v.paused || v.ended ? v.play() : v.pause(),'play-failed'); },
    vol(d) { if (Number.isFinite(d)) command(v => {v.volume=Math.max(0,Math.min(1,v.volume+d));},'volume-failed'); },
    mute() { command(v => {v.muted=!v.muted;},'volume-failed'); },
    step(f) { cancelSkip(); if (Number.isFinite(f)) command(v => {v.pause();seekTarget(v,v.currentTime+f/30,f);},'seek-failed'); },
    reveal() { const v = V(); if (v) v.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' }); report(); },
  };
  watchKeySystemAccess();
  for (const event of ['loadedmetadata','resize','durationchange','play','pause','seeked','emptied','ended','volumechange','error']) document.addEventListener(event,report,true);
  report();
  setInterval(report, 100);
})();

(() => {
  if (window.__nus) return;
  const st = { best: null };
  let edited = false, lastReport = "";
  // Editing is sticky for this document. Re-scanning media on every input
  // adds page-sized work to typing and bulk form updates for no state change.
  addEventListener("input", () => {
    if (edited) return;
    edited = true; report();
  }, { capture: true, once: true });
  function pick() {
    let best = null, area = 0;
    for (const v of document.querySelectorAll('video')) {
      const r = v.getBoundingClientRect();
      const a = r.width * r.height;
      if (a > area && v.readyState > 0 && r.width > 80) { area = a; best = v; }
    }
    return best;
  }
  function report() {
    const v = pick(); st.best = v;
    let p = null;
    if (v) {
      const r = pictureBounds(v);
      p = { x: r.x, y: r.y, w: r.w, h: r.h, vw: innerWidth, vh: innerHeight,
            dx: r.dx || 0, dy: r.dy || 0, dw: r.dw ?? 1, dh: r.dh ?? 1,
            videoWidth: v.videoWidth, videoHeight: v.videoHeight,
            paused: v.paused, ended: v.ended, muted: v.muted, t: v.currentTime, dur: Number.isFinite(v.duration) ? v.duration : 0 };
    }
    const media = [], seen = new Set();
    for (const m of document.querySelectorAll('video,audio')) {
      const src = m.currentSrc || m.src || '';
      if (!src || seen.has(src)) continue;
      seen.add(src);
      media.push({ k: m.tagName.toLowerCase(), src, w: m.videoWidth || 0, h: m.videoHeight || 0, blob: /^(blob:|mediasource:)/.test(src) });
    }
    const payload = JSON.stringify({ v: p, media, top: window === window.top, scrollX, scrollY,
      sleepSafe: !edited && !document.querySelector("input,textarea,select,[contenteditable],video,audio,iframe") });
    if (window.nusVideo && payload !== lastReport) {
      window.nusVideo(payload); lastReport = payload;
    }
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
  const V = () => st.best?.isConnected ? st.best : (st.best = pick());
  // Browser-owned transport ignores page control visibility and PiP hints.
  // Clamp to the seekable range for DVR streams, including gaps in a range.
  function seek(delta) {
    const v=V(); if (!v || !Number.isFinite(delta)) return;
    let target=(Number.isFinite(v.currentTime) ? v.currentTime : 0)+delta;
    const ranges=v.seekable;
    if (ranges?.length) {
      target=Math.max(ranges.start(0),Math.min(ranges.end(ranges.length-1),target));
      for(let i=0;i<ranges.length-1;i++) {
        if(target>ranges.end(i) && target<ranges.start(i+1)) {
          target=delta<0 ? ranges.end(i) : ranges.start(i+1); break;
        }
      }
    } else {
      target=Math.max(0,Number.isFinite(v.duration) ? Math.min(v.duration,target) : target);
    }
    try {v.currentTime=target;} catch (_) { /* Metadata can change during a seek. */ }
    report();
  }
  window.__nus = {
    report,
    seek,
    seekTo(f) { const v = V(); if (v && Number.isFinite(v.duration) && v.duration > 0 && Number.isFinite(f)) { v.currentTime = Math.max(0, Math.min(v.duration, f * v.duration)); report(); } },
    toggle() { const v = V(); if (v) { if (v.paused) v.play()?.catch(() => {}); else v.pause(); } },
    vol(d) { const v = V(); if (v) v.volume = Math.max(0, Math.min(1, v.volume + d)); },
    mute() { const v = V(); if (v) v.muted = !v.muted; },
    step(f) { const v = V(); if (v) { v.pause(); v.currentTime += f / 30; } },
    reveal() { const v = V(); if (v) v.scrollIntoView({ block: 'center', inline: 'center', behavior: 'instant' }); report(); },
  };
  for (const event of ['loadedmetadata','resize','durationchange','play','pause','seeked','emptied']) document.addEventListener(event,report,true);
  report();
  setInterval(report, 100);
})();

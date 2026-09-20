(() => {
  if (window.__nus) return;
  const st = { best: null };
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
      const r = v.getBoundingClientRect();
      p = { x: r.left, y: r.top, w: r.width, h: r.height, vw: innerWidth, vh: innerHeight,
            paused: v.paused, ended: v.ended, muted: v.muted, t: v.currentTime, dur: Number.isFinite(v.duration) ? v.duration : 0 };
    }
    const media = [], seen = new Set();
    for (const m of document.querySelectorAll('video,audio')) {
      const src = m.currentSrc || m.src || '';
      if (!src || seen.has(src)) continue;
      seen.add(src);
      media.push({ k: m.tagName.toLowerCase(), src, w: m.videoWidth || 0, h: m.videoHeight || 0, blob: /^(blob:|mediasource:)/.test(src) });
    }
    if (window.nusVideo) window.nusVideo(JSON.stringify({ v: p, media }));
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
  for (const event of ['loadedmetadata','durationchange','play','pause','seeked','emptied']) document.addEventListener(event,report,true);
  report();
  setInterval(report, 100);
})();

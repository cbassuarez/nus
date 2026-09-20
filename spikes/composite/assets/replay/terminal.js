/* The shell.
 *
 * Not a mockup, not a video, and no longer my own painter: a recorded PTY
 * session decoded by `nus_vt::Term` and drawn by `nus_render::GridRenderer` —
 * the app's own renderer — both compiled to WebAssembly. What arrives here is
 * the renderer's draw list, `nus_render::Instance` quads in push order, plus
 * the glyph bitmaps swash rasterised into its atlas.
 *
 * So the cell metrics, the shaping, the glyph coverage, the colour resolution,
 * the wide characters and the cursor are all the app's. This file blits quads,
 * which is the one part that is exact by construction.
 *
 * wgpu is not involved: the app's pipeline passes screen size as a WGSL
 * immediate, which WebGPU has no equivalent for. Consuming the scene instead of
 * rendering it needs no GPU API and works in every browser.
 */

import init, { Shell } from '../wasm/nus_vt_wasm.js';

/* Broadsheet's palettes, from docs/DESIGN.md in the app repo. */
const THEMES = {
  paper: {
    fg: 0x141414, bg: 0xf4f1ea, cursor: 0x141414,
    ansi: [
      0x141414, 0xb3261e, 0x2e7d32, 0x9a6b00, 0x1f5fbf, 0x8e3b8e, 0x1a7f8a, 0x8a857a,
      0x4a4740, 0xd63a2f, 0x3f9a45, 0xc48a00, 0x3b7ee0, 0xb04eb0, 0x22a3b0, 0xf4f1ea
    ]
  },
  ink: {
    fg: 0xece7da, bg: 0x141414, cursor: 0xece7da,
    ansi: [
      0x141414, 0xe0574c, 0x7ac77f, 0xe5b94a, 0x6ea3ef, 0xd086d0, 0x6fd0da, 0xbdb8ab,
      0x5a564e, 0xff6f63, 0x93e39a, 0xffd06a, 0x8fbcff, 0xe9a0e9, 0x8be6ef, 0xece7da
    ]
  }
};

/* nus_render::Instance kinds. A grid scene uses rect, glyph and stroke. */
const RECT = 0, GLYPH = 1, STROKE = 4;

const css = (r, g, b, a) =>
  `rgba(${Math.round(r * 255)},${Math.round(g * 255)},${Math.round(b * 255)},${a})`;

function parseCast(text) {
  const lines=text.split('\n').filter(l=>l.trim());
  const dimension=n=>Math.max(1,Math.min(1000,Number.isFinite(+n)?+n:80));
  const header=JSON.parse(lines[0]||'{}');const enc=new TextEncoder(),events=[];let duration=0;
  for(let i=1;i<lines.length;i++) {
    let e;try{e=JSON.parse(lines[i]);}catch{continue;}
    if(!Number.isFinite(e[0])||e[0]<0)continue;duration=Math.max(duration,e[0]);
    if(e[1]==='o')events.push([e[0],enc.encode(e[2]),'o']);
    if(e[1]==='r'&&typeof e[2]==='string') {const size=e[2].split('x').map(Number);if(size.length===2&&size.every(Number.isFinite))events.push([e[0],size.map(dimension),'r']);}
  }
  return {cols:dimension(header.width||80),rows:dimension(header.height||24),events,duration};
}

/** The glyph atlas the renderer packs into, mirrored into a canvas. */
class Atlas {
  constructor(size) {
    this.size = size;
    this.height = 256;               // grows as the shelves fill
    this.canvas = document.createElement('canvas');
    this.canvas.width = size;
    this.canvas.height = this.height;
    this.ctx = this.canvas.getContext('2d');
  }

  grow(needed) {
    if (needed <= this.height) return;
    const next = Math.min(this.size, Math.max(needed, this.height * 2));
    const c = document.createElement('canvas');
    c.width = this.size;
    c.height = next;
    const ctx = c.getContext('2d');
    ctx.drawImage(this.canvas, 0, 0);
    this.canvas = c;
    this.ctx = ctx;
    this.height = next;
  }

  /** Packed uploads: [x,y,w,h] LE u32 then w*h coverage bytes, repeated. */
  absorb(bytes) {
    const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    let o = 0;
    while (o + 16 <= bytes.length) {
      const x = dv.getUint32(o, true);
      const y = dv.getUint32(o + 4, true);
      const w = dv.getUint32(o + 8, true);
      const h = dv.getUint32(o + 12, true);
      o += 16;
      const n = w * h;
      if (o + n > bytes.length) break;
      if (w && h) {
        this.grow(y + h);
        // Coverage becomes white with alpha, so a glyph can be tinted later.
        const img = new ImageData(w, h);
        const px = img.data;
        for (let i = 0; i < n; i++) {
          px[i * 4] = 255; px[i * 4 + 1] = 255; px[i * 4 + 2] = 255;
          px[i * 4 + 3] = bytes[o + i];
        }
        this.ctx.putImageData(img, x, y);
      }
      o += n;
    }
  }
}

class HeroTerminal {
  constructor(root) {
    this.root = root;
    this.canvas = root.querySelector('canvas');
    this.ctx = this.canvas.getContext('2d', { alpha: false });
    this.status = root.querySelector('[data-term-status]');
    this.playBtn = root.querySelector('[data-term-play]');
    this.scrub = root.querySelector('[data-term-scrub]');

    this.cursor = 0;
    this.clock = 0;
    this.playing = false;
    this.raf = null;
  }

  /* --- theme ------------------------------------------------------------- */

  themeName() {
    const t = document.documentElement.getAttribute('data-theme');
    if (t === 'ink' || t === 'paper') return t;
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'ink' : 'paper';
  }

  applyTheme() {
    const t = THEMES[this.themeName()];
    this.shell.set_theme(t.fg, t.bg, t.cursor, new Uint32Array(t.ansi));
    this.theme = t;
  }

  /* --- layout ------------------------------------------------------------ */

  layout() {
    const box = this.canvas.parentElement.getBoundingClientRect();
    if (!box.width || !box.height) return;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    this.pad = 11 * dpr;

    const availW = Math.max(20, box.width * dpr - this.pad * 2);
    const availH = Math.max(20, box.height * dpr - this.pad * 2);

    // The renderer's cell size scales with px, so one probe is enough.
    const probe = 20;
    this.shell.set_px(probe);
    const cw = this.shell.cell_w / probe;
    const ch = this.shell.cell_h / probe;
    const px = Math.max(6, Math.min(availW / (this.shell.cols * cw), availH / (this.shell.rows * ch)));
    this.shell.set_px(px);
    this.px = px;

    this.canvas.width = Math.round(box.width * dpr);
    this.canvas.height = Math.round(box.height * dpr);
    this.canvas.style.width = box.width + 'px';
    this.canvas.style.height = box.height + 'px';

    // The scratch layer tints one run of glyphs; it matches the target exactly.
    if (!this.scratch) this.scratch = document.createElement('canvas');
    this.scratch.width = this.canvas.width;
    this.scratch.height = this.canvas.height;
    this.sctx = this.scratch.getContext('2d');

    this.paint();
  }

  /* --- transport --------------------------------------------------------- */

  wire() {
    if (this.playBtn) {
      this.playBtn.addEventListener('click', () => {
        if (this.playing) { this.userPaused = true; this.pause(); }
        else { this.userPaused = false; this.play(); }
      });
    }
    if (this.scrub) {
      this.scrub.max = String(Math.max(0.01, this.duration));
      this.scrub.addEventListener('input', () => {
        this.userPaused = true;
        this.pause();
        this.seek(Number(this.scrub.value));
      });
    }
    new IntersectionObserver((es) => {
      es.forEach((e) => {
        if (!e.isIntersecting && this.playing) { this.pause(true); this.autoPaused = true; }
        else if (e.isIntersecting && this.autoPaused) { this.autoPaused = false; this.play(); }
      });
    }, { threshold: 0.15 }).observe(this.root);
  }

  play() {
    if (this.playing || this.userPaused) return;
    if (this.clock >= this.duration) this.seek(0);
    this.playing = true;
    this.last = performance.now();
    this.setStatus();
    const step = (now) => {
      if (!this.playing) return;
      const dt = Math.min(0.25, (now - this.last) / 1000);
      this.last = now;
      let next=this.clock+dt*(this.speed||1);
      const upcoming=this.cast.events[this.cursor]?.[0];
      if(this.skipIdle&&upcoming-next>2)next=upcoming;
      this.advanceTo(next);
      if (this.clock >= this.duration) {
        this.pause(true);
        this.setStatus('end');
        if(this.loopEnabled) this.loop = setTimeout(() => {
          if (!this.autoPaused && !this.userPaused) { this.seek(0); this.play(); }
        }, 2600);
        return;
      }
      this.raf = requestAnimationFrame(step);
    };
    this.raf = requestAnimationFrame(step);
  }

  pause(quiet) {
    this.playing = false;
    if (this.raf) cancelAnimationFrame(this.raf);
    if (this.loop) { clearTimeout(this.loop); this.loop = null; }
    if (!quiet) this.setStatus();
  }

  seek(t) {
    t=Math.max(0,Math.min(this.duration,Number.isFinite(t)?t:0));
    if (t < this.clock) {
      if (this.loop) { clearTimeout(this.loop); this.loop = null; }
      this.shell.free();
      this.shell = new Shell(this.cast.cols, this.cast.rows, 2000, this.px || 20);
      this.atlas = new Atlas(this.shell.atlas_size);this.painted=false;
      this.applyTheme();
      this.cursor = 0;
      this.clock = 0;
      this.layout();
    }
    this.advanceTo(t);
  }

  advanceTo(t) {
    const ev = this.cast.events;
    let fed = false;
    while (this.cursor < ev.length && ev[this.cursor][0] <= t) {
      if(ev[this.cursor][2]==='r') {this.shell.resize(...ev[this.cursor][1]);this.layout();}
      else this.shell.feed(ev[this.cursor][1]);
      this.cursor++;
      fed = true;
    }
    this.clock = Math.min(t, this.duration);
    if (this.scrub && document.activeElement !== this.scrub) {
      this.scrub.value = String(this.clock);
    }
    if (fed || !this.painted) this.paint();
    this.setStatus();
  }

  /* --- paint: consume the renderer's draw list --------------------------- */

  paint() {
    if (!this.shell || !this.sctx) return;
    this.painted = true;

    this.shell.draw(true);

    // Glyphs the renderer rasterised on this frame go into the atlas first.
    const alen = this.shell.atlas_len;
    if (alen) {
      this.atlas.absorb(new Uint8Array(this.memory.buffer, this.shell.atlas, alen));
    }

    const inst = new Float32Array(
      this.memory.buffer, this.shell.instances, this.shell.instances_len
    );
    const S = this.shell.stride;
    const A = this.atlas.size;
    const ctx = this.ctx;
    const pad = this.pad;

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.fillStyle = '#' + this.shell.default_bg.toString(16).padStart(6, '0');
    ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);

    // Glyphs batch by colour; anything else flushes the batch, so the
    // renderer's push order is preserved exactly.
    let batch = null;
    let batchColor = '';

    const flush = () => {
      if (!batch || !batch.length) { batch = null; return; }
      const s = this.sctx;
      s.setTransform(1, 0, 0, 1, 0, 0);
      s.clearRect(0, 0, this.scratch.width, this.scratch.height);
      s.globalCompositeOperation = 'source-over';
      for (const g of batch) {
        s.drawImage(
          this.atlas.canvas,
          g[4] * A, g[5] * A, (g[6] - g[4]) * A, (g[7] - g[5]) * A,
          pad + g[0], pad + g[1], g[2], g[3]
        );
      }
      s.globalCompositeOperation = 'source-in';
      s.fillStyle = batchColor;
      s.fillRect(0, 0, this.scratch.width, this.scratch.height);
      s.globalCompositeOperation = 'source-over';
      ctx.drawImage(this.scratch, 0, 0);
      batch = null;
    };

    for (let o = 0; o < inst.length; o += S) {
      const kind = inst[o + 12];
      const color = css(inst[o + 8], inst[o + 9], inst[o + 10], inst[o + 11]);

      if (kind === GLYPH) {
        if (batch && batchColor !== color) flush();
        if (!batch) { batch = []; batchColor = color; }
        batch.push([
          inst[o], inst[o + 1], inst[o + 2], inst[o + 3],
          inst[o + 4], inst[o + 5], inst[o + 6], inst[o + 7]
        ]);
        continue;
      }

      flush();

      if (kind === RECT) {
        ctx.fillStyle = color;
        ctx.fillRect(pad + inst[o], pad + inst[o + 1], inst[o + 2], inst[o + 3]);
      } else if (kind === STROKE) {
        // uv carries [radius, thickness]; the grid uses it for a hollow cursor.
        const t = inst[o + 5] || 1;
        ctx.strokeStyle = color;
        ctx.lineWidth = t;
        ctx.strokeRect(
          pad + inst[o] + t / 2, pad + inst[o + 1] + t / 2,
          inst[o + 2] - t, inst[o + 3] - t
        );
      }
      // Other kinds (rounded, hazard, texture) never appear in a grid scene.
    }
    flush();
  }

  setStatus(kind) {
    if (this.playBtn) {
      this.playBtn.setAttribute('aria-label', this.playing ? 'Pause' : 'Play');
      this.playBtn.textContent = this.playing ? '❚❚' : '▶';
    }
    if (!this.status) return;
    this.status.textContent =
      kind === 'end' ? 'replay ended' : this.playing ? 'replaying' : 'paused';
  }
}

/* --- boot ------------------------------------------------------------------ */

export async function mountHeroTerminal(root, opts) {
  const term = new HeroTerminal(root);
  const wasm = await init({ module_or_path: opts.wasm });
  term.memory = wasm.memory;

  const castText = opts.castText ?? await fetch(opts.cast).then((r) => {
    if (!r.ok) throw new Error(`cast ${r.status}`);
    return r.text();
  });

  term.cast = parseCast(castText);
  term.duration = term.cast.duration;
  term.loopEnabled=opts.loop!==false;term.speed=1;term.skipIdle=false;

  term.shell = new Shell(term.cast.cols, term.cast.rows, 2000, 20);
  term.atlas = new Atlas(term.shell.atlas_size);
  term.applyTheme();

  term.layout();
  new ResizeObserver(() => term.layout()).observe(term.canvas.parentElement);
  window.addEventListener('nus:theme', () => { term.applyTheme(); term.paint(); });

  term.wire();

  const sizeLabel = root.querySelector('[data-term-size]');
  if (sizeLabel) sizeLabel.textContent = `${term.cast.cols}×${term.cast.rows}`;

  root.dataset.ready = 'true';

  if (opts.autoplay===false) {
    term.userPaused=true;term.seek(0);term.setStatus();
  } else if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    term.userPaused = true;
    term.seek(term.duration);
    term.setStatus();
  } else {
    term.play();
  }
  return term;
}

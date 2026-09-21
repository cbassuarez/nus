/* Shared live/saved article extraction. This reader emits data, never HTML.
 * No DOM writes, script execution, fetches, hidden tabs, or page navigation.
 * Limits fail explicitly rather than returning a silently truncated article.
 */
(function () {
  'use strict';
  const LIMIT = { nodes: 20000, blocks: 4096, text: 256 * 1024, links: 256 };
  const clean = s => String(s || '').replace(/[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]/g, '');
  const notices = new Set();
  let visited = 0, bytes = 0;
  const all = document.getElementsByTagName('*');
  if (all.length > LIMIT.nodes) throw new Error('Page is too large for a bounded reading capture');
  const visible = e => {
    if (e.hidden || e.getAttribute('aria-hidden') === 'true') return false;
    const s = getComputedStyle(e);
    return s.display !== 'none' && s.visibility !== 'hidden' && s.visibility !== 'collapse';
  };
  const forbidden = /^(SCRIPT|STYLE|NAV|ASIDE|FOOTER|HEADER|FORM|BUTTON|NOSCRIPT|TEMPLATE|INPUT|SELECT|TEXTAREA)$/;
  // textContent on a visible paragraph can include hidden descendants. Walk
  // inline content without including forms, hidden answers, scripts or notices.
  const rawText = (node, depth = 0) => {
    if (depth > 96) throw new Error('Article inline structure exceeds the reading budget');
    if (node.nodeType === Node.TEXT_NODE) return node.nodeValue || '';
    if (node.nodeType !== Node.ELEMENT_NODE || forbidden.test(node.tagName) || !visible(node)) return '';
    if (node.tagName === 'BR') return '\n';
    return Array.from(node.childNodes, n => rawText(n, depth + 1)).join('');
  };
  const text = e => clean(rawText(e)).replace(/\s+/g, ' ').trim();
  const visibleIn = (e, stop) => {
    for (let p = e; p && p !== stop; p = p.parentElement) if (!visible(p) || forbidden.test(p.tagName)) return false;
    return true;
  };

  const metadata = name => {
    const m = document.querySelector('meta[property="' + name + '"],meta[name="' + name + '"]');
    return clean(m ? m.content : '');
  };
  let best = document.body, score = 0;
  for (const c of document.querySelectorAll('article,main,[role="main"],#content,#main,.post,.article,.entry-content,body')) {
    if (!visible(c)) continue;
    let n = 0;
    for (const p of c.querySelectorAll('p,pre')) {
      if (visibleIn(p, c) && !p.closest('nav,aside,footer,form,header')) n += text(p).length;
    }
    n *= c.tagName === 'BODY' ? 0.45 : 1;
    if (n > score) { best = c; score = n; }
  }
  if (!best || score < 80) throw new Error('No substantial article was found; only the link can be saved');
  if (document.querySelector('input[type="password"]') && !best.matches('article,.post,.article,.entry-content') && !best.querySelector('article,.post,.article,.entry-content')) {
    throw new Error('Sign-in form detected; only the link can be saved');
  }
  const h1 = best.querySelector('h1') || document.querySelector('h1');
  const title = clean((h1 && text(h1)) || metadata('og:title') || document.title);
  const byline = metadata('author') || metadata('article:author');
  const when = metadata('article:published_time') || metadata('date');
  if ([title, byline, when].some(x => new TextEncoder().encode(x).length > 8192)) {
    throw new Error('Article metadata is too large');
  }
  const blocks = [], links = new Map();
  function push(b) {
    bytes += new TextEncoder().encode(b.x || '').length + new TextEncoder().encode(b.src || '').length;
    if (bytes > LIMIT.text || blocks.length >= LIMIT.blocks) throw new Error('Article exceeds the reading budget; no truncated copy was returned');
    blocks.push(b);
  }
  function address(raw) {
    try {
      const u = new URL(raw, document.baseURI);
      return /^https?:$/.test(u.protocol) && !u.username && !u.password && u.href.length <= 8192 ? u.href : null;
    } catch (_) { return null; }
  }
  function references(e) {
    for (const a of e.querySelectorAll('a[href]')) {
      if (!visibleIn(a, best)) continue;
      const url = address(a.getAttribute('href'));
      const label = text(a);
      if (!url || !label || links.has(url)) continue;
      if (links.size >= LIMIT.links) throw new Error('Article has too many links for this capture');
      links.set(url, label);
    }
  }
  function image(e) {
    const raw = e.currentSrc || e.src || '';
    // Large data URLs are a reference to an already-loaded image, not article text.
    const src = raw.length > 8192 ? 'nus-loaded-image:' + Array.prototype.indexOf.call(document.images,e) : raw;
    if (src && (e.naturalWidth || e.width) >= 80) {
      push({ t: 'img', x: clean(e.alt || ''), src });
    }
  }
  function walk(e, depth) {
    if (++visited > LIMIT.nodes || depth > 96) throw new Error('Article structure exceeds the reading budget');
    if (e.nodeType !== Node.ELEMENT_NODE || forbidden.test(e.tagName) || !visible(e)) return;
    const tag = e.tagName;
    if (/^H[1-6]$/.test(tag)) {
      const x = text(e);
      if (x && !(tag === 'H1' && x === title)) push({ t: 'h', l: Number(tag[1]), x });
      return;
    }
    if (tag === 'PRE') {
      const x = clean(rawText(e)); // Preserve tabs, leading space AND final newline.
      if (x.trim()) push({ t: 'pre', x });
      references(e); return;
    }
    if (tag === 'TABLE') {
      const rows = Array.from(e.rows || []);
      if (rows.length > 200 || rows.some(r => r.cells.length > 32)) throw new Error('Table exceeds the reading budget');
      const matrix = rows.filter(r => visibleIn(r, e)).map(r => Array.from(r.cells, c => text(c)));
      if (rows.some(r => Array.from(r.cells).some(c => c.colSpan > 1 || c.rowSpan > 1))) {
        notices.add('A table uses merged cells. Its text is preserved; open the original for exact table layout.');
      }
      const widths = [];
      for (const row of matrix) row.forEach((c, i) => { widths[i] = Math.min(48, Math.max(widths[i] || 0, Array.from(c).length)); });
      const rendered = matrix.map(r => r.map((c, i) => c + ' '.repeat(Math.max(0, widths[i] - Array.from(c).length))).join(' | ')).join('\n');
      if (rendered.trim()) push({ t: 'pre', x: rendered });
      references(e); return;
    }
    if (tag === 'IMG') { image(e); return; }
    if (/^(VIDEO|AUDIO|IFRAME|CANVAS|SVG|MATH|OBJECT|EMBED)$/.test(tag)) {
      notices.add('Interactive media or mathematics is not captured. Open the original to use it.'); return;
    }
    if (tag === 'P' || tag === 'FIGCAPTION' || tag === 'BLOCKQUOTE') {
      const x = text(e);
      if (x) push({ t: tag === 'FIGCAPTION' ? 'cap' : tag === 'BLOCKQUOTE' ? 'q' : 'p', x });
      references(e);
      for (const img of e.querySelectorAll('img')) if (visibleIn(img, e)) image(img);
      if (e.querySelector('math,video,audio,iframe,canvas,svg,object,embed')) {
        notices.add('Interactive media or mathematics is not captured. Open the original to use it.');
      }
      return;
    }
    if (tag === 'LI') {
      const own = Array.from(e.childNodes).filter(n => n.nodeType !== Node.ELEMENT_NODE || !/^(UL|OL)$/.test(n.tagName));
      const x = clean(own.map(n => rawText(n)).join('')).replace(/\s+/g, ' ').trim();
      if (x) push({ t: 'li', x });
      references(e);
      for (const img of e.querySelectorAll(':scope > img,:scope > p img')) if (visibleIn(img, e)) image(img);
      for (const c of e.children) if (/^(UL|OL)$/.test(c.tagName)) walk(c, depth + 1);
      return;
    }
    // Preserve prose that is not wrapped in a <p>, without duplicating children.
    let loose = '';
    const flush = () => { const x = clean(loose).replace(/\s+/g, ' ').trim(); if (x) push({ t: 'p', x }); loose = ''; };
    for (const c of e.childNodes) {
      if (c.nodeType === Node.TEXT_NODE) loose += c.nodeValue || '';
      else if (c.nodeType === Node.ELEMENT_NODE) {
        if (/^(SPAN|B|STRONG|I|EM|CODE|A|SMALL|SUP|SUB|ABBR|TIME)$/.test(c.tagName) && visible(c)) { loose += rawText(c); references(c); if (c.matches('a[href]')) { const u = address(c.getAttribute('href')); if (u && text(c)) { if (!links.has(u) && links.size >= LIMIT.links) throw new Error('Article has too many links for this capture'); links.set(u, text(c)); } } }
        else { flush(); walk(c, depth + 1); }
      }
    }
    flush();
  }
  walk(best, 0);
  if (!blocks.some(b => /^(p|pre|q|li)$/.test(b.t) && b.x.trim())) throw new Error('No readable article content');
  // Link destinations are native, keyboard-reachable references. No extracted
  // HTML is mounted. Inline rich-text layout is deliberately not impersonated.
  if (links.size) {
    push({ t: 'h', l: 2, x: 'Links in this article' });
    for (const [src, label] of links) push({ t: 'link', x: label, src });
  }
  for (const n of notices) push({ t: 'cap', x: n });
  const payload = JSON.stringify({ title, byline, when, blocks });
  if (new TextEncoder().encode(JSON.stringify({result:{type:'string',value:payload}})).length > 900000)
    throw new Error('Article exceeds the inspection budget; no truncated article was returned');
  return payload;
})()

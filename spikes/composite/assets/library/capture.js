/* Called with the shared extractor's JSON and a native-generated capture token.
 * It reads only images already decoded by this page. Canvas tainting enforces
 * the origin boundary. No fetch fallback, credentials copied, or network I/O.
 */
(function (token, articleJSON) {
  'use strict';
  const article = JSON.parse(articleJSON);
  const media = [], notices = new Set();
  let missing = 0, total = 0, pixels = 0;
  const saved = new Map();
  const byURL = new Map();
  for (const [index,img] of Array.from(document.images).entries()) {
    const src = img.currentSrc || img.src;
    if (src && img.complete && img.naturalWidth) {
      if (!byURL.has(src)) byURL.set(src, img);
      byURL.set("nus-loaded-image:" + index, img);
    }
  }
  for (const block of article.blocks) {
    if (block.t !== 'img') continue;
    const img = byURL.get(block.src);
    const original = img ? (img.currentSrc || img.src) : block.src;
    block.src = ''; // No external URL survives as an offline image source.
    if (saved.has(original)) { block.src = saved.get(original); continue; }
    let canvas;
    try {
      if (!img || media.length >= 12) throw new Error('unavailable');
      const w = img.naturalWidth, h = img.naturalHeight;
      if (w > 2048 || h > 2048 || w * h > 1048576 || pixels + w * h > 2097152) throw new Error('oversized');
      canvas = document.createElement('canvas');
      canvas.width = w; canvas.height = h;
      const ctx = canvas.getContext('2d');
      if (!ctx) throw new Error('canvas unavailable');
      ctx.drawImage(img, 0, 0);
      const url = canvas.toDataURL('image/png');
      const comma = url.indexOf(',');
      if (!url.startsWith('data:image/png;base64,') || comma < 0 || url.length > 100000) throw new Error('oversized');
      const bytes = atob(url.slice(comma + 1));
      if (bytes.length > 65536 || total + bytes.length > 196608) throw new Error('budget');
      const hex = new Array(bytes.length);
      for (let i = 0; i < bytes.length; i++) hex[i] = bytes.charCodeAt(i).toString(16).padStart(2, '0');
      const id = 'image-' + media.length;
      media.push({ id, width: w, height: h, png: hex.join('') });
      total += bytes.length; pixels += w * h;
      saved.set(original, id);
      block.src = id;
    } catch (_) {
      missing++;
      notices.add('Some images could not be saved without another network request or crossing an origin boundary. Their alternative text is retained.');
    } finally {
      if (canvas) canvas.width = canvas.height = 0;
    }
  }
  let result = { schema: 1, token, url: location.href, epoch: performance.timeOrigin,
    document: article, media, missing_images: missing, notices: Array.from(notices) };
  let payload = JSON.stringify(result);
  // Account for the CDP wrapper's escaping of our returned JSON string.
  if (new TextEncoder().encode(JSON.stringify({result: {type: 'string', value: payload}})).length > 900000) {
    for (const b of article.blocks) if (b.t === 'img') b.src = '';
    result.media = [];
    result.missing_images = article.blocks.filter(b => b.t === 'img').length;
    result.notices.push('Images omitted to stay within the inspection budget. Text is preserved.');
    payload = JSON.stringify(result);
  }
  if (new TextEncoder().encode(JSON.stringify({result: {type: 'string', value: payload}})).length > 900000)
    throw new Error('Article exceeds the inspection budget; no truncated copy was saved');
  return payload;
})

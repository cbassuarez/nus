-- name: memphis
-- says: the kit — squiggle, tape, halftone, solids, the six signals — framing the line

-- Memphis: the promo's kit, made of what nus already draws. One signal
-- each, a 2 px ink line and the hard 8×8 shadow. Composed: the big shapes
-- bleed off the edges, the small ones stack on them, the middle is the
-- line's. They drop in on the beat, each landing before its shadow, then
-- hold nearly still.

local TAU = math.pi * 2
local function clamp(v, a, b) return if v < a then a elseif v > b then b else v end
-- The kit's entry: a quick pop with a little overshoot.
local function pop(t)
  t = clamp(t, 0, 1)
  if t <= 0 then return 0 end
  local c1 = 1.70158
  local c3 = c1 + 1
  return 1 + c3 * (t - 1) ^ 3 + c1 * (t - 1) ^ 2
end

-- The composition, in beat order, for a 1280×800 pane; scaled to the pane.
local pieces = {
  { kind = "quarter", s = 320, x = 1180, y = 690, col = "blue", at = 0.0, depth = 0.9 },
  { kind = "circle", s = 210, x = 80, y = 640, col = "red", at = 0.18, depth = 1.0 },
  { kind = "squiggle", w = 440, h = 110, weight = 44, waves = 1.5, x = 300, y = 118, rot = -7, col = "violet", at = 0.34, depth = 0.6 },
  { kind = "dots", w = 300, h = 220, x = 1090, y = 150, col = "teal", at = 0.5, depth = 0.25 },
  { kind = "half", s = 170, x = 690, y = 128, col = "green", at = 0.62, depth = 0.5 },
  { kind = "stripes", w = 380, h = 44, x = 470, y = 700, rot = -9, col = "gold", at = 0.76, depth = 0.7 },
  { kind = "square", s = 78, x = 1030, y = 560, rot = 14, col = "gold", at = 0.88, depth = 0.85 },
  { kind = "zigzag", w = 250, h = 26, stroke = 5, x = 330, y = 596, col = "red", at = 1.0, depth = 0.4 },
  { kind = "sticker", s = 84, x = 1180, y = 470, at = 1.12, depth = 0.95 },
  { kind = "circle", s = 54, x = 890, y = 232, col = "teal", at = 1.24, depth = 0.3 },
}
for i, p in pieces do p.nod = -10; p.lift = -10; p.ph = i * 1.7; p.rot = p.rot or 0 end
local nudges = 0
local lastTyped = ""

-- Points, rotated about a centre and scaled.
local function place(px, py, cx, cy, rot, sc)
  local cr, sr = math.cos(rot), math.sin(rot)
  local x, y = px * sc, py * sc
  return cx + x * cr - y * sr, cy + x * sr + y * cr
end

local function squigglePts(p, phase, k)
  -- k: a point list for the outline in the piece's own box (origin at its centre).
  local n = 64
  local top, bottom = {}, {}
  for i = 0, n do
    local t = i / n
    local x = t * p.w - p.w / 2
    local y = (p.h / 2 - p.weight / 2) * math.sin(TAU * p.waves * t + phase)
    local dy = (p.h / 2 - p.weight / 2) * math.cos(TAU * p.waves * t + phase) * TAU * p.waves / p.w
    local len = math.sqrt(1 + dy * dy)
    local w = (p.weight / 2) * (0.25 + 0.75 * math.sin(math.pi * t))
    local nx, ny = -dy / len, 1 / len
    top[#top + 1] = { x + nx * w, y + ny * w }
    table.insert(bottom, 1, { x - nx * w, y - ny * w })
  end
  for _, q in bottom do top[#top + 1] = q end
  return top
end

local function polyOf(p, phase)
  if p.kind == "squiggle" then return squigglePts(p, phase) end
  local s = p.s or 0
  local pts = {}
  if p.kind == "circle" then
    for i = 0, 47 do local a = i / 48 * TAU; pts[#pts + 1] = { math.cos(a) * s / 2, math.sin(a) * s / 2 } end
  elseif p.kind == "square" then
    pts = { { -s / 2, -s / 2 }, { s / 2, -s / 2 }, { s / 2, s / 2 }, { -s / 2, s / 2 } }
  elseif p.kind == "quarter" then
    pts[1] = { -s / 2, s / 2 }
    for i = 0, 24 do local a = -math.pi / 2 + i / 24 * (math.pi / 2); pts[#pts + 1] = { -s / 2 + math.cos(a) * s, s / 2 + math.sin(a) * s } end
  elseif p.kind == "half" then
    for i = 0, 24 do local a = math.pi + i / 24 * math.pi; pts[#pts + 1] = { math.cos(a) * s / 2, math.sin(a) * s / 2 } end
  elseif p.kind == "stripes" then
    pts = { { -p.w / 2, -p.h / 2 }, { p.w / 2, -p.h / 2 }, { p.w / 2, p.h / 2 }, { -p.w / 2, p.h / 2 } }
  elseif p.kind == "sticker" then
    for i = 0, 47 do local a = i / 48 * TAU; pts[#pts + 1] = { math.cos(a) * s / 2, math.sin(a) * s / 2 } end
  end
  return pts
end

local function drawPiece(c, p, t, sc, ink, paper, white, pointer)
  local s = pop((t - 0.25 - p.at) / 0.36)
  if s <= 0 then return end
  local land = clamp((t - 0.25 - p.at - 0.2) / 0.3, 0, 1)
  local nod = if p.nod >= 0 then 1 + 0.07 * math.sin(math.pi * clamp((t - p.nod) / 0.34, 0, 1)) else 1
  local lift = if p.lift >= 0 then math.sin(math.pi * clamp((t - p.lift) / 0.6, 0, 1)) else 0
  local fx = math.sin(t * 0.33 + p.ph) * 2.5 * p.depth
  local fy = math.cos(t * 0.27 + p.ph) * 2.5 * p.depth
  local px, py = 0, 0
  if pointer then px = (pointer.x / sc - 640) / 1280 * 12 * p.depth; py = (pointer.y / sc - 400) / 800 * 9 * p.depth end
  local cx, cy = (p.x + fx + px) * sc, (p.y + fy + py) * sc
  local scale = s * nod * (1 + 0.04 * lift) * sc
  local rot = p.rot * math.pi / 180
  local off = (8 * land + 10 * lift) * sc
  local col = c.signals[p.col or "red"]
  local phase = t * 0.45
  local function put(pts, ox, oy)
    local out = {}
    for i, q in pts do
      local x, y = place(q[1], q[2], cx + ox - lift * 6, cy + oy - lift * 6, rot, scale)
      out[i] = { x, y }
    end
    return out
  end
  if p.kind == "dots" then
    local pitch = 20
    local cols, rows = math.floor(p.w / pitch), math.floor(p.h / pitch)
    local ox2, oy2 = 1 + 0.12 * math.sin(t * 0.3), 0.12 * math.cos(t * 0.23)
    for r = 0, rows do
      for q = 0, cols do
        local u, v = q / cols, r / rows
        local d = math.sqrt((u - ox2) ^ 2 + (v - oy2) ^ 2) * 0.8
        local rad = (pitch / 2) * 0.86 * (1 - d)
        if rad > 0.7 then
          local x, y = place(q * pitch - p.w / 2, r * pitch - p.h / 2, cx, cy, 0, scale)
          c:circle(x, y, rad * scale, col)
        end
      end
    end
    return
  end
  if p.kind == "zigzag" then
    local teeth = math.max(3, math.floor(p.w / (p.h * 1.2) + 0.5))
    local pts = {}
    for k = 0, teeth * 2 do
      local zx = k / (teeth * 2) * p.w - p.w / 2
      local zy = if k % 2 == 1 then -p.h / 2 + p.stroke / 2 else p.h / 2 - p.stroke / 2
      pts[#pts + 1] = { zx, zy }
    end
    local out = put(pts, 0, 0)
    for i = 1, #out - 1 do c:line(out[i][1], out[i][2], out[i + 1][1], out[i + 1][2], p.stroke * scale, col) end
    return
  end
  local shape = polyOf(p, phase)
  -- The shadow lands after the shape.
  if off > 0.2 then c:poly(put(shape, off, off), ink, math.min(1, land + lift)) end
  if p.kind == "stripes" then
    c:poly(put(shape, 0, 0), paper)
    -- The tape: diagonal bands, crawling; each band a quad clipped to the box by construction.
    local band = p.h * 0.45
    local period = band * 2
    local crawl = (t * 10) % period
    local x = -p.w / 2 - p.h * 2 + crawl - period
    while x < p.w / 2 + p.h do
      -- Clip the parallelogram to the box's left and right edges.
      local x0, x1 = math.max(x, -p.w / 2), math.min(x + p.h + band, p.w / 2)
      if x1 > x0 then
        local pts = {}
        local top0, top1 = x + p.h, x + p.h + band
        local bot0, bot1 = x, x + band
        pts[1] = { clamp(bot0, -p.w / 2, p.w / 2), p.h / 2 }
        pts[2] = { clamp(top0, -p.w / 2, p.w / 2), -p.h / 2 }
        pts[3] = { clamp(top1, -p.w / 2, p.w / 2), -p.h / 2 }
        pts[4] = { clamp(bot1, -p.w / 2, p.w / 2), p.h / 2 }
        c:poly(put(pts, 0, 0), col)
      end
      x += period
    end
    local o = put(shape, 0, 0)
    for i = 1, 4 do local a, b = o[i], o[i % 4 + 1]; c:line(a[1], a[2], b[1], b[2], 2 * sc, ink) end
    return
  end
  local fill = if p.kind == "sticker" then white else col
  local o = put(shape, 0, 0)
  c:poly(o, fill)
  for i = 1, #o do local a, b = o[i], o[i % #o + 1]; c:line(a[1], a[2], b[1], b[2], 2 * sc, ink) end
  if p.kind == "sticker" then
    c:text(cx - lift * 6, cy - lift * 6 + p.s * 0.18 * scale, "»", p.s * 0.5 * scale, c.signals.red, 1, { font = "strong", align = "center" })
  end
end

function draw(c)
  local sc = math.min(c.w / 1280, c.h / 800)
  local ink = c.ink
  local paper = c.paper
  local white = if c.face == "paper" then "#ffffff" else "#1e1e1d"
  -- Typing makes the next piece nod; a tap lifts the piece under it.
  if c.typed ~= lastTyped then
    if #c.typed > #lastTyped then
      nudges += 1
      local p = pieces[(nudges - 1) % #pieces + 1]
      p.nod = c.t
    end
    lastTyped = c.typed
  end
  for _, tp in c:taps() do
    for i = #pieces, 1, -1 do
      local p = pieces[i]
      local w, h = (p.w or p.s) * sc, (p.h or p.s) * sc
      if math.abs(tp.x - p.x * sc) <= w / 2 + 8 and math.abs(tp.y - p.y * sc) <= h / 2 + 8 then p.lift = c.t; break end
    end
  end
  for _, p in pieces do drawPiece(c, p, c.t, sc, ink, paper, white, c.pointer) end
end

-- name: the pond
-- says: four koi seen from above, slow, quiet, minding the line

-- The pond: a few koi drawn from a bending spine — a long body with a real
-- width profile, translucent fins, a forked tail that lags the body's
-- wave, patches that follow the bend, a soft shadow on the floor. They
-- keep off the line, gather a little when you type, shy from the pointer.

local N = 26
local TAU = math.pi * 2
local fish = {}
local ripples = {}
local W, H = 0, 0
local gather = 0
local pointer = nil
local seeded = false

local function rnd(a, b) return a + math.random() * (b - a) end
local function clamp(v, a, b) return if v < a then a elseif v > b then b else v end

-- Half-width along the body (0 head … 1 tail root), in body lengths.
local PROFILE = { {0, 0.065}, {0.05, 0.105}, {0.14, 0.13}, {0.3, 0.135}, {0.48, 0.115}, {0.66, 0.085}, {0.82, 0.055}, {0.93, 0.04}, {1, 0.038} }
local function halfWidth(s)
  for i = 2, #PROFILE do
    if s <= PROFILE[i][1] then
      local a, b = PROFILE[i - 1], PROFILE[i]
      local t = (s - a[1]) / (b[1] - a[1])
      t = t * t * (3 - 2 * t)
      return a[2] + (b[2] - a[2]) * t
    end
  end
  return PROFILE[#PROFILE][2]
end

-- Markings: blobs in body space (u along 0..1, v across -1..1).
local function blobPts(u, v, ru, rv)
  local pts = {}
  for k = 0, 8 do
    local a = k / 9 * TAU
    local w = 0.78 + math.random() * 0.44
    pts[#pts + 1] = { u + math.cos(a) * ru * w, v + math.sin(a) * rv * w }
  end
  return pts
end
local function markings(i)
  local kinds = { "kohaku", "tancho", "kohaku", "sanke", "kohaku", "shiro", "kohaku", "tancho" }
  local kind = kinds[(i - 1) % #kinds + 1]
  local out = {}
  if kind == "kohaku" then
    out = { { pts = blobPts(0.14, -0.1, 0.13, 0.9), red = true }, { pts = blobPts(0.42, 0.25, 0.14, 0.75), red = true }, { pts = blobPts(0.7, -0.2, 0.1, 0.7), red = true } }
  elseif kind == "tancho" then
    out = { { pts = blobPts(0.1, 0.0, 0.09, 0.55), red = true } }
  elseif kind == "sanke" then
    out = { { pts = blobPts(0.2, 0.15, 0.15, 0.85), red = true }, { pts = blobPts(0.55, -0.3, 0.12, 0.6), red = true }, { pts = blobPts(0.38, 0.5, 0.05, 0.3), red = false }, { pts = blobPts(0.66, 0.2, 0.05, 0.35), red = false } }
  end
  return out
end

local Lmax = 1e9
local function makeFish(i)
  local depth = ({ 1, 0.84, 0.7 })[(i - 1) % 3 + 1]
  local L = math.min(rnd(170, 215) * (W / 1280), Lmax) * depth
  local f = {
    i = i, depth = depth, L = L,
    x = rnd(L, W - L), y = rnd(90, H - 90),
    th = rnd(0, TAU), speed = rnd(14, 22) * depth, wander = rnd(0, TAU),
    phase = rnd(0, TAU), turn = 0, burst = 0,
    marks = markings(i),
    px = {}, py = {}, pa = {},
  }
  for k = 1, N do f.px[k] = f.x; f.py[k] = f.y; f.pa[k] = f.th + math.pi end
  return f
end

local function spine(f)
  local seg = f.L / (N - 1)
  local amp = 0.09 + 0.05 * math.min(1, f.burst + gather * 0.5)
  local x, y, a = f.x, f.y, f.th + math.pi
  for i = 1, N do
    local s = (i - 1) / (N - 1)
    local wave = amp * s ^ 1.3 * math.sin(f.phase - s * 5.2)
    local bend = f.turn * 0.55 * s
    local ai = a + wave - bend
    f.px[i], f.py[i], f.pa[i] = x, y, ai
    x += math.cos(ai) * seg
    y += math.sin(ai) * seg
  end
end

local function at(f, i, k)
  local s = (i - 1) / (N - 1)
  local hw = halfWidth(s) * f.L
  local nx, ny = -math.sin(f.pa[i]), math.cos(f.pa[i])
  return f.px[i] + nx * hw * k, f.py[i] + ny * hw * k
end
local function atUV(f, u, v)
  local fi = u * (N - 1) + 1
  local i = math.min(N - 1, math.floor(fi))
  local t = fi - i
  local ax, ay = at(f, i, v)
  local bx, by = at(f, i + 1, v)
  return ax + (bx - ax) * t, ay + (by - ay) * t
end

local function step(c, dt)
  local line = c.prompt
  local cx = line.x + line.w / 2
  for i, f in fish do
    f.wander += (math.random() - 0.5) * 1.4 * dt
    local fx, fy = math.cos(f.wander) * 0.4, math.sin(f.wander) * 0.4
    local m = f.L * 0.5
    -- The pane's edges, judged by the whole body: head, middle, tail.
    local bx, by = (f.px[1] + f.px[N]) / 2, (f.py[1] + f.py[N]) / 2
    local ex, exr = math.min(f.px[1], bx, f.px[N]), math.max(f.px[1], bx, f.px[N])
    local ey, eyb = math.min(f.py[1], by, f.py[N]), math.max(f.py[1], by, f.py[N])
    -- Walls: the pane's edges and the shore around the line. Each pushes
    -- with a normal into the water and a depth; a fish that has only
    -- brushed one glides along it, a fish deep in the margin turns away.
    local wx, wy, wk = 0, 0, 0
    local sx, sy = 0, 0 -- the strongest wall's normal
    local function wall(nx, ny, k)
      if k > 0 then
        wx += nx * k
        wy += ny * k
        if k > wk then wk = k; sx, sy = nx, ny end
      end
    end
    local margin, ramp = 24, m * 0.4
    wall(1, 0, (margin + ramp - ex) / ramp)
    wall(-1, 0, (exr - (W - margin - ramp)) / ramp)
    wall(0, 1, (margin + ramp - ey) / ramp)
    wall(0, -1, (eyb - (H - 56 - ramp)) / ramp)
    -- The line and the rows beneath: a shore the body never crosses.
    local zx0, zx1 = line.x - 40, line.x + line.w + 40
    local zy0, zy1 = line.y - 36, line.y + line.h + 16 + c.rows
    local hm = f.L * 0.28
    local above = by < (zy0 + zy1) / 2
    local zTop, zBot = zy0 - hm, zy1 + hm
    if bx > zx0 - hm * 0.5 and bx < zx1 + hm * 0.5 then
      if by > zTop - hm and by < zBot + hm then
        -- How clear of the shore the body is; on it, the push is full.
        local clearance = if above then zTop - by else by - zBot
        wall(0, if above then -1 else 1, 1 - clamp(clearance / hm, 0, 1))
      end
      if by > zTop and by < zBot then
        f.y += ((if above then zTop else zBot) - by) * math.min(1, dt * 5)
      end
    end
    local lx, ly = f.x + math.cos(f.th) * f.L * 0.9, f.y + math.sin(f.th) * f.L * 0.9
    if lx > zx0 and lx < zx1 and ly > zy0 - hm * 0.5 and ly < zy1 + hm * 0.5 then wall(0, if above then -1 else 1, 0.8) end
    local edge = clamp(wk, 0, 1)
    if wk > 0 then
      -- Between two walls the pushes cancel: then the strongest wall's
      -- normal is the way out, and gliding along it the way to go.
      local wl = math.sqrt(wx * wx + wy * wy)
      local squeezed = wl < wk * 0.6
      if squeezed then wx, wy = sx, sy else wx, wy = wx / wl, wy / wl end
      local hx, hy = math.cos(f.th), math.sin(f.th)
      local tx, ty = -sy, sx
      if hx * tx + hy * ty < 0 then tx, ty = -tx, -ty end
      local away = if squeezed then edge * 0.3 else edge * edge
      fx += (tx * (1 - away) + wx * away) * (1.5 + 4 * edge)
      fy += (ty * (1 - away) + wy * away) * (1.5 + 4 * edge)
    end
    -- Room for each other.
    for j, g in fish do
      if j ~= i then
        local gx, gy = (g.px[1] + g.px[N]) / 2, (g.py[1] + g.py[N]) / 2
        local dx, dy = bx - gx, by - gy
        local d2 = dx * dx + dy * dy
        local near = (f.L + g.L) * 0.5
        if d2 < near * near and d2 > 1 then
          local d = math.sqrt(d2)
          fx += dx / d * (1 - d / near) * 2.4
          fy += dy / d * (1 - d / near) * 2.4
        end
      end
    end
    -- A gentle pull to the middle of their own water, so the edges are
    -- visited, not lived on.
    local midY = if above then (24 + zTop) / 2 else (zBot + H - 56) / 2
    fx += (W / 2 - f.x) / W * 0.7
    fy += (midY - f.y) / H * 1.4
    -- Typing draws them to the line, each to its own side.
    if gather > 0 then
      local k = (f.i - 0.5) / #fish - 0.5
      local tx = cx + k * line.w * 1.1
      local ty = if above then zTop - 30 - (1 - f.depth) * 60 else zBot + 30 + (1 - f.depth) * 60
      local ddx, ddy = tx - f.x, ty - f.y
      local dl = math.sqrt(ddx * ddx + ddy * ddy)
      if dl > 30 then fx += ddx / dl * gather * 1.1; fy += ddy / dl * gather * 1.1 end
    end
    -- The pointer: shy.
    if pointer then
      local qx, qy = f.x - pointer.x, f.y - pointer.y
      local pd = math.sqrt(qx * qx + qy * qy)
      if pd < 170 and pd > 1 then
        local s = 1 - pd / 170
        fx += qx / pd * s * 3; fy += qy / pd * s * 3
        f.burst = math.max(f.burst, s * 0.8)
      end
    end
    local want = math.atan2(fy, fx)
    local dth = math.atan2(math.sin(want - f.th), math.cos(want - f.th))
    local rate = 1.1 + 1.6 * clamp(edge, 0, 1)
    local turn = clamp(dth, -rate * dt, rate * dt)
    f.th += turn
    f.turn += (turn / dt - f.turn) * math.min(1, dt * 4)
    f.burst = math.max(0, f.burst - dt * 0.5)
    local sp = f.speed * (0.8 + 0.4 * math.min(1, math.abs(fx) + math.abs(fy)) + f.burst * 2 + gather * 0.4) * (W / 1280)
    f.x += math.cos(f.th) * sp * dt
    f.y += math.sin(f.th) * sp * dt
    -- The head, whatever else, stays on the paper.
    f.x = clamp(f.x, 24, W - 24)
    f.y = clamp(f.y, 24, H - 48)
    f.phase += dt * (2.2 + sp * 0.05)
    spine(f)
  end
  for i = #ripples, 1, -1 do if c.t - ripples[i].t > 3.2 then table.remove(ripples, i) end end
end

-- Fins: a soft fan rooted at index i on side k, swept back.
local function fin(f, i, k, len, wid, back)
  local rx, ry = at(f, i, k * 0.85)
  local a = f.pa[i]
  local nx, ny = -math.sin(a) * k, math.cos(a) * k
  local fx, fy = math.cos(a), math.sin(a)
  local L = f.L
  return {
    { rx - fx * wid * 0.35 * L, ry - fy * wid * 0.35 * L },
    { rx + nx * len * 0.45 * L - fx * wid * 0.5 * L, ry + ny * len * 0.45 * L - fy * wid * 0.5 * L },
    { rx + nx * len * L + fx * back * L, ry + ny * len * L + fy * back * L },
    { rx + nx * len * 0.5 * L + fx * (back + wid) * L, ry + ny * len * 0.5 * L + fy * (back + wid) * L },
    { rx + fx * wid * 0.55 * L, ry + fy * wid * 0.55 * L },
  }
end
-- The tail: two lobes from the tail root, lagging the wave.
local function tail(f, k)
  local i = N
  local a = f.pa[i]
  local lag = math.sin(f.phase - 5.2 - 0.9) * 0.35
  local rx, ry = f.px[i], f.py[i]
  local L = f.L
  local len = 0.36 * L
  local ang = a + lag * 1.3 + k * 0.4
  local sx, sy = -math.sin(a) * k, math.cos(a) * k
  return {
    { rx + sx * L * 0.03, ry + sy * L * 0.03 },
    { rx + math.cos(a + lag) * len * 0.35 + sx * L * 0.02, ry + math.sin(a + lag) * len * 0.35 + sy * L * 0.02 },
    { rx + math.cos(ang) * len, ry + math.sin(ang) * len },
    { rx + math.cos(ang + k * 0.5) * len * 0.75, ry + math.sin(ang + k * 0.5) * len * 0.75 },
    { rx - sx * L * 0.01, ry - sy * L * 0.01 },
  }
end

-- The body as one outline: down one side, back up the other, and a
-- rounded snout ahead of the head — one polygon, so a translucent body
-- has no seams.
local function bodyPoly(f, k, ox, oy)
  local pts = {}
  for i = 1, N do
    local x, y = at(f, i, k)
    pts[#pts + 1] = { x + ox, y + oy }
  end
  for i = N, 1, -1 do
    local x, y = at(f, i, -k)
    pts[#pts + 1] = { x + ox, y + oy }
  end
  local hx, hy, r = f.px[1], f.py[1], halfWidth(0) * f.L * k
  local a = f.pa[1]
  for s = 1, 7 do
    local ang = a - math.pi / 2 - s / 8 * math.pi
    pts[#pts + 1] = { hx + math.cos(ang) * r + ox, hy + math.sin(ang) * r + oy }
  end
  return pts
end

local function shifted(pts, ox, oy)
  local out = {}
  for i, p in pts do out[i] = { p[1] + ox, p[2] + oy } end
  return out
end

local function drawFish(c, f, ink, body, finCol, edge, red, redCore, shadow)
  local deep = 1 - f.depth
  local alpha = 0.62 + 0.38 * f.depth
  -- The shadow on the floor: the silhouette, soft-ish, offset with depth.
  if c.face == "paper" then
    local ox, oy = 8 + deep * 26, 14 + deep * 34
    for pass = 1, 3 do
      local spread = (pass - 1) * 2.5
      local a = 0.05 / pass
      c:poly(bodyPoly(f, 1, ox + spread, oy + spread), shadow, a * alpha)
      c:blob(shifted(tail(f, 1), ox, oy), shadow, a * alpha)
      c:blob(shifted(tail(f, -1), ox, oy), shadow, a * alpha)
    end
  end
  -- Fins and tail, translucent, under the body.
  c:blob(tail(f, 1), finCol, 0.72 * alpha)
  c:blob(tail(f, -1), finCol, 0.72 * alpha)
  c:blob(fin(f, 6, 1, 0.13, 0.06, 0.05), finCol, 0.72 * alpha)
  c:blob(fin(f, 6, -1, 0.13, 0.06, 0.05), finCol, 0.72 * alpha)
  c:blob(fin(f, 15, 1, 0.075, 0.04, 0.035), finCol, 0.72 * alpha)
  c:blob(fin(f, 15, -1, 0.075, 0.04, 0.035), finCol, 0.72 * alpha)
  -- The body, then a lighter strip along the spine for roundness.
  c:poly(bodyPoly(f, 1, 0, 0), body, alpha)
  c:poly(bodyPoly(f, 0.55, 0, 0), "#ffffff", (if c.face == "paper" then 0.55 else 0.06) * alpha)
  -- The patches, following the bend.
  for _, mk in f.marks do
    local pts = {}
    for q, p in mk.pts do
      local x, y = atUV(f, clamp(p[1], 0, 1), clamp(p[2], -1, 1))
      pts[q] = { x, y }
    end
    c:blob(pts, if mk.red then red else ink, alpha)
    if mk.red then
      -- A warmer core.
      local core = {}
      for q, p in mk.pts do
        local x, y = atUV(f, clamp(mk.pts[1][1] * 0.3 + p[1] * 0.7, 0, 1), clamp(p[2] * 0.6, -1, 1))
        core[q] = { x, y }
      end
      c:blob(core, redCore, 0.35 * alpha)
    end
  end
  -- The edges as hairlines, and the dorsal line.
  local left, right, mid = {}, {}, {}
  for i = 1, N do
    local lx, ly = at(f, i, 1)
    local rx, ry = at(f, i, -1)
    left[i] = { lx, ly }; right[i] = { rx, ry }
    if i >= 8 and i <= N - 2 then mid[#mid + 1] = { at(f, i, 0) } end
  end
  c:curve(left, 1, edge, 0.9 * alpha)
  c:curve(right, 1, edge, 0.9 * alpha)
  c:curve(mid, 1.2, edge, 0.35 * alpha)
  -- Eyes.
  local e1x, e1y = at(f, 2, 0.62)
  local e2x, e2y = at(f, 2, -0.62)
  c:circle(e1x, e1y, math.max(1.4, f.L * 0.011), ink, alpha)
  c:circle(e2x, e2y, math.max(1.4, f.L * 0.011), ink, alpha)
end

function draw(c)
  if not seeded or W ~= c.w or H ~= c.h then
    W, H = c.w, c.h
    -- The water above the line and below the rows sets the fish's size.
    local l = c.prompt
    local strip = math.max(l.y - 36 - 24, H - 56 - (l.y + l.h + 16 + c.rows))
    Lmax = math.max(110, strip * 0.8)
    if not seeded then
      seeded = true
      for i = 1, 4 do fish[i] = makeFish(i) end
      table.sort(fish, function(a, b) return a.depth < b.depth end)
      for _, f in fish do spine(f) end
    end
  end
  -- Inputs: typing gathers, the pointer is shied from, taps ring.
  local typed = c.typed
  gather = if #typed > 0 then math.min(1, 0.4 + #typed * 0.08) else 0
  pointer = c.pointer
  for _, p in c:taps() do ripples[#ripples + 1] = { x = p.x, y = p.y, t = c.t, k = 0.8 } end
  step(c, c.dt)

  local paper = c.face == "paper"
  local ink = c.ink
  local body = if paper then "#fffdf9" else "#e9e5da"
  local finCol = if paper then "#fffdf9" else "#e9e5da"
  local edge = if paper then "#141413" else "#000000"
  local red = if paper then "#cf3a2a" else "#c8102e"
  local redCore = if paper then "#e2604a" else "#e0432f"
  for _, f in fish do drawFish(c, f, ink, body, finCol, edge, red, redCore, "#141413") end
  -- Rings.
  for _, r in ripples do
    local age = c.t - r.t
    for n = 0, 2 do
      local a = age - n * 0.35
      if a >= 0 then
        local rad = 14 + a * 90 * r.k
        local al = math.max(0, 0.5 - a * 0.18) * r.k
        local pts = {}
        for k = 0, 35 do
          local th = k / 36 * TAU
          pts[#pts + 1] = { r.x + math.cos(th) * rad, r.y + math.sin(th) * rad * 0.45 }
        end
        pts[#pts + 1] = pts[1]
        c:curve(pts, 1, ink, al * 0.4)
      end
    end
  end
end

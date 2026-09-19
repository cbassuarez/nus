-- name: the brain
-- says: what the kernel is doing, as a living process tree in dim type

-- The brain: every process on this machine as a row — name, pid, its
-- share of the processor, its memory — laid out as three trees in dim
-- type: the system down the left, the desktop down the right, nus's own
-- work in the middle, above the line and below the rows. Hairlines run
-- from parent to child; a process that wakes sends a pulse down its
-- connector and flares; the kernel's counters tick along the top. Hover a
-- row and its ancestry lights; type and the matching names come forward.

local function clamp(v, a, b) return if v < a then a elseif v > b then b else v end
local function fmt(n)
  n = math.floor(n + 0.5)
  local s = tostring(n)
  local out = ""
  while #s > 3 do out = " " .. string.sub(s, -3) .. out; s = string.sub(s, 1, -4) end
  return s .. out
end

local seen = {}        -- pid → { alpha, flare, cpu }
local pulses = {}
local lastCpu = {}
local rowH, indent = 22, 10
local counters = { ctx = 0, sys = 0 }

local function build(list)
  -- Parents, children, roots.
  local byPid, kids = {}, {}
  for _, p in list do byPid[p.pid] = p; kids[p.pid] = kids[p.pid] or {} end
  for _, p in list do
    if p.ppid ~= 0 and byPid[p.ppid] and p.ppid ~= p.pid then
      local k = kids[p.ppid]; k[#k + 1] = p
    end
  end
  for _, k in kids do table.sort(k, function(a, b) return a.pid < b.pid end) end
  local function flatten(p, depth, out, col, guard)
    if guard > 400 then return end
    out[#out + 1] = { p = p, depth = depth, col = col }
    for _, k in kids[p.pid] or {} do flatten(k, depth + 1, out, col, guard + 1) end
  end
  -- Which tree is which: nus's own (composite) in the middle, the desktop
  -- (explorer) on the right, everything else on the left.
  local mine, desk, rest = {}, {}, {}
  local placed = {}
  local function isMine(name) return name == "composite" or name == "nus" end
  for _, p in list do
    if isMine(p.name) and (not byPid[p.ppid] or not isMine(byPid[p.ppid].name)) then flatten(p, 0, mine, 1, 0) end
  end
  for _, r in mine do placed[r.p.pid] = true end
  for _, p in list do
    if p.name == "explorer" and not placed[p.pid] then flatten(p, 0, desk, 2, 0) end
  end
  for _, r in desk do placed[r.p.pid] = true end
  -- The rest, by their roots.
  local roots = {}
  for _, p in list do
    if not placed[p.pid] and (p.ppid == 0 or not byPid[p.ppid] or placed[p.ppid]) then roots[#roots + 1] = p end
  end
  table.sort(roots, function(a, b) return a.pid < b.pid end)
  for _, r in roots do
    if not placed[r.pid] then
      local tmp = {}
      flatten(r, 0, tmp, 0, 0)
      for _, row in tmp do if not placed[row.p.pid] then rest[#rest + 1] = row; placed[row.p.pid] = true end end
    end
  end
  return rest, mine, desk
end

function draw(c)
  local W, H = c.w, c.h
  local paper = c.face == "paper"
  local text = c.ink
  local signal = if paper then "#c8102e" else "#e8604a"
  local line = c.prompt
  local procs = c:processes()
  c:text(W - 36, 56, "kernel · " .. (if procs.ready then fmt(procs.ctx) .. " ctx/s · " .. fmt(procs.syscalls) .. " syscalls/s · " .. fmt(procs.threads) .. " threads · " .. fmt(procs.handles) .. " handles" else "listening…"), 10.5, text, 0.38, { align = "right" })
  if not procs.ready then return end
  local left, mine, desk = build(procs.list)
  -- Layout: the columns, the middle one above the line then below its rows.
  local cols = {
    { x = 36, w = line.x - 36 - 30, y0 = 78, y1 = H - 50 },
    { x = line.x + 10, w = line.w - 20, y0 = 78, y1 = line.y - 40, more0 = line.y + line.h + 20 + c.rows + 10, more1 = H - 50 },
    { x = line.x + line.w + 30, w = W - (line.x + line.w + 30) - 36, y0 = 78, y1 = H - 50 },
  }
  local zoneTop, zoneBot = line.y - 40, line.y + line.h + 20 + c.rows + 4
  local placedRows = {}
  local function lay(rows, ci)
    local col = cols[ci]
    local y, stage = col.y0, 0
    for _, r in rows do
      if ci == 2 and stage == 0 and y + rowH > col.y1 then stage = 1; y = col.more0 end
      if ci == 2 and stage == 1 and y + rowH > col.more1 then break end
      if ci ~= 2 and y + rowH > col.y1 then break end
      r.x, r.y, r.w = col.x, y, col.w
      placedRows[#placedRows + 1] = r
      y += rowH
    end
  end
  lay(left, 1); lay(mine, 2); lay(desk, 3)
  local byPid = {}
  for _, r in placedRows do byPid[r.p.pid] = r end
  -- The pointer: the row under it and its line back to the root.
  local lit = {}
  if c.pointer then
    for _, r in placedRows do
      if c.pointer.x >= r.x - 6 and c.pointer.x < r.x + r.w and c.pointer.y >= r.y - 6 and c.pointer.y < r.y + rowH - 6 then
        local q = r
        while q do lit[q.p.pid] = true; q = byPid[q.p.ppid] end
        break
      end
    end
  end
  local typed = string.lower(c.typed)
  -- Connectors, breaking around the line's box.
  for _, r in placedRows do
    local par = byPid[r.p.ppid]
    if par and par ~= r then
      local px = par.x + par.depth * indent + 4
      local cxx = r.x + r.depth * indent - 6
      local on = lit[r.p.pid] and lit[par.p.pid]
      local a = if on then 0.55 else 0.13
      local y0, y1 = par.y + 4, r.y - 1
      if y0 < zoneTop and y1 > zoneBot then
        c:line(px, y0, px, zoneTop - 6, 1, text, a)
        c:line(px, zoneBot + 6, px, y1, 1, text, a)
      else
        c:line(px, y0, px, y1, 1, text, a)
      end
      c:line(px, y1, cxx, y1, 1, text, a)
    end
  end
  -- The rows.
  for _, r in placedRows do
    local p = r.p
    local s = seen[p.pid]
    if not s then s = { alpha = 0.15, flare = -10, cpu = p.cpu }; seen[p.pid] = s end
    if p.cpu - s.cpu > 6 then
      s.flare = c.t
      local par = byPid[p.ppid]
      if par then pulses[#pulses + 1] = { from = par, to = r, t = c.t } end
    end
    s.cpu = p.cpu
    local a = 0.15 + clamp(p.cpu / 40, 0, 1) * 0.45
    local flare = clamp(1 - (c.t - s.flare) / 0.9, 0, 1)
    a = math.max(a, flare * 0.95)
    if lit[p.pid] then a = 0.9 end
    if #typed > 0 and string.sub(string.lower(p.name), 1, #typed) == typed then a = 0.95 end
    s.alpha += (a - s.alpha) * math.min(1, c.dt * 6)
    local x, base = r.x + r.depth * indent, r.y + 13
    local cpu = if p.cpu >= 9.95 then string.format("%d", p.cpu) else string.format("%.1f", p.cpu)
    local mem = if p.mem >= 1000 then string.format("%.1fG", p.mem / 1000) else string.format("%dM", p.mem)
    local figures = string.format("%d %s%% %s", p.pid, cpu, mem)
    -- The name, cut to what fits beside the figures.
    local room = r.x + r.w - c:measure(figures, 10.5) - 10 - x
    local name = p.name
    local fits = math.floor(room / c:measure("m", 10.5))
    if fits < #name then name = if fits > 2 then string.sub(name, 1, fits - 1) .. "…" else "" end
    c:text(x, base, name, 10.5, text, s.alpha)
    c:text(r.x + r.w, base, figures, 10.5, text, s.alpha * 0.7, { align = "right" })
    local bar = clamp(p.cpu / 100, 0, 1) * (r.w - r.depth * indent - 4)
    if p.cpu > 20 then
      c:rect(x, r.y + 17, math.max(2, bar), 1.5, signal, 0.4 + flare * 0.5)
    else
      c:rect(x, r.y + 17, math.max(2, bar), 1.5, text, 0.1 + s.alpha * 0.3)
    end
  end
  -- Pulses: a bright dash running the connector from parent to child.
  for i = #pulses, 1, -1 do
    local pu = pulses[i]
    local age = c.t - pu.t
    local from, to = byPid[pu.from.p.pid], byPid[pu.to.p.pid]
    if age > 0.7 or not from or not to then
      table.remove(pulses, i)
    else
      local sx, sy = from.x + from.depth * indent + 4, from.y + 4
      local ex, ey = to.x + to.depth * indent - 6, to.y - 1
      local v, h = ey - sy, ex - sx
      local len = math.abs(v) + math.abs(h)
      local d = clamp(age / 0.5, 0, 1) * len
      local hx, hy
      if d <= math.abs(v) then hx, hy = sx, sy + (if v < 0 then -d else d) else hx, hy = sx + (if h < 0 then -(d - math.abs(v)) else d - math.abs(v)), ey end
      if not (hy > zoneTop - 6 and hy < zoneBot + 6 and hx > line.x - 30 and hx < line.x + line.w + 30) then
        c:rect(hx - 1.5, hy - 1.5, 3, 3, signal, 0.9 * clamp(1.2 - age * 1.5, 0, 1))
      end
    end
  end
end

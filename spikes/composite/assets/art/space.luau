-- name: space
-- says: the sky over this machine, now — an all-sky chart that turns with the hour

-- Space: looking straight up. Zenith at the centre, the horizon a hairline
-- circle, north at the top, east on the left as it is when you lie back.
-- The brightest northern stars from a small catalogue by magnitude, the red
-- giants tinted, the classic figures in hairlines, the names in small
-- caps, a few hundred faint ones for texture. Sidereal time from the clock
-- and the place, so the Dipper is where it is; SPEED turns the sky faster
-- than the hour does (1 = real).

local SPEED = 30
local TAU = math.pi * 2
local D = math.pi / 180
local function rnd(a, b) return a + math.random() * (b - a) end
local function clamp(v, a, b) return if v < a then a elseif v > b then b else v end

-- name, RA (hours), Dec (degrees), magnitude, warm (a red giant)
local CAT = {
  {"Dubhe", 11.06, 61.75, 1.8}, {"Merak", 11.03, 56.38, 2.4}, {"Phecda", 11.90, 53.69, 2.4}, {"Megrez", 12.26, 57.03, 3.3}, {"Alioth", 12.90, 55.96, 1.8}, {"Mizar", 13.40, 54.93, 2.2}, {"Alkaid", 13.79, 49.31, 1.9},
  {"Polaris", 2.53, 89.26, 2.0}, {"Kochab", 14.85, 74.16, 2.1}, {"Pherkad", 15.35, 71.83, 3.0}, {"Yildun", 17.54, 86.59, 4.4}, {"ε UMi", 16.77, 82.04, 4.2}, {"ζ UMi", 15.73, 77.79, 4.3}, {"η UMi", 16.29, 75.76, 5.0},
  {"Caph", 0.15, 59.15, 2.3}, {"Schedar", 0.68, 56.54, 2.2}, {"γ Cas", 0.95, 60.72, 2.5}, {"Ruchbah", 1.43, 60.24, 2.7}, {"Segin", 1.91, 63.67, 3.4},
  {"Alderamin", 21.31, 62.59, 2.5}, {"Alfirk", 21.48, 70.56, 3.2}, {"Errai", 23.66, 77.63, 3.2}, {"ζ Cep", 22.18, 58.20, 3.4}, {"ι Cep", 22.83, 66.20, 3.5},
  {"Thuban", 14.07, 64.38, 3.7}, {"Eltanin", 17.94, 51.49, 2.2}, {"Rastaban", 17.51, 52.30, 2.8}, {"Edasich", 15.42, 58.97, 3.3}, {"Aldhibain", 16.40, 61.51, 2.7}, {"Altais", 19.21, 67.66, 3.1}, {"Grumium", 17.89, 56.87, 3.7}, {"κ Dra", 12.56, 69.79, 3.9},
  {"Vega", 18.62, 38.78, 0.03}, {"Sheliak", 18.83, 33.36, 3.5}, {"Sulafat", 18.98, 32.69, 3.2}, {"ζ Lyr", 18.75, 37.60, 4.4}, {"δ Lyr", 18.90, 36.90, 4.3},
  {"Deneb", 20.69, 45.28, 1.25}, {"Sadr", 20.37, 40.26, 2.2}, {"Albireo", 19.51, 27.96, 3.1}, {"Gienah", 20.77, 33.97, 2.5}, {"δ Cyg", 19.75, 45.13, 2.9}, {"ι Cyg", 19.50, 51.73, 3.8},
  {"Altair", 19.85, 8.87, 0.77}, {"Tarazed", 19.77, 10.61, 2.7}, {"Alshain", 19.92, 6.41, 3.7},
  {"Arcturus", 14.26, 19.18, -0.05, true}, {"Izar", 14.75, 27.07, 2.4}, {"Muphrid", 13.91, 18.40, 2.7}, {"Seginus", 14.53, 38.31, 3.0}, {"Nekkar", 15.03, 40.39, 3.5}, {"δ Boo", 15.26, 33.31, 3.5}, {"ρ Boo", 14.53, 30.37, 3.6},
  {"Alphecca", 15.58, 26.71, 2.2}, {"Nusakan", 15.46, 29.11, 3.7}, {"θ CrB", 15.55, 31.36, 4.1}, {"γ CrB", 15.71, 26.30, 3.8}, {"δ CrB", 15.83, 26.07, 4.6}, {"ε CrB", 15.96, 26.88, 4.1},
  {"ζ Her", 16.69, 31.60, 2.8}, {"η Her", 16.72, 38.92, 3.5}, {"π Her", 17.25, 36.81, 3.2}, {"ε Her", 17.00, 30.93, 3.9},
  {"Mirfak", 3.41, 49.86, 1.8}, {"Algol", 3.14, 40.96, 2.1}, {"δ Per", 3.72, 47.79, 3.0}, {"ε Per", 3.96, 40.01, 2.9}, {"γ Per", 3.08, 53.51, 2.9}, {"η Per", 2.84, 55.90, 3.8},
  {"Capella", 5.28, 46.00, 0.08}, {"Menkalinan", 6.00, 44.95, 1.9}, {"Mahasim", 5.99, 37.21, 2.6}, {"Hassaleh", 4.95, 33.17, 2.7}, {"Almaaz", 5.03, 43.82, 3.0}, {"Elnath", 5.44, 28.61, 1.65},
  {"Castor", 7.58, 31.89, 1.6}, {"Pollux", 7.76, 28.03, 1.1, true}, {"Alhena", 6.63, 16.40, 1.9}, {"Mebsuta", 6.73, 25.13, 3.0}, {"Tejat", 6.38, 22.51, 2.9},
  {"Regulus", 10.14, 11.97, 1.4}, {"Denebola", 11.82, 14.57, 2.1}, {"Algieba", 10.33, 19.84, 2.0}, {"Zosma", 11.24, 20.52, 2.6}, {"Chertan", 11.24, 15.43, 3.3}, {"Adhafera", 10.28, 23.42, 3.4}, {"Rasalas", 9.88, 26.01, 3.9}, {"Algenubi", 9.76, 23.77, 3.0}, {"η Leo", 10.12, 16.76, 3.5},
  {"Aldebaran", 4.60, 16.51, 0.85, true}, {"Alcyone", 3.79, 24.11, 2.9}, {"Atlas", 3.82, 24.05, 3.6}, {"Electra", 3.75, 24.11, 3.7}, {"Maia", 3.76, 24.37, 3.9}, {"Merope", 3.77, 23.95, 4.2}, {"Taygeta", 3.75, 24.47, 4.3},
  {"Betelgeuse", 5.92, 7.41, 0.5, true}, {"Rigel", 5.24, -8.20, 0.13}, {"Bellatrix", 5.42, 6.35, 1.6}, {"Saiph", 5.80, -9.67, 2.1}, {"Alnitak", 5.68, -1.94, 1.8}, {"Alnilam", 5.60, -1.20, 1.7}, {"Mintaka", 5.53, -0.30, 2.2},
  {"Sirius", 6.75, -16.72, -1.46}, {"Procyon", 7.66, 5.22, 0.34}, {"Spica", 13.42, -11.16, 1.0}, {"Rasalhague", 17.58, 12.56, 2.1},
  {"Markab", 23.08, 15.21, 2.5}, {"Scheat", 23.06, 28.08, 2.4}, {"Algenib", 0.22, 15.18, 2.8}, {"Alpheratz", 0.14, 29.09, 2.1}, {"Mirach", 1.16, 35.62, 2.1, true}, {"Almach", 2.06, 42.33, 2.1},
}
local byName = {}
for i, s in CAT do byName[s[1]] = i end
local function chain(names)
  local out = {}
  for i = 1, #names - 1 do out[#out + 1] = { byName[names[i]], byName[names[i + 1]] } end
  return out
end
local function join(a, b) for _, x in b do a[#a + 1] = x end return a end
local FIG = {
  { name = "Ursa Major", lines = chain({"Alkaid", "Mizar", "Alioth", "Megrez", "Phecda", "Merak", "Dubhe", "Megrez"}), at = "Megrez" },
  { name = "Ursa Minor", lines = chain({"Polaris", "Yildun", "ε UMi", "ζ UMi", "Kochab", "Pherkad", "η UMi", "ζ UMi"}), at = "ζ UMi" },
  { name = "Cassiopeia", lines = chain({"Caph", "Schedar", "γ Cas", "Ruchbah", "Segin"}), at = "γ Cas" },
  { name = "Cepheus", lines = chain({"Alderamin", "Alfirk", "Errai", "ι Cep", "ζ Cep", "Alderamin"}), at = "ι Cep" },
  { name = "Draco", lines = join(chain({"Rastaban", "Eltanin", "Grumium", "Rastaban"}), chain({"Grumium", "Altais", "Aldhibain", "Edasich", "Thuban", "κ Dra"})), at = "Aldhibain" },
  { name = "Lyra", lines = chain({"Vega", "ζ Lyr", "Sheliak", "Sulafat", "δ Lyr", "ζ Lyr"}), at = "Sheliak" },
  { name = "Cygnus", lines = join(chain({"Deneb", "Sadr", "Albireo"}), chain({"Gienah", "Sadr", "δ Cyg", "ι Cyg"})), at = "Sadr" },
  { name = "Aquila", lines = chain({"Tarazed", "Altair", "Alshain"}), at = "Altair" },
  { name = "Boötes", lines = chain({"Arcturus", "Izar", "δ Boo", "Nekkar", "Seginus", "ρ Boo", "Arcturus", "Muphrid"}), at = "ρ Boo" },
  { name = "Corona Borealis", lines = chain({"θ CrB", "Nusakan", "Alphecca", "γ CrB", "δ CrB", "ε CrB"}), at = "Alphecca" },
  { name = "Hercules", lines = chain({"ζ Her", "η Her", "π Her", "ε Her", "ζ Her"}), at = "π Her" },
  { name = "Perseus", lines = join(chain({"η Per", "γ Per", "Mirfak", "δ Per", "ε Per"}), chain({"Mirfak", "Algol"})), at = "Mirfak" },
  { name = "Auriga", lines = chain({"Capella", "Menkalinan", "Mahasim", "Elnath", "Hassaleh", "Almaaz", "Capella"}), at = "Almaaz" },
  { name = "Gemini", lines = join(chain({"Castor", "Mebsuta", "Tejat"}), chain({"Pollux", "Alhena"})), at = "Mebsuta" },
  { name = "Leo", lines = join(chain({"Regulus", "η Leo", "Algieba", "Adhafera", "Rasalas", "Algenubi"}), chain({"Regulus", "Chertan", "Denebola", "Zosma", "Algieba"})), at = "Chertan" },
  { name = "Taurus", lines = chain({"Aldebaran", "Elnath"}), at = "Aldebaran" },
  { name = "Orion", lines = join(chain({"Betelgeuse", "Bellatrix", "Mintaka", "Alnilam", "Alnitak", "Saiph", "Rigel", "Mintaka"}), chain({"Betelgeuse", "Alnitak"})), at = "Alnilam" },
  { name = "Pegasus", lines = chain({"Markab", "Scheat", "Alpheratz", "Algenib", "Markab"}), at = "Markab" },
  { name = "Andromeda", lines = chain({"Alpheratz", "Mirach", "Almach"}), at = "Mirach" },
}

-- The faint field, fixed on the sky.
local faint = {}
for i = 1, 420 do faint[i] = { rnd(0, 24), math.asin(rnd(math.sin(-35 * D), 1)) / D, rnd(4.6, 6.0), rnd(0, TAU) } end
local streaks = {}
local started = nil
local pos = {}

-- Sidereal time in hours at a longitude (east positive) for unix ms.
local function lst(ms, lon)
  local d = ms / 86400000 - 10957.5
  local gmst = 18.697374558 + 24.06570982441908 * d
  return (gmst + lon / 15) % 24
end

function draw(c)
  local W, H = c.w, c.h
  local cx, cy = W / 2, H / 2
  local K = (H / 2 - 4) / 90
  local paper = c.face == "paper"
  local star = if paper then "#141413" else "#f4f1ea"
  local warm = if paper then "#c8102e" else "#e8604a"
  local place = c:place()
  local lat, lon = place.lat, place.lon
  local sinLat, cosLat = math.sin(lat * D), math.cos(lat * D)
  local nowMs = c:now()
  if not started then started = nowMs end
  local now = started + (nowMs - started) * SPEED
  local LST = lst(now, lon)
  local typed = string.lower(c.typed)
  local R = 90 * K

  local function at(ra, dec)
    local Hh = (LST - ra) * 15 * D
    local dd = dec * D
    local sa = sinLat * math.sin(dd) + cosLat * math.cos(dd) * math.cos(Hh)
    local alt = math.asin(clamp(sa, -1, 1))
    local az = math.atan2(-math.cos(dd) * math.sin(Hh), math.sin(dd) * cosLat - math.cos(dd) * sinLat * math.cos(Hh))
    local r = (90 - alt / D) * K
    return cx - r * math.sin(az), cy - r * math.cos(az), alt / D
  end
  local function inLine(x, y, pad)
    local l = c.prompt
    return x > l.x - pad and x < l.x + l.w + pad and y > l.y - 30 - pad and y < l.y + l.h + c.rows + pad
  end
  -- How clear of the line a point is: 0 on it, 1 a margin away.
  local function clear(x, y)
    local l = c.prompt
    local x0, x1, y0, y1 = l.x - 16, l.x + l.w + 16, l.y - 40, l.y + l.h + c.rows + 8
    local dx = math.max(x0 - x, 0, x - x1)
    local dy = math.max(y0 - y, 0, y - y1)
    return clamp(math.sqrt(dx * dx + dy * dy) / 36, 0, 1)
  end
  -- A hairline in pieces, each fading as it nears the line.
  local function hair(x1, y1, x2, y2, w, col, alpha)
    local n = 6
    for k = 0, n - 1 do
      local t0, t1 = k / n, (k + 1) / n
      local ax, ay = x1 + (x2 - x1) * t0, y1 + (y2 - y1) * t0
      local bx, by = x1 + (x2 - x1) * t1, y1 + (y2 - y1) * t1
      local a = alpha * clear((ax + bx) / 2, (ay + by) / 2)
      if a > 0.01 then c:line(ax, ay, bx, by, w, col, a) end
    end
  end
  local function ring(rad, alpha, dotted)
    local pts = {}
    local n = 96
    for k = 0, n do
      local th = k / n * TAU
      pts[#pts + 1] = { cx + math.cos(th) * rad, cy + math.sin(th) * rad }
    end
    if dotted then
      for k = 1, #pts - 1, 3 do hair(pts[k][1], pts[k][2], pts[k + 1][1], pts[k + 1][2], 1, star, alpha) end
    else
      for k = 1, #pts - 1 do hair(pts[k][1], pts[k][2], pts[k + 1][1], pts[k + 1][2], 1, star, alpha) end
    end
  end

  -- The horizon, two altitude rings, the cardinal points.
  ring(R, 0.1, false)
  ring(60 * K, 0.1, true)
  ring(30 * K, 0.1, true)
  local dim = c.dim
  for _, m in { { cx, cy - R - 8, "N" }, { cx, cy + R + 16, "S" }, { cx - R - 12, cy + 4, "E" }, { cx + R + 12, cy + 4, "W" } } do
    local a = 0.9 * clear(m[1], m[2])
    if a > 0.01 then c:text(m[1], m[2], m[3], 10, dim, a, { align = "center" }) end
  end

  -- The faint field.
  for i, f in faint do
    local x, y, alt = at(f[1], f[2])
    if alt > 0 then
      local a = (0.12 + 0.12 * (6 - f[3])) * (0.85 + 0.15 * math.sin(c.t * 1.1 + f[4])) * clamp(alt / 8, 0, 1) * clear(x, y)
      if a > 0.01 then c:circle(x, y, 0.7, star, a) end
    end
  end
  -- The catalogue, placed.
  for i, s in CAT do
    local x, y, alt = at(s[2], s[3])
    pos[i] = { x, y, alt }
  end
  -- The figures, under the stars.
  for _, fig in FIG do
    local lit = #typed > 0 and string.sub(string.lower(fig.name), 1, #typed) == typed
    for _, ln in fig.lines do
      local a, b = pos[ln[1]], pos[ln[2]]
      if a and b and (a[3] >= -2 or b[3] >= -2) then
        hair(a[1], a[2], b[1], b[2], 1, star, if lit then 0.6 else 0.2)
      end
    end
  end
  local nearest, nd = nil, 1e9
  for i, s in CAT do
    local p = pos[i]
    if p[3] >= -1 then
      local mag = s[4]
      local rad = clamp(3.9 - mag * 0.72, 0.8, 4.2)
      local horizon = clamp((p[3] + 1) / 6, 0, 1)
      local tw = 0.86 + 0.14 * math.sin(c.t * (0.9 + (i % 5) * 0.2) + i)
      local alpha = clamp(0.95 - mag * 0.13, 0.3, 0.95) * tw * horizon
      local lit = #typed > 0 and string.sub(string.lower(s[1]), 1, #typed) == typed
      if lit then alpha = 1 end
      alpha *= clear(p[1], p[2])
      local col = if s[5] then warm else star
      if alpha > 0.01 then c:circle(p[1], p[2], rad, col, alpha) end
      if (mag < 1.2 or lit) and alpha > 0.01 then
        local L = rad * 3.4
        c:line(p[1] - L, p[2], p[1] + L, p[2], 0.7, col, alpha * 0.4)
        c:line(p[1], p[2] - L, p[1], p[2] + L, 0.7, col, alpha * 0.4)
      end
      if c.pointer and mag < 3.2 then
        local dx, dy = p[1] - c.pointer.x, p[2] - c.pointer.y
        local d2 = dx * dx + dy * dy
        if d2 < nd then nd = d2; nearest = i end
      end
    end
  end
  -- The names of the figures, in small caps, off the line.
  for _, fig in FIG do
    local p = pos[byName[fig.at]]
    if p and p[3] >= 6 and not inLine(p[1], p[2], 20) then
      local lit = #typed > 0 and string.sub(string.lower(fig.name), 1, #typed) == typed
      c:text(p[1], p[2] + 16, string.upper(fig.name), 9, c.ink, if lit then 0.9 else 0.36, { align = "center", caps = true })
    end
  end
  -- The nearest bright star names itself under the pointer.
  if nearest and nd < 60 * 60 then
    local s, p = CAT[nearest], pos[nearest]
    c:text(p[1] + 9, p[2] - 9, string.format("%s · %.1f", s[1], s[4]), 10, c.ink, 0.85)
  end
  -- Meteors: a tap makes one; one on its own every so often.
  for _, tp in c:taps() do streaks[#streaks + 1] = { x = tp.x, y = tp.y, a = rnd(0, TAU), t = c.t } end
  if math.random() < 0.0015 then
    local ang, rr = rnd(0, TAU), rnd(0.3, 0.8) * R
    streaks[#streaks + 1] = { x = cx + math.cos(ang) * rr, y = cy + math.sin(ang) * rr, a = rnd(0, TAU), t = c.t }
  end
  for i = #streaks, 1, -1 do
    local st = streaks[i]
    local age = c.t - st.t
    if age > 1 then
      table.remove(streaks, i)
    else
      local d, len = age * 560, 80
      local hx, hy = st.x + math.cos(st.a) * d, st.y + math.sin(st.a) * d
      local col = if paper then "#c8102e" else "#f4f1ea"
      for k = 0, 5 do
        local t0, t1 = k / 6, (k + 1) / 6
        c:line(hx - math.cos(st.a) * len * (1 - t0), hy - math.sin(st.a) * len * (1 - t0), hx - math.cos(st.a) * len * (1 - t1), hy - math.sin(st.a) * len * (1 - t1), 1.2, col, 0.85 * clamp(1 - age, 0, 1) * t1)
      end
    end
  end
  -- The chart's card: where and when.
  local lh = math.floor(LST)
  local lm = math.floor((LST - lh) * 60)
  local ns = if lat >= 0 then "N" else "S"
  local ew = if lon >= 0 then "E" else "W"
  c:text(32, H - 62, string.format("%.1f° %s · %.1f° %s", math.abs(lat), ns, math.abs(lon), ew), 10, c.ink, 0.45, { caps = true })
  c:text(32, H - 46, string.format("LST %dh %02dm%s", lh, lm, if SPEED ~= 1 then string.format(" · %d×", SPEED) else ""), 10, c.ink, 0.45, { caps = true })
end

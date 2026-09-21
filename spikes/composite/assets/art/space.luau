-- name: space
-- says: the sky over this machine, now — an all-sky chart that turns with the hour

-- Space: looking straight up. Zenith at the centre, the horizon a hairline
-- circle, north at the top, east on the left as it is when you lie back.
-- Bright northern and southern stars from a small catalogue by magnitude, the red
-- giants tinted, the classic figures in hairlines, the names in small
-- caps, a deterministic synthetic faint field for texture. Sidereal time from the clock
-- and the place, so the Dipper is where it is; SPEED turns the sky faster
-- than the hour does (1 = real).

local SPEED = 1
local TAU = math.pi * 2
local D = math.pi / 180
local rng = 72341
local function rnd(a, b)
  rng = (rng * 48271) % 2147483647
  return a + rng / 2147483647 * (b - a)
end
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
  -- Southern additions: IAU archived J2000 catalogue (2021), V <= 3.5.
  {"Imai", 12.252421, -58.748927, 2.75},
  {"Paikauhale", 16.598042, -28.216017, 2.82},
  {"Toliman", 14.659740, -60.837528, 1.35},
  {"Azmidi", 7.821571, -24.859786, 3.45},
  {"Heze", 13.578220, -0.595820, 3.38},
  {"Kraz", 12.573121, -23.396759, 2.65},
  {"Acamar", 2.971021, -40.304672, 2.88},
  {"Achernar", 1.628568, -57.236753, 0.45},
  {"Acrab", 16.090620, -19.805453, 2.56},
  {"Acrux", 12.443304, -63.099093, 1.33},
  {"Adhara", 6.977097, -28.972086, 1.50},
  {"Albaldah", 19.162731, -21.023615, 2.88},
  {"Aldhanab", 21.898813, -37.364855, 3.00},
  {"Algorab", 12.497738, -16.515431, 2.94},
  {"Alnair", 22.137218, -46.960974, 1.73},
  {"Alnasl", 18.096802, -30.424100, 2.98},
  {"Alniyat", 16.353143, -25.592792, 2.90},
  {"Alphard", 9.459790, -8.658602, 1.99, true},
  {"Alsephina", 8.745063, -54.708819, 1.99},
  {"Aludra", 7.401584, -29.303106, 2.45},
  {"Ankaa", 0.438063, -42.306084, 2.40},
  {"Antares", 16.490128, -26.432003, 1.06, true},
  {"Arneb", 5.545504, -17.822289, 2.58},
  {"Ascella", 19.043536, -29.880063, 2.60},
  {"Aspidiske", 9.284835, -59.275232, 2.21},
  {"Atria", 16.811082, -69.027712, 1.91},
  {"Avior", 8.375232, -59.509484, 1.86, true},
  {"Brachium", 15.067838, -25.281961, 3.25},
  {"Canopus", 6.399197, -52.695661, -0.62},
  {"Cursa", 5.130829, -5.086446, 2.78},
  {"Dabih", 20.350187, -14.781405, 3.05},
  {"Deneb Algedi", 21.784012, -16.127287, 2.85},
  {"Diphda", 0.726492, -17.986606, 2.04},
  {"Dschubba", 16.005557, -22.621710, 2.29},
  {"Fang", 15.980865, -26.114108, 2.89},
  {"Fomalhaut", 22.960846, -29.622237, 1.17},
  {"Furud", 6.338553, -30.063367, 3.02},
  {"Fuyue", 17.830967, -37.043305, 3.19},
  {"Gacrux", 12.519433, -57.113213, 1.59, true},
  {"Hadar", 14.063724, -60.373035, 0.61},
  {"Hatysa", 5.590551, -5.909901, 2.80},
  {"Kaus Australis", 18.402866, -34.384616, 1.79},
  {"Kaus Borealis", 18.466178, -25.421701, 2.82},
  {"Kaus Media", 18.349901, -29.828104, 2.72},
  {"Lesath", 17.512732, -37.295813, 2.70},
  {"Markeb", 9.368560, -55.010667, 2.47},
  {"Menkent", 14.111374, -36.369958, 2.06},
  {"Miaplacidus", 9.219994, -69.717208, 1.67},
  {"Mimosa", 12.795351, -59.688764, 1.25},
  {"Mirzam", 6.378329, -17.955919, 1.98},
  {"Naos", 8.059735, -40.003148, 2.21},
  {"Nihal", 5.470756, -20.759441, 2.81},
  {"Nunki", 18.921091, -26.296724, 2.05},
  {"Peacock", 20.427460, -56.735090, 1.94},
  {"Phact", 5.660817, -34.074110, 2.65},
  {"Porrima", 12.694345, -1.449373, 2.74},
  {"Rigil Kentaurus", 14.660138, -60.833975, -0.01},
  {"Sabik", 17.172969, -15.724907, 2.43},
  {"Sadalmelik", 22.096399, -0.319849, 2.95},
  {"Sadalsuud", 21.525981, -5.571176, 2.90},
  {"Sargas", 17.621981, -42.997824, 1.86},
  {"Shaula", 17.560144, -37.103824, 1.62},
  {"Skat", 22.910837, -15.820827, 3.27},
  {"Suhail", 9.133266, -43.432589, 2.23},
  {"Tiaki", 22.711125, -46.884576, 2.12},
  {"Tureis", 8.125736, -24.304324, 2.83},
  {"Unurgunite", 7.028652, -27.934830, 3.49},
  {"Wazn", 5.849331, -35.768310, 3.12},
  {"Wezen", 7.139857, -26.393200, 1.83},
  {"Xamidimura", 16.864509, -38.047380, 3.00},
  {"Yed Posterior", 16.305358, -4.692510, 3.23},
  {"Yed Prior", 16.239094, -3.694323, 2.73},
  {"Zaurak", 3.967157, -13.508516, 2.97},
  {"Zubenelgenubi", 14.847976, -16.041777, 2.75},
  {"Zubeneschamali", 15.283448, -9.382914, 2.61},
  {"Larawag", 16.836059, -34.293232, 2.29},
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
  { name = "Crux", lines = join(chain({"Acrux", "Gacrux"}), chain({"Mimosa", "Imai"})), at = "Gacrux" },
  { name = "Centaurus", lines = chain({"Rigil Kentaurus", "Hadar", "Menkent"}), at = "Hadar" },
  { name = "Scorpius", lines = chain({"Dschubba", "Antares", "Larawag", "Sargas", "Shaula", "Lesath"}), at = "Antares" },
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
for i = 1, 420 do faint[i] = { rnd(0, 24), math.asin(rnd(-1, 1)) / D, rnd(4.6, 6.0), rnd(0, TAU) } end
local streaks = {}
local started = nil
local lastFrame = nil
local pos = {}

-- Sidereal time in hours at a longitude (east positive) for unix ms.
local function lst(ms, lon)
  local d = ms / 86400000 - 10957.5
  local gmst = 18.697374558 + 24.06570982441908 * d
  return (gmst + lon / 15) % 24
end

function draw(c)
  -- Repeated fixed-time samples are reduced-motion frames. Do not leave
  -- a randomly spawned meteor hanging forever across the prompt.
  local animated = lastFrame ~= nil and c.t > lastFrame
  lastFrame = c.t
  if not animated then streaks = {} end
  local W, H = c.w, c.h
  local cx, cy = W / 2, H / 2
  local K = math.max(1, math.min(W, H) / 2 - 28) / 90
  c:backdrop("dark")
  c:rect(0, 0, W, H, "#080d19")
  local paper = false
  local star = "#e4edff"
  local warm = "#ffd1a6"
  local place = c:place()
  -- Without a place, show an illustrated northern star chart. These are
  -- drawing coordinates, never stored or exposed as the user's location.
  local lat, lon = 45, 0
  if place then lat, lon = place.lat, place.lon end
  local sinLat, cosLat = math.sin(lat * D), math.cos(lat * D)
  local nowMs = c:now()
  if not started then started = nowMs end
  local now = started + (nowMs - started) * SPEED
  local LST = if place then lst(now, lon) else 8 + c.t / 3600
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
  local dim = "#8294b6"
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
      local air = 1 / math.max(0.06, math.sin(math.max(0, p[3]) * D))
      local scintillation = 0.015 + 0.07 * clamp((air - 1) / 8, 0, 1)
      local tw = 0.96 + scintillation * math.sin(c.t * (2.1 + (i % 5) * 0.4) + i)
      -- Extinction and logarithmic magnitude compressed for a display.
      horizon *= math.exp(-0.10 * (air - 1))
      local alpha = clamp((10 ^ (-0.4 * mag)) ^ 0.32, 0.08, 0.98) * tw * horizon
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
      c:text(p[1], p[2] + 16, string.upper(fig.name), 9, star, if lit then 0.9 else 0.36, { align = "center", caps = true })
    end
  end
  -- The nearest bright star names itself under the pointer.
  if nearest and nd < 60 * 60 then
    local s, p = CAT[nearest], pos[nearest]
    c:text(p[1] + 9, p[2] - 9, string.format("%s · %.1f", s[1], s[4]), 10, star, 0.85)
  end
  -- Pointer input explores labels, never restarts or invents celestial events.
  -- The Moon uses the same dated ephemeris as the atmospheric sky.
  if place then
    local ephemeris = c:celestial(now, lat, lon)
    local moon = ephemeris.moon
    local mx, my, altitude = at(moon.ra, moon.dec)
    if altitude > 0 and clear(mx, my) > 0.1 then
      local radius = 7
      local light = 2 * ephemeris.illumination - 1
      local side = if ephemeris.waxing then 1 else -1
      c:circle(mx, my, radius + 4, "#d0ddff", 0.06 * ephemeris.illumination)
      c:circle(mx, my, radius, "#182239", 0.9)
      -- Orthographic illuminated sphere: a curved terminator, not a mask wipe.
      for iy = -7, 7 do
        local yy = iy / radius
        local edge = math.sqrt(math.max(0, 1 - yy * yy))
        for ix = -7, 7 do
          local xx = ix / radius
          if math.abs(xx) <= edge then
            local zz = math.sqrt(math.max(0, 1 - xx * xx - yy * yy))
            if xx * side * math.sqrt(math.max(0, 1 - light * light)) + zz * light > 0 then
              c:rect(mx + ix, my + iy, 1, 1, "#ececf1", clear(mx, my))
            end
          end
        end
      end
      c:text(mx + 14, my + 4, string.format("Moon · %d%%", math.floor(ephemeris.illumination * 100 + 0.5)), 10, star, 0.7 * clear(mx, my))
    end
  end
  -- The chart's card: where and when.
  if not place then
    c:text(32, H - 86, "Illustrated northern sky", 10, star, 0.45, { caps = true })
    return
  end
  local lh = math.floor(LST)
  local lm = math.floor((LST - lh) * 60)
  local ns = if lat >= 0 then "N" else "S"
  local ew = if lon >= 0 then "E" else "W"
  c:text(32, H - 62, string.format("%.1f° %s · %.1f° %s", math.abs(lat), ns, math.abs(lon), ew), 10, star, 0.45, { caps = true })
  c:text(32, H - 46, string.format("LST %dh %02dm%s", lh, lm, if SPEED ~= 1 then string.format(" · %d×", SPEED) else ""), 10, star, 0.45, { caps = true })
end

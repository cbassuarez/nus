-- name: the sky
-- says: the sky over this machine, now — the sun where it is, clouds on the wind

-- The sky: one call to the canvas's own weather. The sun's height and
-- bearing come from the clock and the place (the same arithmetic the star
-- chart uses), so the light is morning light in the morning, the dusk
-- goes warm, and at night there is a moon and stars. The clouds are a
-- noise field lit from the sun, drifting; the cover eases over the hours
-- so no two afternoons are the same. Tap the paper and the wind picks up.

local D = math.pi / 180
local seed = { math.random() * 10, math.random() * 10 }
local gust = 0

-- Where the sun is: altitude and azimuth (from north, east positive).
local function sun(ms, lat, lon)
  local d = ms / 86400000 - 10957.5
  local g = (357.529 + 0.98560028 * d) % 360
  local q = (280.459 + 0.98564736 * d) % 360
  local L = (q + 1.915 * math.sin(g * D) + 0.020 * math.sin(2 * g * D)) % 360
  local e = 23.439 - 0.00000036 * d
  local ra = math.atan2(math.cos(e * D) * math.sin(L * D), math.cos(L * D)) / D / 15
  local dec = math.asin(math.sin(e * D) * math.sin(L * D))
  local lst = (18.697374558 + 24.06570982441908 * d + lon / 15) % 24
  local H = (lst - ra) * 15 * D
  local phi = lat * D
  local alt = math.asin(math.sin(phi) * math.sin(dec) + math.cos(phi) * math.cos(dec) * math.cos(H))
  local az = math.atan2(-math.cos(dec) * math.sin(H), math.sin(dec) * math.cos(phi) - math.cos(dec) * math.sin(phi) * math.cos(H))
  return alt, az
end

function draw(c)
  local place = c:place()
  local ms = c:now()
  -- An illustrated afternoon works without location access. Only an
  -- explicitly chosen place turns this into the user's local sky.
  local alt, az = 25 * D, 0.4
  if place then alt, az = sun(ms, place.lat, place.lon) end
  -- Facing the equator: from the north, east is on the left; from the
  -- south, the sun crosses the northern sky and east is on the right.
  local x = -math.sin(az)
  if place and place.lat < 0 then x = -x end
  -- The cover eases over the hours; a tap on the paper gusts the wind.
  local hours = ms / 3600000
  local cover = 0.2 + 0.22 * (0.5 + 0.5 * math.sin(hours * 0.7 + seed[1]))
  for _ in c:taps() do gust = 1 end
  gust = math.max(0, gust - c.dt * 0.15)
  c:backdrop(if alt > -0.05 then "light" else "dark")
  c:sky({ az = x, alt = math.sin(alt), cover = cover, wind = 1 + gust * 3, seed = seed })
end

-- name: the sky
-- says: the sky over this machine, now — the sun where it is, clouds on the wind

-- The sky: one call to the canvas's own weather. The sun's height and
-- bearing come from the clock and the place (the same arithmetic the star
-- chart uses), so the light is morning light in the morning, the dusk
-- goes warm, and at night there is a moon and stars. The clouds are a
-- noise field lit from the sun, drifting; the cover eases over the hours
-- so no two afternoons are the same. Pointer input leaves the sky undisturbed.

local D = math.pi / 180
local seed = { 3.71, 1.39 }

function draw(c)
  local place = c:place()
  local ms = c:now()
  -- An illustrated afternoon works without location access. Only an
  -- explicitly chosen place turns this into the user's local sky.
  local alt, az = 25 * D, 0.4
  local ephemeris = nil
  if place then
    ephemeris = c:celestial(ms, place.lat, place.lon)
    alt, az = ephemeris.sun.alt, ephemeris.sun.az
  end
  -- Facing the equator: from the north, east is on the left; from the
  -- south, the sun crosses the northern sky and east is on the right.
  local x = -math.sin(az)
  if place and place.lat < 0 then x = -x end
  -- The cover eases over the hours; the wind stays steady through pointer input.
  local hours = ms / 3600000
  local cover = 0.2 + 0.22 * (0.5 + 0.5 * math.sin(hours * 0.7 + seed[1]))
  c:backdrop(if alt > -0.05 then "light" else "dark")
  local mx, ma, light, waxing = 0, -1, 0, 1
  if ephemeris then
    mx = -math.sin(ephemeris.moon.az)
    if place.lat < 0 then mx = -mx end
    ma, light = math.sin(ephemeris.moon.alt), ephemeris.illumination
    waxing = if ephemeris.waxing then 1 else 0
  end
  c:sky({ az = x, alt = math.sin(alt), cover = cover, wind = 1, seed = seed,
    moon_az = mx, moon_alt = ma, moon_light = light, moon_waxing = waxing })
end

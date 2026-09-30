-- name: the sky
-- says: an immense sky above your place — layered clouds, sunlight, and weather

-- Geometry is procedural. Place supplies the dated Sun and Moon. Connected
-- weather is separately opt-in; it supplies forecast conditions, never a
-- photograph or a reconstruction of the exact clouds outside the window.
local D = math.pi / 180
local seed = 42

local function direction(alt, az)
  return { math.cos(alt) * math.sin(az), math.sin(alt), math.cos(alt) * math.cos(az) }
end

local function wind(w, fallback, scale)
  if w and w.speed ~= nil and w.direction ~= nil then
    local a = w.direction * D
    -- Forecast bearings say where wind comes FROM; the volume moves downwind.
    return { -math.sin(a) * w.speed, -math.cos(a) * w.speed }
  end
  return { fallback[1] * scale, fallback[2] * scale }
end

function draw(c)
  local place = c:place()
  local weather = c:weather()
  local alt, az = 25 * D, 225 * D
  local moon, illumination, waxing = { 0, -1, 0 }, 0, true
  local bearing = math.pi
  if place then
    local e = c:celestial(c:now(), place.lat, place.lon)
    alt, az = e.sun.alt, e.sun.az
    moon = direction(e.moon.alt, e.moon.az)
    illumination, waxing = e.illumination, e.waxing
    if place.lat < 0 then bearing = 0 end
  end

  local low, middle, high, precipitation, haze = 0.43, 0.03, 0.18, 0, 0.08
  local low_wind, middle_wind, high_wind = { -7, -2 }, { -10, -3 }, { -15, -4 }
  if weather then
    low = weather.low or weather.cover or low
    middle, high = weather.middle or middle, weather.high or high
    precipitation = weather.precipitation or 0
    haze = math.max(weather.fog or 0, math.max(0, (weather.humidity or 0.5) - 0.65) * 0.6)
    low_wind = wind(weather.wind_low, low_wind, 1)
    -- Upper winds remain modeled when the provider has no measurements there.
    middle_wind = wind(weather.wind_middle, low_wind, 1.4)
    high_wind = wind(weather.wind_high, low_wind, 1.8)
  end
  local stratus = math.clamp((low - 0.64) / 0.26, 0, 1)
  if precipitation > 0.1 then stratus = math.max(stratus, 0.7) end
  c:backdrop(if alt > -0.05 then "light" else "dark")
  c:atmosphere({ sun = direction(alt, az), moon = moon,
    moon_light = illumination, moon_waxing = waxing, bearing = bearing,
    low = low, middle = middle, high = high, stratus = stratus,
    precipitation = precipitation, haze = haze,
    wind_low = low_wind, wind_middle = middle_wind, wind_high = high_wind,
    seed = seed, prompt_light = 0.14 })
end

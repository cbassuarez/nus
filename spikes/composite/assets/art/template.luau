-- name: mine
-- says: a blank, with a few circles drifting

-- Kept between frames: the file is one Luau chunk, `draw` runs each frame.
local dots = {}
for i = 1, 12 do
  dots[i] = { x = math.random(), y = math.random(), r = 6 + math.random() * 18, vx = (math.random() - 0.5) * 0.02, vy = (math.random() - 0.5) * 0.02 }
end

-- Nothing over the line, nor the rows beneath it while something is typed.
local function clear_of_line(c, x, y)
  local l = c.prompt
  return x < l.x - 40 or x > l.x + l.w + 40 or y < l.y - 40 or y > l.y + l.h + c.rows + 40
end

function draw(c)
  for _, d in dots do
    d.x = (d.x + d.vx * c.dt) % 1
    d.y = (d.y + d.vy * c.dt) % 1
    local x, y = d.x * c.w, d.y * c.h
    if clear_of_line(c, x, y) then
      c:circle(x, y, d.r, c.signal, 0.12)
    end
  end
  c:text(24, c.h - 44, "mine · edit me in profile/art", 11, c.dim, 0.8, { caps = true })
end

# The canvas an art draws on

A nus art is one Luau file in `profile/art/`. It defines `draw(c)`, which
nus calls every frame with the canvas `c`. Whatever the file keeps in
locals at the top persists between frames; the file reloads when saved.
Draw nothing over the line: `c.prompt` is its box, and `c.rows` says how far
the rows beneath it reach while something is typed.

    c.w, c.h              the pane, in px — draw within it
    c.prompt              {x, y, w, h}: the prompt's line, keep clear of it
    c.rows                px the rows reach beneath the line (0 when nothing is typed)
    c.t, c.dt             seconds since the art began, and since the last frame
    c.face                "paper" or "ink"
    c.paper c.ink c.signal c.dim c.tint   the tokens, as "#rrggbb"
    c.signals             {red, blue, gold, green, violet, teal}
    c.pointer             {x, y} or nil
    c.typed               what is typed on the line
    c:taps()              {{x=, y=}, …} clicks since the last frame
    c:now()               unix milliseconds (the sky's own clock on Home: the viewer may have turned it)
    c:place()             {lat, lon} chosen by the user, or nil; draw a fallback when unset
    c:weather()           cached forecast conditions when connected weather is enabled,
                           otherwise nil; never fetches from a script
    c:processes()         {list = {{pid, ppid, name, cpu, mem, threads}, …},
                           ctx, syscalls, threads, handles, ready}

`c.face` describes resolved text polarity: "ink" means light text and
"paper" means dark text, including when a saved palette has been tinted.
`c.signals` contains the selected theme's six artwork colors in the order
shown above. Older/custom themes without an artwork palette use the legacy
six colors. The table stays alive and updates in place on a theme change,
so an artwork can retain it without restarting its animation. Treat it as
read-only; copy entries when an artwork needs private mutable colors.

Colours are "#rrggbb", "#rrggbbaa" or {r=, g=, b=, a=} in 0..1; the
optional `alpha` multiplies.

    c:rect(x, y, w, h, colour, alpha?, radius?)
    c:circle(cx, cy, r, colour, alpha?)
    c:oval(cx, cy, rx, ry, colour, alpha?)
    c:line(x1, y1, x2, y2, width, colour, alpha?)
    c:quad({{x,y},{x,y},{x,y},{x,y}}, colour, alpha?)     a convex quad
    c:poly({{x,y}, …}, colour, alpha?)                     any simple polygon
    c:blob({{x,y}, …}, colour, alpha?)                     a smooth closed curve through the points, filled
    c:curve({{x,y}, …}, width, colour, alpha?)             a smooth open curve
    c:text(x, y, text, px, colour, alpha?, {font = "mono"|"serif"|"strong", align = "left"|"center"|"right", caps = true})
    c:measure(text, px, font?)   a near-enough width
    c:mix(a, b, t)               a colour between two
    c:rgba(r, g, b, a?)          a colour table
    c:sky({az=, alt=, cover=, wind=, seed={x,y}, x?, y?, w?, h?})
                                 a whole sky in the shader: az -1 (east, left) … 1 (west, right), alt the sine of the
                                 sun's altitude (night below 0: a moon, stars), cover 0..1, wind 1 = a breeze
    c:backdrop("dark")           light text over dark artwork; "light" for dark text; default "paper" follows the theme

`c:atmosphere(options)` draws the native cached cloud volume. The older `c:sky`
primitive remains available for saved artwork. Atmosphere options:

    sun, moon            {east, up, north} unit directions (all three axes matter)
    moon_light           illuminated fraction 0..1; moon_waxing is a boolean
    bearing, elevation   view angles in radians; bearing clockwise from north
    fov                  vertical field of view, radians
    low, middle, high    cloud fractions 0..1 at the three modeled layers
    stratus              0 sculpted cumulus .. 1 layered ceiling
    precipitation        next-hour mean precipitation, mm/h
    base, haze           low cloud base in km; haze 0..1
    wind_low, wind_middle, wind_high   {east, north} velocity, m/s
    seed                 stable integer cloud identity
    astro                true: with a Place, nus lays the real sky in (stars, planets, an exact Sun
                         and Moon, eclipses) and overrides sun/moon above on Home
    prompt_light         subtle local prompt contrast, 0..0.35 (default .14)
    x, y, w, h           optional canvas rectangle

The weather table has `cover`, `low`, `middle`, `high`, `humidity`, `fog`
fractions, `visibility` in meters, and `precipitation` in mm for the next hour.
Unknown fields are nil. Winds are `{speed, direction, height}`: meters/second,
degrees clockwise from north **from which** wind blows, and meters when known.
`source`, `valid_at`, `fetched_at`, `stale`, `offline`, and `code` describe the
forecast's provenance. Times are Unix seconds. The current MET Norway provider
supplies surface wind; middle/high cloud motion is modeled when upper winds are
unavailable. Cloud species, shapes, and base heights are procedural rather than
observed. The same cloud field evolves when the forecast changes.

Connected weather sends the explicitly chosen Place to MET Norway only after
the user enables it in Start/New Tab. Without that option, astronomy stays local.
Data attribution: [MET Norway](https://www.met.no/en), adapted from
[Locationforecast](https://api.met.no/weatherapi/locationforecast/2.0/documentation)
under [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).

The file's first lines say what it is:

    -- name: the pond
    -- says: four koi, seen from above

Be quiet: a few hundred primitives a frame is plenty; low alphas; slow
motion; nothing on the line.


### Native orbital scene

`c:orbital({})` draws the shared, cached Limb / Darkroom renderer. It is the
implementation behind the existing `space` key, not a separate built-in choice.
Options `x,y,w,h` default to the full canvas; `phase` defaults to the recovered
opening camera; `blend` is 0 for Earth and 1 for Darkroom; `time` defaults to 0;
`exposure=1`, `seed=42`, `lines=true`. At most two orbital commands per canvas.
The native Home owns the Space turn/hold/typing clock. Settings uses the static
opening pose. `c.prompt` and the current result rows attenuate catalogue stars
and exclude native labels; this does not add another opaque text-backdrop layer.
Maps are historical composites, not live imagery. See `space/NOTICE.md`.

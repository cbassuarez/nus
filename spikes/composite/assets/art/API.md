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
    c:now()               unix milliseconds
    c:place()             {lat, lon} — the machine's, for a sky
    c:processes()         {list = {{pid, ppid, name, cpu, mem, threads}, …},
                           ctx, syscalls, threads, handles, ready}

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
    c:backdrop("dark")           say the art is dark: the line goes paper with a shadow (default "paper")

The file's first lines say what it is:

    -- name: the pond
    -- says: four koi, seen from above

Be quiet: a few hundred primitives a frame is plenty; low alphas; slow
motion; nothing on the line.

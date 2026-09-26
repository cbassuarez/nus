//! The selvedge: a shell that runs somewhere else wears that place's name
//! on its edge, set like a BSD man page. Reverse video across the top,
//! `HOST(SSH)   nus · elsewhere   HOST(SSH)`; a footer below as login(1)
//! would print it; ticked rules down the sides. Colour says which place,
//! the lettering says elsewhere, so it still reads in grey, in a
//! screenshot, or to someone who can't tell the hues apart. A local
//! shell is never lettered.
//!
//! Guarded places (names matching `guarded_places`, `*prod*` out of the
//! box) swap the header for a motd banner, `*** HOST(SSH) *** GUARDED`,
//! on the signal colour, and the footer for a row of `/ / /`.
//! A session whose program has gone prints `-- no carrier --`.

use nus_render::{Color, Rect, Scene, Style};

use crate::app::{App, TermPane};

/// Where a shell is, when it isn't here.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// SSH, MOSH, ET, WSL; REMOTE when only the shell's own report says so.
    pub kind: &'static str,
    pub guarded: bool,
}

impl Place {
    /// `BUILD-01(SSH)`: the man page's name for it.
    pub fn tag(&self) -> String {
        format!("{}({})", self.name.to_uppercase(), self.kind)
    }
}

/// `*` matches any run; case is ignored. `*prod*`, `db-?` is not special.
pub fn matches(pattern: &str, name: &str) -> bool {
    let (p, n) = (pattern.to_ascii_lowercase(), name.to_ascii_lowercase());
    let parts: Vec<&str> = p.split('*').collect();
    if parts.len() == 1 {
        return p == n;
    }
    let mut rest = n.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            match rest.strip_prefix(part) {
                Some(r) => rest = r,
                None => return false,
            }
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else {
            match rest.find(part) {
                Some(at) => rest = &rest[at + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// The distro a `wsl.exe -d <name>` profile opens.
fn wsl_distro(p: &nus_pty::Profile) -> Option<String> {
    let mut it = p.args.iter();
    while let Some(a) = it.next() {
        if a == "-d" || a == "--distribution" {
            return it.next().cloned();
        }
    }
    p.name.strip_prefix("wsl:").map(str::to_string)
}

fn kind_of(program: &str) -> &'static str {
    match nus_pty::discover::base(program).as_str() {
        "ssh" | "autossh" => "SSH",
        "mosh" => "MOSH",
        "et" => "ET",
        _ => "REMOTE",
    }
}

/// Where this pane's shell is: a tunnel (ssh, mosh, et, or a remote
/// cwd reported over OSC 7) or a WSL distro. None on this machine.
pub fn place_of(p: &TermPane, profile: Option<&nus_pty::Profile>, guarded: &[String]) -> Option<Place> {
    let (name, kind) = if let Some(host) = p.tunnel() {
        let running = p.blocks().last().filter(|b| b.running).map(|b| crate::blocks::program_of(&b.cmd));
        let kind = match running.as_deref().map(kind_of) {
            Some(k) if k != "REMOTE" => k,
            _ => kind_of(&p.program),
        };
        let kind = if kind == "REMOTE" && p.ssh_host.is_some() { "SSH" } else { kind };
        (host, kind)
    } else {
        let prof = profile.filter(|pr| nus_pty::discover::base(&pr.program) == "wsl")?;
        (wsl_distro(prof).unwrap_or_else(|| "wsl".into()), "WSL")
    };
    let guarded = guarded.iter().any(|g| matches(g.trim(), &name));
    Some(Place { name, kind, guarded })
}

impl App {
    /// The pane's place, under the current settings.
    pub(crate) fn pane_place(&self, p: &TermPane) -> Option<Place> {
        if !self.behavior.selvedge {
            return None;
        }
        place_of(p, self.profiles.get(p.profile), &self.behavior.guarded_places)
    }

    /// The selvedge's own face: the terminal's, small, lightly tracked.
    fn selvedge_style(&self, px: f32, color: Color) -> Style {
        Style { font: self.f.term, px, color, tracking: px * 0.04 }
    }

    /// Heights of the header and footer bands; they sit in the pane's
    /// padding, so the grid never moves.
    pub(crate) fn selvedge_bands(&self) -> (f32, f32) {
        (self.px(14.0), self.px(12.0))
    }

    /// Three cells across a band: left, centre, right, each clipped to its third.
    fn band_line(&mut self, scene: &mut Scene, r: Rect, style: Style, cells: [&str; 3]) {
        let pad = self.px(8.0);
        let base = r.y + r.h * 0.5 + style.px * 0.36;
        let [l, c, rt] = cells;
        let prev = scene.clip();
        scene.layer(Some(prev.map(|c| c.intersect(&r)).unwrap_or(r)));
        self.fonts.draw(scene, style, r.x + pad, base, l);
        let rw = self.fonts.measure(style, rt);
        self.fonts.draw(scene, style, r.right() - pad - rw, base, rt);
        let lw = self.fonts.measure(style, l);
        let cw = self.fonts.measure(style, c);
        // The centre only where it clears both ends.
        let cx = r.x + (r.w - cw) * 0.5;
        if cx > r.x + pad + lw + pad && cx + cw < r.right() - pad - rw - pad {
            self.fonts.draw(scene, style, cx, base, c);
        }
        scene.layer(prev);
    }

    /// Letter the edge of `clip` for `place`. `down`: the program is gone.
    pub(crate) fn draw_selvedge(&mut self, scene: &mut Scene, clip: Rect, place: &Place, down: bool, footer: [String; 3]) {
        let t = self.theme.clone();
        // Guarded is danger whatever the theme's accent: a fixed red, made to read.
        let danger = nus_render::oklch::readable(nus_render::theme::hex(0xb3122b), t.paper, 4.5);
        let (top_h, foot_h) = self.selvedge_bands();
        let hue = self.tunnel_color(&place.name);
        let rule = if place.guarded { danger } else if down { t.dim } else { t.ink };
        let head = Rect::new(clip.x, clip.y, clip.w, top_h);
        let foot = Rect::new(clip.x, clip.bottom() - foot_h, clip.w, foot_h);
        let small = self.px(9.5);

        // Header: reverse video, or the motd banner when guarded.
        let fill = if place.guarded { danger } else if down { t.dim } else { t.ink };
        scene.rect(head, fill);
        let on = self.on_fill(fill);
        let tag = place.tag();
        if place.guarded {
            let s = Style { font: self.f.strong, ..self.selvedge_style(small, on) };
            let end = format!("*** {tag} ***");
            self.band_line(scene, head, s, [&end, "GUARDED", &end]);
        } else {
            // A square of the place's hue before its name: which place, at a glance.
            let sq = top_h - self.px(6.0);
            scene.rect(Rect::new(head.x + self.px(8.0), head.y + self.px(3.0), sq, sq), hue);
            let s = self.selvedge_style(small, on);
            let lead = format!("{}{tag}", " ".repeat(3));
            let centre = if down { "-- no carrier --" } else { "nus \u{b7} elsewhere" };
            self.band_line(scene, head, s, [&lead, centre, &tag]);
        }

        // Footer: login(1)'s line, or the guarded hatch.
        scene.rect(foot, t.paper);
        scene.rect(Rect::new(foot.x, foot.y, foot.w, self.px(1.0)), rule);
        if place.guarded {
            let s = Style { font: self.f.strong, ..self.selvedge_style(small, danger) };
            let unit = self.fonts.measure(s, "/ ").max(1.0);
            let n = (foot.w / unit).ceil() as usize + 1;
            let hatch = "/ ".repeat(n);
            self.band_line(scene, foot, s, [&hatch, "", ""]);
        } else {
            let s = self.selvedge_style(self.px(9.0), t.ink);
            let [l, c, r] = footer;
            self.band_line(scene, foot, s, [&l, &c, &r]);
        }

        // Sides: ticked, never solid. A solid edge is the window; a ticked
        // one is a line to somewhere.
        let (w, on_len, gap) = (self.px(3.0), self.px(7.0), self.px(4.0));
        let mut y = head.bottom();
        while y < foot.y {
            let h = on_len.min(foot.y - y);
            scene.rect(Rect::new(clip.x, y, w, h), rule);
            scene.rect(Rect::new(clip.right() - w, y, w, h), rule);
            y += on_len + gap;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_match_like_a_shell_glob() {
        assert!(matches("*prod*", "prod-bastion"));
        assert!(matches("*prod*", "db.PROD.internal"));
        assert!(matches("prod-*", "prod-1"));
        assert!(!matches("prod-*", "preprod-1"));
        assert!(matches("*-db", "orders-db"));
        assert!(!matches("*prod*", "build-01"));
        assert!(matches("build-01", "BUILD-01"));
        assert!(matches("a*b*c", "a-x-b-y-c"));
        assert!(!matches("a*b*c", "a-x-c-y-b"));
    }

    #[test]
    fn tags_read_like_man_pages() {
        let p = Place { name: "build-01".into(), kind: "SSH", guarded: false };
        assert_eq!(p.tag(), "BUILD-01(SSH)");
    }

    #[test]
    fn wsl_profiles_name_their_distro() {
        let mut p = nus_pty::Profile::wsl("Ubuntu");
        assert_eq!(wsl_distro(&p).as_deref(), Some("Ubuntu"));
        p.args = vec!["--distribution".into(), "Debian".into()];
        assert_eq!(wsl_distro(&p).as_deref(), Some("Debian"));
        assert_eq!(kind_of("/usr/bin/ssh"), "SSH");
        assert_eq!(kind_of("mosh"), "MOSH");
        assert_eq!(kind_of("zsh"), "REMOTE");
    }
}

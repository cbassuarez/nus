//! The welcome page: a full tab, not a PTY — the whole of nus laid out
//! as a ruled document you can act from. Every row has a chord you can
//! read and, where it makes sense, a TRY button that does the thing:
//! opens the palette, runs a demo in a shell, lights hints, opens the
//! studio. The first section is a checklist that reads the real state
//! (shell integration per profile, default browser, login item, the
//! look). It opens on first launch, from F1, the palette, and settings.

use nus_render::text::Style;
use nus_render::{Rect, Scene};
use std::sync::Arc;

use crate::app::{fade, hover_key, App, IconMotion, Pane, PaletteMode};
use nus_render::theme::metric as m;

/// What a welcome-page control does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Palette(PaletteMode),
    NewShell,
    Split,
    Studio,
    Settings(usize),
    Reader,
    Hints,
    Find,
    Demo(&'static str),
    Rules,
    DefaultBrowser,
    LoginItem,
    NewWindow,
    Rename,
    RenameTab,
    Close,
    Scroll(f32),
}

/// One row: chord (may be empty), title, what it does, an optional action.
struct Row {
    chord: String,
    title: &'static str,
    what: String,
    act: Option<(&'static str, Act)>,
}

fn row(chord: impl Into<String>, title: &'static str, what: impl Into<String>, act: Option<(&'static str, Act)>) -> Row {
    Row { chord: chord.into(), title, what: what.into(), act }
}

/// The platform's chord prefix in words.
fn k(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("⌘⇧{key}") } else { format!("CTRL+SHIFT+{key}") }
}

impl App {
    /// Open the welcome page as its own tab (or go to it).
    pub(crate) fn open_welcome(&mut self) {
        if let Some(i) = self.tabs.iter().position(|t| matches!(t.left, Pane::Hints(_))) {
            return self.activate(i);
        }
        let tab = self.make_tab(Pane::Hints(crate::app::HintsPane { rect: Rect::new(0.0, 0.0, 1.0, 1.0), scroll: 0.0 }), None);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1);
        self.layout();
    }

    /// The app icon as a texture, made once per look.
    fn welcome_icon(&mut self) -> Option<Arc<wgpu::BindGroup>> {
        let key = (self.theme.mode, self.surface.signal);
        if let Some((k, b)) = &self.welcome_icon_tex {
            if *k == key {
                return Some(b.clone());
            }
        }
        let n = self.surface.base.unwrap_or(self.theme.ink);
        let size = 256;
        let rgba = nus_render::icon::app_icon(size, n, self.surface.signal);
        let bgra: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[2], p[1], p[0], p[3]]).collect();
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("welcome icon"),
            size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bgra,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(size * 4), rows_per_image: Some(size) },
            wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        );
        let b = (self.bind_texture)(&tex);
        self.welcome_icon_tex = Some((key, b.clone()));
        Some(b)
    }

    /// The sections, with live state folded in.
    fn welcome_sections(&self) -> Vec<(&'static str, &'static str, Vec<Row>)> {
        use crate::settings::{LOOK_PRESETS, SEC_LOOK, SEC_SOUND, SEC_STARTUP, SEC_TERMINAL, SEC_BROWSER, RULES};
        let ink = self.theme.mode == nus_render::Mode::Ink;
        let default_browser = crate::little::registered();
        let login = crate::little::login_item_registered();
        let mut start = vec![
            row("", "The look", format!("{} · {} · {} carapace · the chip in the footer hot-swaps on hover", self.preset_name.to_lowercase(), if ink { "ink" } else { "paper" }, self.surface.shell.name()), Some(("OPEN THE STUDIO", Act::Studio))),
            row("", "Default browser", if default_browser { "nus is your default browser · links from other apps open in the little window".to_string() } else { "not yet · links from other apps would open here in a little window".to_string() }, if default_browser { None } else { Some(("MAKE DEFAULT", Act::DefaultBrowser)) }),
            row("", "Start on login", if login { "on · nus starts with the system".to_string() } else { "off".to_string() }, Some((if login { "TURN OFF" } else { "TURN ON" }, Act::LoginItem))),
        ];
        for p in self.profiles.iter().take(4) {
            let kind = crate::shell::kind_of(&p.program);
            let on = self.behavior.shell_integration && !matches!(kind, crate::shell::Kind::Other);
            start.push(row("", if on { "Shell integration" } else { "Shell integration · off" }, format!("{} · {}", p.name, crate::shell::describe(kind)), Some(("TERMINAL SETTINGS", Act::Settings(SEC_TERMINAL)))));
        }
        let shell = vec![
            row("ENTER", "A URL at the prompt", "type a URL at a fresh prompt and Enter opens it beside; Ctrl+Enter runs it in the shell", Some(("TRY", Act::Demo("https://docs.rs/wgpu")))),
            row(k("↑ / ↓"), "Jump between prompts", "each command is a block: a hairline where it starts, an × code when it failed, DONE when a long one finishes", Some(("TRY", Act::Demo("git log --oneline -3")))),
            row(k("C"), "Copy the last output", "or the selection when there is one · right-click copies a selection, pastes otherwise", None),
            row(k("V"), "Paste, carefully", "bracketed when the program asks; many lines or control characters ask first", None),
            row(k("O"), "Hints", "labels over every URL, path and hash on screen · type one: URLs open, the rest copies", Some(("TRY", Act::Hints))),
            row(k("F"), "Find in scrollback", "a band; Enter next, Shift+Enter back; the scrollbar's ticks are the prompts", Some(("TRY", Act::Find))),
            row("→ / END", "Predictions", "the history entry that continues your line ghosts after the caret; Right or End accepts · tokens colour as you type", Some(("TRY", Act::Demo("git st")))),
            row("CLICK", "Select and move", "double-click a word, triple a line, Ctrl+triple a command's output · at a prompt a click moves the caret", None),
            row("", "Hover a block", "a gutter rule and two chips: copy its output, run it again", None),
            row("", "Images", "Kitty and iTerm2 image protocols draw right in the shell (icat, timg, chafa)", None),
        ];
        let pages = vec![
            row(k("T"), "New tab", "a name for a shell, a URL for a page; NEW TAB in the sidebar opens the default shell, hold or right-click for the kinds", Some(("TRY", Act::Palette(PaletteMode::New)))),
            row(k("K"), "The palette", "tabs, ports, history, commands to run again, settings, ask an assistant", Some(("TRY", Act::Palette(PaletteMode::Go)))),
            row(k("L"), "Address", "history ranked by visits and recency; a search when it isn't a URL", Some(("TRY", Act::Palette(PaletteMode::Url)))),
            row(k("D"), "Split", "a page beside the shell, or two of anything", Some(("TRY", Act::Split))),
            row(k("R"), "Reader", "the article, set in Newsreader on our paper; the page keeps living underneath", Some(("TRY", Act::Reader))),
            row(k("F"), "Find in page", "the same band, on a page", None),
            row("", "Downloads", format!("straight to {} · the list rises from the footer; click one to reveal it", crate::browser::downloads_dir().to_string_lossy().replace(char::from(92), "/")), None),
            row("", "Permissions", "a page asking for the camera, location or notifications gets a band, not a dialog", None),
            row("", "Content blocking", format!("{} ad, tracker and analytics hosts refused · add yours to profile/blocklist.txt", crate::browser::blocklist_len()), Some(("BROWSER SETTINGS", Act::Settings(SEC_BROWSER)))),
            row("", "Sleep and archive", format!("idle pages sleep after {} minutes and archive after {} hours; pinned tabs and shells never", self.behavior.sleep_after_min, self.behavior.archive_after_h), None),
            row("ESC", "Little nus", "links from other apps open in a small floating window; Ctrl+Shift+O keeps one as a tab", None),
        ];
        let windows = vec![
            row("SHIFT+F2", "Name this window", format!("“{}” · auto-named from the git root or the host; the name is in the title bar and Alt-Tab", self.window_name()), Some(("RENAME", Act::Rename))),
            row("CTRL+N", "New window", "windows own their tabs; the rail or the header lists them", Some(("OPEN ONE", Act::NewWindow))),
            row("CTRL+1–9", "Tabs by number", "Ctrl+` goes back to the last one; Ctrl+PgUp / PgDn walk them", None),
            row("", "Containers", "named cookie jars: “container” in the palette lists them, switches this window's (its square wears the colour; new windows inherit it), reopens a page in one, or makes a new one — sign-ins stay apart", None),
            row("CTRL+SHIFT+F11", "Focus", "the page (or shell) alone in the window: no strip, no sidebar, no rows · the same chord leaves", None),
            row(k("B"), "Compact", "a 48px column of icons; the top strip hides until the pointer reaches the top; hover a row for its name", None),
            row(k("S"), "The sidebar", "pin it, or let it slide in from the edge · BAR or RAIL header under settings", None),
            row("", "Site panel", "the gear at the end of a page's URL row: zoom (remembered), autoplay, JavaScript, cookies, boosts, blocking and the permissions this site was given · per host", None),
            row("", "Folders", "under the tabs: GITHUB (open pull requests, via gh), PORTS (what's listening), lists from rules.luau, and your own — SAVE TO FOLDER in a page's menu; a saved page never archives", None),
            row(k("?"), "Ask", "a small panel beside the shell: one line in, a few commands out — INSERT at the prompt, RUN, or COPY; the shell, folder and last output go along · claude, codex, copilot, ollama or ANTHROPIC_API_KEY", None),
            row(k("K"), "Chains", "named lists of palette commands in rules.luau — open pages, run commands, tile — and every settings row, by name", None),
            row("ALT+CLICK", "Peek", "a link floats over the page instead of leaving it · Esc closes, Ctrl+Enter keeps it in the stack", None),
            row(k("D"), "Tiles", "Ctrl+click two to four rows, then tile them: side by side, an L, or a grid · drag the rules · Ctrl+Alt+arrows walk the tiles, with Shift they swap", None),
            row("F2", "Name a tab", "right-click a tab for its menu: rename, an emoji or short string as its icon, a colour, pin, close", Some(("RENAME THIS TAB", Act::RenameTab))),
            row("", "Stacks", "pages a page opens sit under it, as deep as they go; fold with the caret, Ctrl+Shift+- folds all; drag a tab onto another to nest it", None),
            row("", "Rules", "profile/rules.luau colours new tabs and windows, boosts pages, picks sounds", Some(("OPEN RULES", Act::Rules))),
        ];
        let look = vec![
            row("", "The studio", "a live proof of the window, presets as cards, tokens as tiles, a real picker", Some(("OPEN", Act::Settings(SEC_LOOK)))),
            row("", "Sound", "seventeen cues on fourteen events, synthesised in the app", Some(("OPEN", Act::Settings(SEC_SOUND)))),
            row("", "Startup", "how the window comes up, the splash, what follows, the atlas", Some(("OPEN", Act::Settings(SEC_STARTUP)))),
            row("", "Cursor", "shape, blink, colour, glide or comet, the pointer over the chrome", Some(("OPEN", Act::Settings(SEC_LOOK)))),
            row("F11", "Fullscreen", "the sidebar follows the rule you set for it", None),
        ];
        let _ = (LOOK_PRESETS, RULES);
        vec![
            ("START HERE", "What's on, what isn't, and where to change it.", start),
            ("THE SHELL", "Prompt marks make the shell legible: blocks, jumps, hints, predictions.", shell),
            ("PAGES", "A browser under the same carapace, with the same rules.", pages),
            ("WINDOWS AND TABS", "A window owns its tabs. Everyone knows what a window is.", windows),
            ("THE LOOK", "Broadsheet's tokens are fixed; everything around them is yours.", look),
        ]
    }

    /// Draw the page into `r`, scrolled by the pane's offset.
    pub(crate) fn draw_welcome(&mut self, scene: &mut Scene, r: Rect, scroll: f32) -> f32 {
        let t = self.theme.clone();
        let ink = t.ink;
        let paper = self.paper();
        let label = self.label();
        let strong = self.label_strong();
        let ui = self.ui();
        let dim = Style { color: t.dim, ..label };
        self.welcome_hits.clear();
        scene.layer(Some(r));
        let pad = self.px(40.0).min(r.w * 0.06);
        let colw = self.px(150.0);
        let maxw = (r.w - 2.0 * pad).min(self.px(900.0));
        let x0 = r.x + pad;
        let mut y = r.y + self.px(36.0) - scroll;
        let (mx, my) = self.mouse;

        // Masthead: the icon, the wordmark, the line.
        let isz = self.px(96.0);
        if let Some(b) = self.welcome_icon() {
            scene.texture(Rect::new(x0, y, isz, isz), b, Some(r));
            scene.layer(Some(r));
        }
        let wm = Style { font: self.f.wordmark, px: self.px(64.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, wm, x0 + isz + self.px(22.0), y + self.px(58.0), "nus");
        let line = Style { font: self.f.serif, px: self.px(19.0), color: ink, tracking: 0.0 };
        self.fonts.draw(scene, line, x0 + isz + self.px(24.0), y + self.px(88.0), "a terminal and a browser, under one carapace.");
        // Close, top right.
        {
            let text = "CLOSE THIS PAGE";
            let tw = self.fonts.measure(label, text);
            let hit = Rect::new(x0 + maxw - tw - self.px(20.0), y + self.px(4.0), tw + self.px(20.0), self.px(28.0));
            let hot = hit.contains(mx, my);
            if hot {
                scene.rect(hit, t.tint);
            }
            self.fonts.draw(scene, dim, hit.x + self.px(10.0), hit.y + self.px(19.0), text);
            self.welcome_hits.push((hit, Act::Close));
        }
        y += isz + self.px(28.0);
        let intro = "Everything below works right now. Chords are on the left; TRY does the thing. This page is a tab — close it when you're done, F1 brings it back.";
        for l in crate::reader::wrap(&self.fonts, ui, intro, maxw) {
            self.fonts.draw(scene, Style { color: t.dim, ..ui }, x0, y + self.px(14.0), &l);
            y += self.px(20.0);
        }
        y += self.px(24.0);

        let sections = self.welcome_sections();
        for (name, lede, rows) in sections {
            // Section head: caption column, lede, a rule.
            scene.hline(x0, y, maxw, self.px(m::STRUCTURE), ink);
            y += self.px(16.0);
            self.fonts.draw(scene, strong, x0, y + self.px(12.0), name);
            let lede_st = Style { font: self.f.serif, px: self.px(15.0), color: ink, tracking: 0.0 };
            self.fonts.draw(scene, lede_st, x0 + colw, y + self.px(13.0), lede);
            y += self.px(36.0);
            for rw in rows {
                let row_top = y;
                // Chord chip in the caption column.
                if !rw.chord.is_empty() {
                    let cw = self.fonts.measure(label, &rw.chord) + self.px(16.0);
                    let chip = Rect::new(x0, y + self.px(2.0), cw.min(colw - self.px(10.0)), self.px(22.0));
                    scene.outline(chip, self.px(m::HAIRLINE), ink);
                    let ct = self.fit(label, &rw.chord, chip.w - self.px(10.0));
                    self.fonts.draw(scene, label, chip.x + self.px(8.0), chip.y + self.px(15.0), &ct);
                }
                // Title and what.
                let tx = x0 + colw;
                let mut avail = maxw - colw;
                if let Some((bt, _)) = &rw.act {
                    avail -= self.fonts.measure(strong, bt) + self.px(40.0);
                }
                self.fonts.draw(scene, Style { color: ink, ..strong }, tx, y + self.px(15.0), &rw.title.to_uppercase());
                let mut wy = y + self.px(34.0);
                for l in crate::reader::wrap(&self.fonts, ui, &rw.what, avail) {
                    self.fonts.draw(scene, Style { color: fade(ink, 0.82), ..ui }, tx, wy, &l);
                    wy += self.px(19.0);
                }
                // The button.
                if let Some((bt, act)) = rw.act.clone() {
                    let bw = self.fonts.measure(strong, bt) + self.px(24.0);
                    let b = Rect::new(x0 + maxw - bw, y + self.px(2.0), bw, self.px(26.0));
                    let hot = b.contains(mx, my);
                    let key = hover_key("welcome", (row_top as i64).unsigned_abs() as usize);
                    let lift = {
                        let h = self.hovers.entry(key).or_insert_with(|| crate::app::Hover { alpha: crate::anim::Anim::at(0.0), pulse: crate::anim::Anim::at(1.0), hot: false });
                        if hot != h.hot {
                            h.hot = hot;
                            h.alpha.go(if hot { 1.0 } else { 0.0 }, 120.0);
                        }
                        if h.alpha.active() {
                            self.dirty = true;
                        }
                        h.alpha.value()
                    };
                    let off = self.px(3.0) + self.px(2.0) * lift;
                    let bb = Rect::new(b.x - self.px(1.0) * lift, b.y - self.px(1.0) * lift, b.w, b.h);
                    scene.rect(Rect::new(bb.x + off, bb.y + off, bb.w, bb.h), ink);
                    scene.rect(bb, if hot { ink } else { paper });
                    scene.outline(bb, self.px(m::STRUCTURE), ink);
                    self.fonts.draw(scene, Style { color: if hot { t.paper } else { ink }, ..strong }, bb.x + self.px(12.0), bb.y + self.px(17.0), bt);
                    self.welcome_hits.push((b, act));
                    let _ = IconMotion::Still;
                }
                y = wy.max(y + self.px(40.0)) + self.px(10.0);
                scene.hline(tx, y, maxw - colw, self.px(m::HAIRLINE), t.tint);
                y += self.px(8.0);
            }
            y += self.px(20.0);
        }
        // Foot.
        scene.hline(x0, y, maxw, self.px(m::STRUCTURE), ink);
        y += self.px(22.0);
        self.fonts.draw(scene, dim, x0, y, "F1 · THIS PAGE       CTRL+, · SETTINGS       PROFILE/RULES.LUAU · THE RULES       DOCS/PRODUCT.MD · THE PLAN");
        y += self.px(40.0);
        scene.layer(None);
        // Hits above or below the pane are unreachable.
        self.welcome_hits.retain(|(hr, _)| hr.bottom() > r.y && hr.y < r.bottom());
        let _ = paper;
        y + scroll - r.y
    }

    /// A click on the page. Returns true when consumed.
    pub(crate) fn welcome_click(&mut self, x: f32, y: f32) -> bool {
        let Some((_, act)) = self.welcome_hits.iter().find(|(r, _)| r.contains(x, y)).cloned() else { return false };
        self.welcome_act(act);
        true
    }

    pub(crate) fn welcome_act(&mut self, act: Act) {
        use crate::settings::Hit;
        self.play_event("control.press");
        match act {
            Act::Palette(m) => self.open_palette(m),
            Act::NewShell => {
                let p = self.behavior.default_profile;
                self.new_tab(p);
            }
            Act::Split => {
                // A shell with a page beside it, so the split shows.
                let p = self.behavior.default_profile;
                self.new_tab(p);
                if let Some(w) = self.new_web_pane("https://docs.rs/wgpu/latest/wgpu/") {
                    if let Some(tab) = self.tabs.get_mut(self.active) {
                        tab.right = Some(Pane::Web(w));
                    }
                    self.layout();
                }
            }
            Act::Studio => {
                self.open_settings();
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = crate::settings::SEC_LOOK;
                }
                self.look_tab = crate::settings::LOOK_PRESETS;
            }
            Act::Settings(k) => {
                self.open_settings();
                if let Some(Pane::Settings(s)) = self.tabs.get_mut(self.active).map(|t| &mut t.left) {
                    s.section = k;
                }
            }
            Act::Reader => {
                self.open_url("https://en.wikipedia.org/wiki/Monterey_Bay_Aquarium", true);
                self.welcome_pending = Some((Act::Reader, std::time::Instant::now()));
            }
            Act::Hints => {
                self.run_in_shell("echo https://docs.rs/wgpu ./src/main.rs 9058fca1");
                self.welcome_pending = Some((Act::Hints, std::time::Instant::now()));
            }
            Act::Find => {
                self.run_in_shell("git log --oneline -8");
                self.welcome_pending = Some((Act::Find, std::time::Instant::now()));
            }
            Act::Demo(cmd) => self.run_in_shell(cmd),
            Act::Rules => self.apply_setting(Hit::OpenRules, 0.0),
            Act::DefaultBrowser => self.apply_setting(Hit::MakeDefault, 0.0),
            Act::LoginItem => {
                let on = !crate::little::login_item_registered();
                self.apply_setting(Hit::LoginItem(on), 0.0);
            }
            Act::NewWindow => self.new_window_request = true,
            Act::Rename => self.open_palette(PaletteMode::Rename),
            Act::RenameTab => self.open_palette(PaletteMode::RenameTab(self.active)),
            Act::Close => self.dismiss_hints(),
            Act::Scroll(_) => {}
        }
        self.dirty = true;
    }
}

impl App {
    /// Demos that need a moment: hints and find once the shell is back at
    /// a prompt, reader once the page has loaded.
    pub(crate) fn welcome_tick(&mut self) {
        let Some((act, since)) = self.welcome_pending.clone() else { return };
        if since.elapsed().as_secs_f32() > 12.0 {
            self.welcome_pending = None;
            return;
        }
        match act {
            Act::Hints | Act::Find => {
                let ready = self.tabs.get_mut(self.active).map(|t| matches!(t.focused(), Pane::Term(p) if p.term.at_prompt() && p.running_since.is_none())).unwrap_or(false);
                if ready && since.elapsed().as_millis() > 700 {
                    self.welcome_pending = None;
                    if act == Act::Hints {
                        self.term_hints_open();
                    } else {
                        self.term_search_open();
                    }
                }
            }
            Act::Reader => {
                let loaded = self.tabs.get(self.active).and_then(|t| match (&t.left, &t.right) {
                    (Pane::Web(w), _) | (_, Some(Pane::Web(w))) => Some(!w.tab.shared.borrow().loading && w.reader.is_none()),
                    _ => None,
                }).unwrap_or(false);
                if loaded && since.elapsed().as_millis() > 900 {
                    self.welcome_pending = None;
                    self.toggle_reader();
                }
            }
            _ => self.welcome_pending = None,
        }
    }
}

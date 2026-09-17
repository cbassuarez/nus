//! A block as a page: the command as a heading, its output as pre, a caps
//! dateline with cwd · shell · time · exit. Written to profile/blocks/ as
//! HTML in the Broadsheet look and opened beside the shell; the tab's
//! tools row offers copy-as-markdown and a gist through `gh`.

use std::path::PathBuf;
use std::time::SystemTime;

use nus_render::theme::Theme;

#[derive(Clone, Debug)]
pub struct BlockPage {
    pub cmd: String,
    pub output: String,
    pub cwd: String,
    pub exit: Option<i32>,
    pub lines: u64,
    pub when: SystemTime,
    pub shell: String,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn hex(c: nus_render::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

impl BlockPage {
    pub fn markdown(&self) -> String {
        let status = match self.exit {
            Some(0) => "ok".to_string(),
            Some(c) => format!("exit {c}"),
            None => "".to_string(),
        };
        format!(
            "```\n$ {}\n{}```\n\n_{} · {} · {}_\n",
            self.cmd.trim(),
            if self.output.ends_with('\n') { self.output.clone() } else { format!("{}\n", self.output) },
            self.cwd,
            self.shell,
            status
        )
    }

    pub fn html(&self, theme: &Theme, signal: nus_render::Color) -> String {
        let (paper, ink, dim) = (hex(theme.paper), hex(theme.ink), hex(theme.dim));
        let sig = hex(signal);
        let lamp = match self.exit {
            Some(0) => format!("<span class=\"lamp ok\"></span>ok"),
            Some(c) => format!("<span class=\"lamp bad\"></span>exit {c}"),
            None => "<span class=\"lamp\"></span>".to_string(),
        };
        let when = humantime(self.when);
        format!(
            r#"<!doctype html><html><head><meta charset="utf-8"><title>{title}</title>
<style>
:root {{ --paper:{paper}; --ink:{ink}; --dim:{dim}; --signal:{sig}; }}
html,body {{ margin:0; background:var(--paper); color:var(--ink); }}
body {{ font-family:"IBM Plex Mono","Cascadia Mono",Consolas,monospace; font-size:13px; line-height:1.5; padding:28px 34px; }}
.band {{ position:fixed; left:0; top:0; right:0; height:6px; background:var(--signal); }}
h1 {{ font-family:"Newsreader","Times New Roman",serif; font-style:italic; font-weight:400; font-size:30px; margin:8px 0 6px; letter-spacing:0; }}
h1 .dollar {{ color:var(--dim); font-style:normal; font-family:inherit; }}
.dateline {{ font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); display:flex; gap:14px; align-items:center; border-top:1.5px solid var(--ink); border-bottom:1px solid var(--ink); padding:8px 0; margin:10px 0 18px; }}
.lamp {{ display:inline-block; width:8px; height:8px; background:var(--dim); margin-right:6px; vertical-align:-1px; }}
.lamp.ok {{ background:#2e7d32; }} .lamp.bad {{ background:var(--signal); }}
pre {{ margin:0; white-space:pre-wrap; word-break:break-word; }}
.foot {{ margin-top:22px; border-top:1px solid var(--ink); padding-top:8px; font-size:11px; letter-spacing:.08em; text-transform:uppercase; color:var(--dim); }}
</style></head><body>
<div class="band"></div>
<h1><span class="dollar">$ </span>{cmd}</h1>
<div class="dateline"><span>{cwd}</span><span>·</span><span>{shell}</span><span>·</span><span>{when}</span><span>·</span><span>{lamp}</span><span>·</span><span>{lines} lines</span></div>
<pre>{out}</pre>
<div class="foot">a block from nus · copy as markdown and gist in the tab's tools</div>
</body></html>"#,
            title = esc(self.cmd.trim()),
            cmd = esc(self.cmd.trim()),
            cwd = esc(&self.cwd),
            shell = esc(&self.shell),
            when = when,
            lamp = lamp,
            lines = self.lines,
            out = esc(&self.output),
        )
    }
}

fn humantime(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Local time without a tz crate: the OS's `date` is overkill; show UTC clock.
    let (h, mi) = ((secs / 3600) % 24, (secs / 60) % 60);
    format!("{h:02}:{mi:02} utc")
}

/// Write the page under profile/blocks/ and return its path.
pub fn write(html: &str) -> Option<PathBuf> {
    let dir = std::env::current_dir().ok()?.join("profile").join("blocks");
    std::fs::create_dir_all(&dir).ok()?;
    let name = format!("{}.html", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).ok()?.as_millis());
    let path = dir.join(name);
    std::fs::write(&path, html).ok()?;
    Some(path)
}

/// `gh gist create` on the markdown; the URL comes back on stdout.
pub fn gist(md: &str) -> Result<String, String> {
    let dir = std::env::temp_dir();
    let path = dir.join("nus-block.md");
    std::fs::write(&path, md).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("gh").args(["gist", "create", "--public=false", "-f", "block.md"]).arg(&path).output().map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_and_html_shape() {
        let p = BlockPage { cmd: "ls -la".into(), output: "a\nb\n".into(), cwd: "/x".into(), exit: Some(0), lines: 2, when: SystemTime::UNIX_EPOCH, shell: "bash".into() };
        let md = p.markdown();
        assert!(md.starts_with("```\n$ ls -la\na\nb\n```"));
        assert!(md.contains("/x · bash · ok"));
        let h = p.html(&Theme::ink(), [1.0, 0.0, 0.0, 1.0]);
        assert!(h.contains("<h1><span class=\"dollar\">$ </span>ls -la</h1>"));
        assert!(h.contains("lamp ok"));
        let p2 = BlockPage { exit: Some(2), output: "<b>".into(), ..p };
        assert!(p2.html(&Theme::paper(), [0.0; 4]).contains("&lt;b&gt;"));
        assert!(p2.markdown().contains("exit 2"));
    }
}

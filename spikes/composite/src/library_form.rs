//! The list's add/edit surface. Drafts are pane-local; cancel never writes.
use super::*;
#[derive(Clone, Default)]
pub struct Draft {
    pub target: Option<(String, u64)>,
    pub title: String,
    pub source: String,
    pub notes: String,
    pub error: String,
}
impl App {
    pub(crate) fn library_edit(&mut self, id: Option<&str>) {
        if crate::private::enabled() {
            return;
        }
        self.library.flush(true);
        let draft = if let Some(id) = id {
            let e = match self.library.store.read(id) {
                Ok(e) if !e.deleted => e,
                _ => {
                    self.library_message("This item is no longer available.");
                    return;
                }
            };
            Draft {
                target: Some((e.id, e.revision)),
                title: e.title,
                source: if e.source.starts_with("note:") {
                    String::new()
                } else {
                    e.source
                },
                notes: e
                    .extra
                    .get("user_notes")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                error: String::new(),
            }
        } else {
            Draft::default()
        };
        self.open_library();
        if let Some(h) = self.library_home_mut() {
            h.reading = None;
            h.library_scroll = 0.0;
            h.library_ui.draft = Some(draft);
            h.library_ui.focus = Some(Hit::Field(0));
        }
        self.dirty = true;
    }
    fn library_save_draft(&mut self) {
        let Some(d) = self.library_home().and_then(|h| h.library_ui.draft.clone()) else {
            return;
        };
        let result = (|| -> Result<Entry, String> {
            if d.title.trim().is_empty() {
                return Err("Enter a title.".into());
            }
            let source = if d.source.trim().is_empty() {
                if d.notes.trim().is_empty() {
                    return Err("Enter a link or some text to save.".into());
                }
                d.target
                    .as_ref()
                    .and_then(|(id, _)| self.library.store.read(id).ok())
                    .filter(|e| e.source.starts_with("note:"))
                    .map(|e| e.source)
                    .unwrap_or_else(|| {
                        format!(
                            "note:{}",
                            store::id(&format!(
                                "{}:{}:{}",
                                d.title,
                                crate::journal::now(),
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_nanos()
                            ))
                        )
                    })
            } else {
                let raw = d.source.trim();
                let u = if Path::new(raw).is_absolute() {
                    url::Url::from_file_path(raw).map_err(|_| "Invalid file path")?
                } else {
                    url::Url::parse(raw)
                        .map_err(|_| "Enter a full https:// link or an absolute file path.")?
                };
                if !matches!(u.scheme(), "http" | "https" | "file")
                    || !u.username().is_empty()
                    || u.password().is_some()
                {
                    return Err(
                        "Use a web link or local file, without embedded credentials.".into(),
                    );
                }
                if u.scheme() == "file" && u.to_file_path().is_err() {
                    return Err("Use a local file path.".into());
                }
                u.to_string()
            };
            self.library
                .store
                .save_item(
                    d.target.as_ref().map(|(id, r)| (id.as_str(), *r)),
                    &source,
                    &d.title,
                    &d.notes,
                    crate::journal::now(),
                )
                .map_err(|e| e.to_string())
        })();
        match result {
            Ok(e) => {
                self.library.remember(e);
                if let Some(h) = self.library_home_mut() {
                    h.library_ui.draft = None;
                    h.reading = None;
                    h.input.clear();
                    h.library_ui.filter = 1;
                    h.library_ui.focus = Some(Hit::Search);
                }
                self.library_message("Saved to your reading list.");
            }
            Err(error) => {
                if let Some(d) = self
                    .library_home_mut()
                    .and_then(|h| h.library_ui.draft.as_mut())
                {
                    d.error = error;
                }
            }
        }
        self.dirty = true;
    }
    pub(super) fn library_form_action(&mut self, hit: &Hit) -> bool {
        match hit {
            Hit::Add => self.library_edit(None),
            Hit::SaveDraft => self.library_save_draft(),
            Hit::CancelDraft => {
                if let Some(h) = self.library_home_mut() {
                    h.library_ui.draft = None;
                    h.library_ui.focus = Some(Hit::Search);
                }
            }
            Hit::Field(_) => {
                if let Some(h) = self.library_home_mut() {
                    h.library_ui.focus = Some(hit.clone());
                }
            }
            _ => return false,
        }
        self.dirty = true;
        true
    }
    pub(crate) fn library_form_key(&mut self, ev: &crate::app::KeyIn) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if self
            .library_home()
            .is_none_or(|h| h.library_ui.draft.is_none())
        {
            return false;
        }
        let command = crate::field::command(self.mods);
        let shift = self.mods.shift_key();
        let mods = self.mods;
        let focus = self
            .library_home()
            .and_then(|h| h.library_ui.focus.clone())
            .unwrap_or(Hit::Field(0));
        let order = [
            Hit::Field(0),
            Hit::Field(1),
            Hit::Field(2),
            Hit::SaveDraft,
            Hit::CancelDraft,
        ];
        match &ev.logical_key {
            Key::Named(NamedKey::Escape) => {
                self.library_form_action(&Hit::CancelDraft);
            }
            Key::Named(NamedKey::Tab) => {
                let i = order.iter().position(|h| *h == focus).unwrap_or(0);
                let i = (i + if shift { order.len() - 1 } else { 1 }) % order.len();
                if let Some(h) = self.library_home_mut() {
                    h.library_ui.focus = Some(order[i].clone());
                }
            }
            Key::Named(NamedKey::Enter) if command || focus == Hit::SaveDraft => {
                self.library_save_draft()
            }
            Key::Named(NamedKey::Enter) if focus == Hit::CancelDraft => {
                self.library_form_action(&Hit::CancelDraft);
            }
            Key::Named(NamedKey::Enter) if focus != Hit::Field(2) => {
                let i = order.iter().position(|h| *h == focus).unwrap_or(0);
                if let Some(h) = self.library_home_mut() {
                    h.library_ui.focus = Some(order[(i + 1) % order.len()].clone());
                }
            }
            _ => {
                if let Hit::Field(i) = focus {
                    if let Some(d) = self
                        .library_home_mut()
                        .and_then(|h| h.library_ui.draft.as_mut())
                    {
                        let text = match i {
                            0 => &mut d.title,
                            1 => &mut d.source,
                            _ => &mut d.notes,
                        };
                        if i == 2 && matches!(&ev.logical_key, Key::Named(NamedKey::Enter)) {
                            if text.len() < 24000 {
                                text.push('\n');
                            }
                        } else if i == 2
                            && command
                            && matches!(&ev.logical_key,Key::Character(c) if c.eq_ignore_ascii_case("v"))
                        {
                            if let Ok(mut cb) = arboard::Clipboard::new() {
                                if let Ok(paste) = cb.get_text() {
                                    for c in paste
                                        .chars()
                                        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
                                    {
                                        if text.len() + c.len_utf8() > 24000 {
                                            break;
                                        }
                                        text.push(c);
                                    }
                                }
                            }
                        } else {
                            crate::field::edit(text, ev, mods, if i == 2 { 6000 } else { 2000 });
                        }
                        d.error.clear();
                    }
                }
            }
        }
        self.dirty = true;
        true
    }
    pub(super) fn draw_library_form(&mut self, scene: &mut Scene, h: &mut HomePane) {
        let Some(d) = h.library_ui.draft.clone() else {
            return;
        };
        let r = h.rect;
        let scale = self.scale;
        let px = |v: f32| v * scale;
        let width = px(760.0).min((r.w - px(40.0)).max(1.0));
        let x = r.x + (r.w - width) / 2.0;
        let label = self.label();
        let ink = self.theme.ink;
        let dim = self.theme.dim;
        let mut y = r.y + px(28.0) - h.library_scroll;
        self.fonts.draw(
            scene,
            self.ui_strong(),
            x,
            y,
            if d.target.is_some() {
                "Edit reading item"
            } else {
                "Add to reading list"
            },
        );
        y += px(12.0);
        y = self.library_controls(
            scene,
            h,
            &[
                (Hit::SaveDraft, "Save".into()),
                (Hit::CancelDraft, "Cancel".into()),
            ],
            y,
        );
        for (i, (name, text)) in [
            ("Title", d.title.as_str()),
            (
                "Link or file path · leave empty for a note",
                d.source.as_str(),
            ),
            ("Text / personal notes", d.notes.as_str()),
        ]
        .into_iter()
        .enumerate()
        {
            self.fonts.draw(
                scene,
                Style {
                    color: dim,
                    ..label
                },
                x,
                y + px(14.0),
                name,
            );
            y += px(23.0);
            let height = if i == 2 {
                (r.bottom() - y - px(82.0)).clamp(px(42.0), px(260.0))
            } else {
                px(40.0)
            };
            let field = Rect::new(x, y, width, height).intersect(&r);
            let focused = h.library_ui.focus == Some(Hit::Field(i as u8));
            scene.outline(
                field,
                px(1.0),
                if focused { self.surface.signal } else { dim },
            );
            let old = scene.clip();
            scene.layer(Some(field));
            let st = Style {
                color: ink,
                ..self.ui()
            };
            let lines: Vec<String> = if i == 2 {
                text.split('\n')
                    .flat_map(|l| {
                        let w =
                            crate::reader::wrap(&self.fonts, st, l, (width - px(20.0)).max(1.0));
                        if w.is_empty() {
                            vec![String::new()]
                        } else {
                            w
                        }
                    })
                    .collect()
            } else {
                vec![self.fit(st, text, (width - px(20.0)).max(1.0))]
            };
            let count = ((height - px(10.0)) / px(23.0)).floor().max(1.0) as usize;
            for (j, line) in lines
                .iter()
                .skip(lines.len().saturating_sub(count))
                .enumerate()
            {
                self.fonts.draw(
                    scene,
                    st,
                    x + px(10.0),
                    y + px(25.0) + j as f32 * px(23.0),
                    line,
                );
            }
            if focused {
                let last = lines.last().map(String::as_str).unwrap_or("");
                let cx = (x + px(10.0) + self.fonts.measure(st, last)).min(field.right() - px(6.0));
                let cy = y + px(8.0) + lines.len().min(count).saturating_sub(1) as f32 * px(23.0);
                scene.vline(cx, cy, px(21.0), px(1.0), self.surface.signal);
            }
            scene.layer(old);
            if field.h > 0.0 {
                h.library_ui.hits.push((field, Hit::Field(i as u8)));
            }
            y += height + px(12.0);
        }
        h.library_reach = (y + px(72.0) + h.library_scroll - r.bottom()).max(0.0);
        let message = if d.error.is_empty() {
            "Save: Ctrl/⌘+Enter · Cancel: Esc · Tab moves between fields"
        } else {
            &d.error
        };
        for (i, line) in crate::reader::wrap(&self.fonts, label, message, width)
            .into_iter()
            .enumerate()
        {
            self.fonts.draw(
                scene,
                Style {
                    color: if d.error.is_empty() {
                        dim
                    } else {
                        self.surface.signal
                    },
                    ..label
                },
                x,
                y + px(14.0) + i as f32 * px(20.0),
                &line,
            );
        }
    }
}

//! Personal reading in the EXISTING Home-based library and native reader.
//! Existing serialized Entry/Article files remain readable. No second library.
#[path = "library_store.rs"]
pub(crate) mod store;
#[path = "library_access.rs"]
mod access;
use std::{collections::BTreeMap, path::{Path, PathBuf}, time::Instant};
use nus_render::{Rect, Scene, text::Style};
use crate::{app::{App, Pane}, home::HomePane, reader::{Article, Reader, Block}};
pub use store::Entry;
use store::{Position, Store, MAX_TEXT};
use crate::reader::interaction::{Hit as TextHit, Picture};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hit {
    Search, Filter(u8), Row(String), Back, Original, Finished, Archive,
    Refresh, Remove, ConfirmRemove, CancelRemove, Undo, Find, Copy,
    Smaller, Larger, Link(String), Code(usize),
}
#[derive(Default)]
pub struct Ui {
    pub hits: Vec<(Rect, Hit)>,
    pub focus: Option<Hit>,
    pub filter: u8,
    pub confirm: bool,
    pub selected: Option<String>,
    pub reveal: bool,
    pub size: f32,
}
pub struct Library {
    pub entries: BTreeMap<String, Entry>,
    loaded: bool,
    pending: Vec<Pending>,
    dirty: BTreeMap<String, (Instant, Position)>,
    store: Store,
    last_scan: Instant,
    serial: u64,
    scan: Option<(u64, std::sync::mpsc::Receiver<ScanResult>)>,
    pub status: String,
    undo: Option<(String, u64)>,
    pub(crate) access_ids: BTreeMap<String, u64>,
    pub(crate) access_map: BTreeMap<u64, Hit>,
}
type ScanResult = std::io::Result<(BTreeMap<String, Entry>, Vec<String>)>;
struct Pending {
    tab: u64, right: bool, request: i32, entry: String, ticket: String,
    source: String, container: Option<String>, since: Instant,
    shared: std::rc::Weak<std::cell::RefCell<crate::browser::Shared>>,
    verified: Option<(Article, f64, String)>,
}
pub struct Reading {
    pub id: String,
    pub reader: Reader,
    resume: Option<Position>,
    version: String,
    available: bool,
    pub note: String,
}
impl Default for Library {
    fn default() -> Self {
        Self { entries: BTreeMap::new(), loaded: false, pending: vec![], dirty: BTreeMap::new(),
            store: Store::new(PathBuf::from("profile/library")), last_scan: crate::clock::now(), serial: 0, scan: None,
            status: String::new(), undo: None, access_ids: BTreeMap::new(), access_map: BTreeMap::new() }
    }
}
impl Library {
    /// Only scans run off-thread. The worker owns no App, browser or renderer,
    /// and never writes. Small, serialized record mutations retain explicit
    /// success/failure acknowledgement through Store.
    pub fn reload(&mut self) {
        if crate::private::enabled() || self.scan.is_some() { return; }
        let store=self.store.clone();let (tx,rx)=std::sync::mpsc::sync_channel(1);
        match std::thread::Builder::new().name("nus-library-scan".into()).spawn(move || {let _=tx.send(store.list());}) {
            Ok(_)=>{self.scan=Some((self.serial,rx));if !self.loaded{self.status="Loading reading library…".into();}},
            Err(e)=>self.status=format!("Could not start the library scan: {e}"),
        }
        self.last_scan=crate::clock::now();
    }
    fn scan_ready(&mut self)->bool {
        let Some((serial,rx))=&self.scan else{return false;};
        let serial=*serial;
        let result=match rx.try_recv(){
            Ok(value)=>value,
            Err(std::sync::mpsc::TryRecvError::Empty)=>return false,
            Err(_)=>Err(std::io::Error::other("Library scan stopped unexpectedly")),
        };
        self.scan=None;self.last_scan=crate::clock::now();
        // A save/refresh/state mutation happened after this scan started. Do
        // not publish its older view over acknowledged writes; retry later.
        if serial!=self.serial{return false;}
        match result {
            Ok((mut entries,errors))=>{
                for (key,(_,pos)) in &self.dirty {
                    if let Some(e)=entries.get_mut(key).filter(|e|!e.deleted){
                        e.progress=pos.fraction;e.anchor=pos.quote.clone();e.position=Some(pos.clone());
                    }
                }
                let changed=!self.loaded||entries!=self.entries||!errors.is_empty();
                self.entries=entries;self.loaded=true;
                if !errors.is_empty(){self.status=format!("{} unreadable record(s); original files left untouched. {}",errors.len(),errors[0]);}
                else if self.status=="Loading reading library…"{self.status.clear();}
                changed
            },
            Err(e)=>{self.status=format!("Could not read the library; previous view retained: {e}");true},
        }
    }
    fn ensure(&mut self) { if !self.loaded { self.reload(); } }
    fn remember(&mut self, e: Entry) { self.serial=self.serial.wrapping_add(1);self.entries.insert(e.id.clone(), e); }
    pub(crate) fn flush(&mut self, force: bool) {
        if crate::private::enabled() { return; }
        let dirty = std::mem::take(&mut self.dirty);
        for (key, (since, p)) in dirty {
            if !force && crate::clock::since(since).as_secs_f32() < 0.75 { self.dirty.insert(key, (since, p)); continue; }
            match self.store.progress(&key, &p) {
                Ok(e) => self.remember(e),
                Err(e) => {
                    self.status = format!("Reading position was not saved: {e}");
                    // A removed or refreshed record must not be resurrected by
                    // a reader still showing an older copy. I/O failures retry.
                    let stale = self.store.read(&key).ok().is_some_and(|e| e.deleted || e.snapshot.as_ref().is_some_and(|h| h != &p.snapshot));
                    if !stale { self.dirty.insert(key, (crate::clock::now(), p)); }
                },
            }
        }
    }
}
impl Drop for Library { fn drop(&mut self) { self.flush(true); } }
fn container(name: &str) -> Option<String> { (name != crate::containers::PERSONAL).then(|| name.to_string()) }
fn hex_png(hex: &str) -> Result<(u32, u32, Vec<u8>), String> {
    if hex.len() > 131072 || hex.len() % 2 != 0 { return Err("Image exceeds the saved-image budget".into()); }
    let bytes: Vec<u8> = (0..hex.len()).step_by(2).map(|i| hex.get(i..i+2).and_then(|s| u8::from_str_radix(s, 16).ok()).ok_or("Invalid image encoding".to_string())).collect::<Result<_,_>>()?;
    if bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" { return Err("Invalid PNG".into()); }
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    if w == 0 || h == 0 || w > 2048 || h > 2048 || u64::from(w) * u64::from(h) > 1_048_576 { return Err("Image dimensions exceed the saved-image budget".into()); }
    Ok((w, h, bytes))
}

impl App {
    fn library_home(&self) -> Option<&HomePane> {
        match self.tabs.get(self.active)?.focused_ref() { Pane::Home(h) if h.library => Some(h), _ => None }
    }
    fn library_home_mut(&mut self) -> Option<&mut HomePane> {
        match self.tabs.get_mut(self.active)?.focused() { Pane::Home(h) if h.library => Some(h), _ => None }
    }
    pub(crate) fn library_over(&self, x: f32, y: f32) -> bool {
        self.tabs.get(self.active).is_some_and(|t| std::iter::once(&t.left).chain(t.right.as_ref()).any(|p| matches!(p, Pane::Home(h) if h.library && h.rect.contains(x,y))))
    }
    fn library_focus_at(&mut self, x: f32, y: f32) -> bool {
        let Some(t) = self.tabs.get_mut(self.active) else { return false; };
        for right in [false, true] {
            let p = if right { t.right.as_ref() } else { Some(&t.left) };
            if matches!(p, Some(Pane::Home(h)) if h.library && h.rect.contains(x,y)) { t.focus_right = right; return true; }
        }
        false
    }
    fn library_message(&mut self, text: impl Into<String>) {
        self.library.status = text.into();
        if self.library_home().is_none() { let text = self.library.status.clone(); self.notice(&text); }
        self.dirty = true;
    }
    pub(crate) fn open_library(&mut self) {
        if crate::private::enabled() { return; }
        self.library.ensure();
        for (i,t) in self.tabs.iter().enumerate() {
            let right = if matches!(&t.left, Pane::Home(h) if h.library) { Some(false) }
                else if matches!(&t.right, Some(Pane::Home(h)) if h.library) { Some(true) } else { None };
            if let Some(right) = right { self.tabs[i].focus_right = right; self.activate(i); return; }
        }
        let mut h = HomePane::new(); h.library = true; h.library_ui.focus = Some(Hit::Search);
        let mut tab = self.make_tab(Pane::Home(h), None); tab.name = Some("Reading library".into());
        self.tabs.push(tab); self.activate(self.tabs.len()-1); self.layout(); self.dirty = true;
    }
    pub(crate) fn save_reading(&mut self) { self.save_reading_mode(false); }
    pub(crate) fn refresh_reading(&mut self) { self.save_reading_mode(true); }
    fn save_reading_mode(&mut self, refresh: bool) {
        if crate::private::enabled() { self.library_message("Reading is not saved from an incognito window."); return; }
        self.library.ensure();
        if self.library.pending.len() >= 4 { self.library_message("Four captures are already in progress. Try again after one finishes."); return; }
        // Refresh from a saved copy only uses an already-open matching source.
        // It never opens the network behind the user's back.
        let reading = self.library_home().and_then(|h| h.reading.as_ref()).map(|r| r.id.clone());
        let target = if refresh {
            reading.as_ref().and_then(|id| self.library.entries.get(id)).and_then(|e| {
                self.tabs.iter().enumerate().find_map(|(i,t)| [(false,Some(&t.left)),(true,t.right.as_ref())].into_iter().find_map(|(right,p)| match p {
                    Some(Pane::Web(w)) if w.tab.shared.borrow().url == e.source && container(&w.container) == e.container => Some((i,right)), _ => None,
                }))
            })
        } else { None };
        if refresh && reading.is_some() && target.is_none() {
            self.library_message("Open the original first, then refresh from its open page. The saved copy is unchanged."); return;
        }
        let (i, right) = target.unwrap_or_else(|| (self.active, self.tabs.get(self.active).is_some_and(|t| t.focus_right && t.right.is_some())));
        let Some(t) = self.tabs.get(i) else { return; };
        let pane = if right { t.right.as_ref().unwrap_or(&t.left) } else { &t.left };
        let (source, title, context, direct) = match pane {
            Pane::Web(w) => {
                let s = w.tab.shared.borrow();
                if !url::Url::parse(&s.url).ok().is_some_and(|u| matches!(u.scheme(),"http"|"https"|"file") && u.username().is_empty() && u.password().is_none()) {
                    drop(s);
                    self.library_message("Only web pages and local text files can be saved for reading."); return;
                }
                (s.url.clone(), if s.title.is_empty() { s.url.clone() } else { s.title.clone() }, container(&w.container), None)
            },
            Pane::Editor(editor) => {
                let Some(b) = editor.buf() else { return; };
                if b.text.len_bytes() > MAX_TEXT { self.library_message("This file exceeds the 2 MiB reading limit."); return; }
                let title = editor.title(); let text = b.text.to_string();
                let source = b.path.as_ref().map(|p| format!("file:{}",p.display())).unwrap_or_else(|| format!("note:{}",store::id(&text)));
                (source, title.clone(), None, Some(Article { title, blocks: vec![Block::Pre(text)], ..Default::default() }))
            },
            _ => { self.library_message("Open a page or text file, then Save to reading library."); return; },
        };
        let tab_id = t.id;
        let saved = self.library.store.save_link(&source, &title, context.clone(), crate::journal::now());
        let (e, created) = match saved { Ok(v) => v, Err(e) => { self.library_message(format!("Could not save reading: {e}")); return; } };
        self.library.remember(e.clone());
        if !created && !refresh { self.library_message("Saved in the library. Any existing copy, reading position, and archive state were kept. Refresh is a separate action."); return; }
        let e = match self.library.store.start_capture(&e.id) { Ok(e) => e, Err(e) => { self.library_message(format!("Link saved; capture did not start: {e}")); return; } };
        let ticket = e.capture.clone().unwrap();
        self.library.remember(e.clone());
        if let Some(article) = direct { self.library_finish(&e.id, &ticket, article, "Text file snapshot. No network content was fetched."); return; }
        let Some(Pane::Web(w)) = self.tabs.get(i).and_then(|t| if right { t.right.as_ref() } else { Some(&t.left) }) else { return; };
        let expr = format!("({})({},({}))", include_str!("../assets/library/capture.js"), serde_json::to_string(&ticket).unwrap(), crate::reader::EXTRACT_JS);
        let request = w.tab.eval_reply(&expr);
        let shared = std::rc::Rc::downgrade(&w.tab.shared);
        self.library.pending.push(Pending { tab: tab_id, right, request, entry: e.id, ticket, source,
            container: context, since: crate::clock::now(), shared, verified: None });
        self.library_message("Link saved. Preparing an offline copy from this page; no additional image requests.");
    }
    fn library_finish(&mut self, key: &str, ticket: &str, article: Article, note: &str) {
        let result = serde_json::to_vec(&article).map_err(std::io::Error::other).and_then(|bytes| self.library.store.commit(key,ticket,&bytes,&article.title,article.words(),note));
        match result {
            Ok(e) => { self.library.remember(e); self.library_message("Saved copy is available offline. Missing images, if any, are identified in the article."); },
            Err(e) => { self.library_message(format!("Copy was not committed; any previous copy remains intact: {e}")); },
        }
    }
    fn library_fail(&mut self, p: &Pending, reason: &str) {
        if let Some(shared) = p.shared.upgrade() { shared.borrow_mut().replies.retain(|(id,_)| *id != p.request); }
        if let Ok(e) = self.library.store.capture_failed(&p.entry,&p.ticket,reason) { self.library.remember(e); }
        self.library_message(format!("Capture stopped: {reason}. The link and any previous copy were retained."));
    }
    pub(crate) fn poll_library(&mut self) {
        if crate::private::enabled() { return; }
        self.library.flush(false);
        if self.library.scan_ready() {
            let removed=self.library_home().and_then(|h|h.reading.as_ref()).is_some_and(|r|self.library.entries.get(&r.id).is_none_or(|e|e.deleted));
            if removed{if let Some(h)=self.library_home_mut(){h.reading=None;h.library_ui.confirm=false;}}
            self.dirty=true;
        }
        if self.library_home().is_some() && crate::clock::since(self.library.last_scan).as_secs()>=2 {
            self.library.reload();
        }
        for mut p in std::mem::take(&mut self.library.pending) {
            let source = self.tabs.iter().find(|t|t.id == p.tab).and_then(|t| if p.right {t.right.as_ref()} else {Some(&t.left)}).and_then(|pane|match pane {
                Pane::Web(w) if w.tab.shared.borrow().url == p.source && container(&w.container) == p.container
                    && p.shared.upgrade().is_some_and(|s| std::rc::Rc::ptr_eq(&s,&w.tab.shared)) => Some(&w.tab), _ => None,
            });
            let Some(source) = source else { self.library_fail(&p,"source page changed, moved, or closed"); continue; };
            let Some(reply) = source.take_reply(p.request) else {
                if crate::clock::since(p.since).as_secs() >= 10 { self.library_fail(&p,"page did not return a bounded article in time"); }
                else { self.library.pending.push(p); }
                continue;
            };
            let parsed = reply.pointer("/result/value").and_then(|v|v.as_str()).filter(|s|s.len() <= MAX_TEXT).and_then(|s|serde_json::from_str::<serde_json::Value>(s).ok());
            let Some(v) = parsed else { self.library_fail(&p,"page returned no readable article or exceeded the inspection limit"); continue; };
            if v["url"].as_str() != Some(p.source.as_str()) { self.library_fail(&p,"document address changed"); continue; }
            if let Some((article, epoch, note)) = p.verified.take() {
                if v["epoch"].as_f64() != Some(epoch) { self.library_fail(&p,"document was reloaded during capture"); continue; }
                self.library_finish(&p.entry,&p.ticket,article,&note);
                continue;
            }
            if v["token"].as_str() != Some(p.ticket.as_str()) || v["schema"].as_u64() != Some(1) { self.library_fail(&p,"capture identity did not match"); continue; }
            let Some(epoch) = v["epoch"].as_f64().filter(|n|n.is_finite()) else { self.library_fail(&p,"missing document identity"); continue; };
            let article = capture_article(&v);
            let (article,note) = match article { Ok(v) => v, Err(e) => { self.library_fail(&p,&e); continue; } };
            // A second read confirms a same-URL reload did not race extraction.
            p.request = source.eval_reply("JSON.stringify({url:location.href,epoch:performance.timeOrigin})");
            p.verified = Some((article,epoch,note));
            self.library.pending.push(p);
        }
    }
    pub(crate) fn library_rows(&self, input: &str) -> Vec<Entry> {
        let filter = self.library_home().map(|h|h.library_ui.filter).unwrap_or(0);
        self.library_rows_for(input,filter)
    }
    fn library_rows_for(&self,input:&str,filter:u8)->Vec<Entry> {
        let q = input.trim().to_lowercase();
        // Keep the old explicit `archive` search as a compatibility alias.
        let archived = q == "archive" || q.starts_with("archive ");
        let q = if archived {q.strip_prefix("archive").unwrap().trim()} else {q.as_str()};
        let words: Vec<_> = q.split_whitespace().collect();
        let mut rows: Vec<_> = self.library.entries.values().filter(|e| !e.deleted && match if archived {3} else {filter} {
            0 => !e.archived && !e.finished, 1 => !e.archived, 2 => !e.archived && e.finished, _ => e.archived,
        }).filter(|e| { let text = format!("{} {}",e.title,e.source).to_lowercase(); words.iter().all(|w|text.contains(w)) }).cloned().collect();
        rows.sort_by(|a,b| {
            let ongoing = |e:&Entry| filter == 0 && e.progress > 0.0 && !e.finished;
            ongoing(b).cmp(&ongoing(a)).then_with(||b.saved.cmp(&a.saved)).then_with(||a.id.cmp(&b.id))
        });
        rows
    }
    pub(crate) fn read_saved(&mut self,key:&str) {
        self.library.flush(true);
        self.library.ensure();
        let e = match self.library.store.read(key) { Ok(e) if !e.deleted => e, _ => { self.library_message("This reading item is no longer available."); return; } };
        let loaded = self.library.store.article(&e).and_then(|(bytes,hash)| serde_json::from_slice::<Article>(&bytes).map(|a|(a,hash)).map_err(std::io::Error::other));
        let (article,version,available,note) = match loaded {
            Ok((a,h)) => (a,h,true,if e.note.is_empty(){"Saved copy · available offline".into()}else{format!("Saved copy · {}",e.note)}),
            Err(err) => (Article{title:e.title.clone(),..Default::default()},String::new(),false,format!("No readable saved copy: {err}. Open original is an explicit action; nothing was fetched.")),
        };
        let mut reader = Reader::new(article); reader.saved.offline = true;
        let image_note = self.library_pictures(&mut reader);
        let resume = e.position.clone().or_else(|| (!e.anchor.is_empty() || e.progress > 0.0).then(||Position{quote:e.anchor.clone(),fraction:e.progress,..Default::default()}));
        self.library.remember(e.clone()); self.open_library();
        if let Some(h) = self.library_home_mut() {
            h.reading = Some(Reading{id:e.id,reader,resume,version,available,note:format!("{note}{image_note}")});
            h.library_ui.confirm = false; h.library_ui.focus = Some(Hit::Back);
        }
        self.dirty = true;
    }
    fn library_pictures(&mut self, reader:&mut Reader)->String {
        let mut missing=0; let mut pixels=0u64;
        for b in &reader.article.blocks {
            let Block::Image(_,src)=b else {continue;};
            if reader.saved.pictures.contains_key(src){continue;}
            let Some(hex)=src.strip_prefix("nus-png:") else {missing+=1;continue;};
            let result=(|| -> Result<Picture,String> {
                let (w,h,bytes)=hex_png(hex)?;
                if pixels+u64::from(w)*u64::from(h)>2_097_152{return Err("Image budget".into());}
                let mut decoder=png::Decoder::new(std::io::Cursor::new(bytes));
                decoder.set_limits(png::Limits{bytes:8*1024*1024});
                decoder.set_transformations(png::Transformations::EXPAND|png::Transformations::STRIP_16);
                let mut png=decoder.read_info().map_err(|e|e.to_string())?;
                let mut data=vec![0u8;png.output_buffer_size()];
                let info=png.next_frame(&mut data).map_err(|e|e.to_string())?;
                if info.width!=w||info.height!=h{return Err("Image dimensions changed".into());}
                let data=&data[..info.buffer_size()]; let mut bgra=Vec::with_capacity(w as usize*h as usize*4);
                match info.color_type {
                    png::ColorType::Rgba=>for p in data.chunks_exact(4){bgra.extend_from_slice(&[p[2],p[1],p[0],p[3]]);},
                    png::ColorType::Rgb=>for p in data.chunks_exact(3){bgra.extend_from_slice(&[p[2],p[1],p[0],255]);},
                    png::ColorType::Grayscale=>for &p in data{bgra.extend_from_slice(&[p,p,p,255]);},
                    png::ColorType::GrayscaleAlpha=>for p in data.chunks_exact(2){bgra.extend_from_slice(&[p[0],p[0],p[0],p[1]]);},
                    _=>return Err("Unsupported PNG".into()),
                }
                if bgra.len()!=w as usize*h as usize*4{return Err("Invalid image length".into());}
                let texture=self.device.create_texture(&wgpu::TextureDescriptor{label:Some("saved library image"),size:wgpu::Extent3d{width:w,height:h,depth_or_array_layers:1},mip_level_count:1,sample_count:1,dimension:wgpu::TextureDimension::D2,format:wgpu::TextureFormat::Bgra8Unorm,usage:wgpu::TextureUsages::TEXTURE_BINDING|wgpu::TextureUsages::COPY_DST,view_formats:&[]});
                self.gpu.queue.write_texture(wgpu::TexelCopyTextureInfo{texture:&texture,mip_level:0,origin:wgpu::Origin3d::ZERO,aspect:wgpu::TextureAspect::All},&bgra,wgpu::TexelCopyBufferLayout{offset:0,bytes_per_row:Some(w*4),rows_per_image:Some(h)},wgpu::Extent3d{width:w,height:h,depth_or_array_layers:1});
                pixels+=u64::from(w)*u64::from(h);
                Ok(Picture{width:w,height:h,texture:(self.bind_texture)(&texture)})
            })();
            match result{Ok(p)=>{reader.saved.pictures.insert(src.clone(),p);},Err(_)=>missing+=1}
        }
        if missing>0{format!(" · {missing} image(s) unavailable offline")}else{String::new()}
    }
    fn open_reading_source(&mut self,e:&Entry) {
        if let Ok(url)=url::Url::parse(&e.source) {
            if matches!(url.scheme(),"http"|"https") && url.username().is_empty() && url.password().is_none() {
                let name=e.container.as_deref().unwrap_or(crate::containers::PERSONAL);
                if !self.containers.iter().any(|c|c.name==name) {self.library_message("The source browser container is unavailable. No other sign-in context was substituted.");return;}
                if let Some(w)=self.new_web_pane_in(url.as_str(),name){let tab=self.make_tab(Pane::Web(w),None);self.tabs.push(tab);self.activate(self.tabs.len()-1);self.layout();}
                return;
            }
            if url.scheme()=="file" && e.source.starts_with("file://") {if let Ok(path)=url.to_file_path(){self.open_file(&path,false);return;}}
        }
        if let Some(path)=e.source.strip_prefix("file:"){self.open_file(Path::new(path),false);}
        else{self.library_message("The original is not a permitted web page or local file.");}
    }
    pub(crate) fn library_commit(&mut self) {
        let Some(h)=self.library_home() else{return;};
        let rows=self.library_rows_for(&h.input,h.library_ui.filter);
        let key=h.library_ui.selected.clone().filter(|id|rows.iter().any(|e|&e.id==id)).or_else(||rows.get(h.sel.saturating_sub(1)).map(|e|e.id.clone()));
        if let Some(key)=key{self.read_saved(&key);}
    }
    fn library_copy(&mut self,text:String) {
        if text.is_empty(){self.library_message("Select text first, or use a code block’s Copy button.");return;}
        match arboard::Clipboard::new().and_then(|mut cb|cb.set_text(text)) {Ok(())=>self.library_message("Copied."),Err(e)=>self.library_message(format!("Could not copy: {e}"))}
    }
    pub(crate) fn library_action(&mut self,hit:Hit) {
        self.library.flush(true);
        let id=self.library_home().and_then(|h|h.reading.as_ref()).map(|r|r.id.clone());
        let e=id.as_ref().and_then(|id|self.library.entries.get(id)).cloned();
        match hit {
            Hit::Row(id)=>self.read_saved(&id),
            Hit::Search=>{if let Some(h)=self.library_home_mut(){h.library_ui.focus=Some(Hit::Search);}},
            Hit::Filter(filter)=>{if let Some(h)=self.library_home_mut(){h.library_ui.filter=filter;h.library_ui.selected=None;h.library_scroll=0.0;h.sel=0;h.library_ui.focus=Some(Hit::Filter(filter));}},
            Hit::Back=>{if let Some(h)=self.library_home_mut(){h.reading=None;h.library_ui.confirm=false;h.library_ui.focus=Some(Hit::Search);}},
            Hit::Original=>if let Some(e)=e{self.open_reading_source(&e);},
            Hit::Refresh=>self.refresh_reading(),
            Hit::Finished|Hit::Archive=>if let Some(e)=e{
                let result=self.library.store.state(&e.id,if hit==Hit::Finished{Some(!e.finished)}else{None},if hit==Hit::Archive{Some(!e.archived)}else{None});
                match result{Ok(e)=>{self.library.remember(e);self.library_message("Reading state saved.");},Err(e)=>self.library_message(format!("State was not saved: {e}"))}
            },
            Hit::Remove=>{if let Some(h)=self.library_home_mut(){h.library_ui.confirm=true;h.library_ui.focus=Some(Hit::CancelRemove);if let Some(r)=h.reading.as_mut(){r.reader.saved.find=None;r.reader.saved.found=None;}}},
            Hit::CancelRemove=>{if let Some(h)=self.library_home_mut(){h.library_ui.confirm=false;h.library_ui.focus=Some(Hit::Remove);}},
            Hit::ConfirmRemove=>if let Some(e)=e{
                if !self.library_home().is_some_and(|h|h.library_ui.confirm){return;}
                match self.library.store.remove(&e.id){
                    Ok(e)=>{self.library.dirty.remove(&e.id);self.library.undo=Some((e.id.clone(),e.revision));self.library.remember(e);self.library_action(Hit::Back);self.library_message("Removed from the list. Undo is available. This is not a secure erase.");},
                    Err(err)=>self.library_message(format!("Removal failed: {err}")),
                }
            },
            Hit::Undo=>if let Some((id,rev))=self.library.undo.clone(){match self.library.store.undo_remove(&id,rev){Ok(e)=>{self.library.remember(e);self.library.undo=None;self.library_message("Restored to the reading library.");},Err(e)=>self.library_message(format!("Undo failed: {e}"))}},
            Hit::Find=>if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){r.reader.saved.find.get_or_insert_with(String::new);h.library_ui.focus=Some(Hit::Find);}},
            Hit::Copy=>{let text=self.library_home().and_then(|h|h.reading.as_ref()).map(|r|r.reader.reading_selected_text()).unwrap_or_default();self.library_copy(text);},
            Hit::Smaller|Hit::Larger=>if let Some(h)=self.library_home_mut(){h.library_ui.size=((if h.library_ui.size==0.0{1.0}else{h.library_ui.size})+if hit==Hit::Larger{0.1}else{-0.1}).clamp(0.8,1.8);},
            Hit::Code(index)=>{let text=self.library_home().and_then(|h|h.reading.as_ref()).and_then(|r|r.reader.saved.code.get(&index)).cloned().unwrap_or_default();self.library_copy(text);},
            Hit::Link(url)=>if let Some(mut e)=e{e.source=url;self.open_reading_source(&e);},
        }
        self.dirty=true;
    }
}

impl App {
    pub(crate) fn library_zoom_key(&mut self,ev:&crate::app::KeyIn)->bool {
        if !self.library_home().is_some_and(|h| h.reading.as_ref().is_some_and(|r|r.available)) {return false;}
        self.library_key(ev)
    }
    pub(crate) fn library_key(&mut self,ev:&crate::app::KeyIn)->bool {
        use winit::keyboard::{Key,NamedKey};
        if ev.state!=winit::event::ElementState::Pressed || self.library_home().is_none(){return false;}
        let modifiers=self.mods;
        let command=if cfg!(target_os="macos"){modifiers.super_key()}else{modifiers.control_key()};
        let character=match &ev.logical_key{Key::Character(s)=>s.to_lowercase(),_=>String::new()};
        let reading=self.library_home().is_some_and(|h|h.reading.is_some());
        // Confirmation owns unmodified input. Its initial focus is Cancel,
        // so repeated Enter cannot turn an accidental double-press into removal.
        if self.library_home().is_some_and(|h|h.library_ui.confirm) {
            if command||modifiers.control_key()||modifiers.super_key()||modifiers.alt_key(){return false;}
            match &ev.logical_key {
                Key::Named(NamedKey::Escape)=>self.library_action(Hit::CancelRemove),
                Key::Named(NamedKey::Tab)=>{if let Some(h)=self.library_home_mut(){h.library_ui.focus=Some(if h.library_ui.focus==Some(Hit::ConfirmRemove){Hit::CancelRemove}else{Hit::ConfirmRemove});}},
                Key::Named(NamedKey::Enter)=>{let hit=self.library_home().and_then(|h|h.library_ui.focus.clone());self.library_action(if hit==Some(Hit::ConfirmRemove){Hit::ConfirmRemove}else{Hit::CancelRemove});},
                _=>{},
            }
            self.dirty=true;return true;
        }
        // Do not steal app/window/browser chords except the reader's own.
        if command && !modifiers.alt_key() {
            if character=="f" {self.library_action(if reading{Hit::Find}else{Hit::Search});return true;}
            if reading && matches!(character.as_str(),"+"|"="|"-"|"0") {
                if character=="0" {if let Some(h)=self.library_home_mut(){h.library_ui.size=1.0;}self.dirty=true;}
                else {self.library_action(if character=="-"{Hit::Smaller}else{Hit::Larger});}
                return true;
            }
        }
        let find_focused=self.library_home().is_some_and(|h|h.reading.is_some() && h.library_ui.focus==Some(Hit::Find));
        if find_focused {
            if matches!(&ev.logical_key,Key::Named(NamedKey::Escape)) {
                if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){r.reader.saved.find=None;r.reader.saved.found=None;}h.library_ui.focus=Some(Hit::Back);}
                self.dirty=true;return true;
            }
            if matches!(&ev.logical_key,Key::Named(NamedKey::Enter)) {
                if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){r.reader.reading_find(!modifiers.shift_key());}}
                self.library_record_position();self.dirty=true;return true;
            }
            if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){
                let q=r.reader.saved.find.get_or_insert_with(String::new);
                let took=crate::field::edit(q,ev,modifiers,2000);
                if took.taken(){if took.changed(){r.reader.saved.found=None;r.reader.reading_find(true);}self.dirty=true;return true;}
            }}
        }
        if reading && command && !modifiers.alt_key() {
            if character=="c"{self.library_action(Hit::Copy);return true;}
            if character=="a"{if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){r.reader.reading_select_all();}}self.dirty=true;return true;}
        }
        if !reading && command && !modifiers.alt_key() && matches!(character.as_str(),"a"|"c"|"v"|"x") {
            if let Some(h)=self.library_home_mut(){let took=crate::field::edit(&mut h.input,ev,modifiers,2000);if took.taken(){if took.changed(){h.library_ui.selected=None;h.sel=0;h.library_scroll=0.0;}h.library_ui.focus=Some(Hit::Search);self.dirty=true;return true;}}
        }
        if command||modifiers.control_key()||modifiers.super_key()||modifiers.alt_key(){return false;}
        if matches!(&ev.logical_key,Key::Named(NamedKey::Tab)) {
            if let Some(h)=self.library_home_mut(){
                let controls:Vec<_>=h.library_ui.hits.iter().map(|(_,hit)|hit.clone()).collect();
                if !controls.is_empty(){let at=h.library_ui.focus.as_ref().and_then(|f|controls.iter().position(|v|v==f));
                    let next=if modifiers.shift_key(){at.unwrap_or(0).checked_sub(1).unwrap_or(controls.len()-1)}else{at.map(|i|(i+1)%controls.len()).unwrap_or(0)};
                    h.library_ui.focus=Some(controls[next].clone());
                }
            }
            self.dirty=true;return true;
        }
        if matches!(&ev.logical_key,Key::Named(NamedKey::Enter)) {
            let hit=self.library_home().and_then(|h|h.library_ui.focus.clone());
            match hit{Some(Hit::Search)|None if !reading=>self.library_commit(),Some(Hit::Search)|None=>{},Some(hit)=>self.library_action(hit)}
            return true;
        }
        if self.library_home().is_some_and(|h|h.library_ui.confirm) {
            if matches!(&ev.logical_key,Key::Named(NamedKey::Escape)){self.library_action(Hit::CancelRemove);}
            return true;
        }
        if reading {
            let page=self.library_home().and_then(|h|h.reading.as_ref()).and_then(|r|r.reader.saved.viewport).map(|r|r.h*0.8).unwrap_or(200.0);
            match &ev.logical_key {
                Key::Named(NamedKey::Escape)=>self.library_action(Hit::Back),
                Key::Named(NamedKey::ArrowDown)=>self.library_scroll(-48.0*self.scale),
                Key::Named(NamedKey::ArrowUp)=>self.library_scroll(48.0*self.scale),
                Key::Named(NamedKey::Space)=>self.library_scroll(if modifiers.shift_key(){page}else{-page}),
                Key::Named(NamedKey::PageDown)=>self.library_scroll(-page),
                Key::Named(NamedKey::PageUp)=>self.library_scroll(page),
                Key::Named(NamedKey::Home)=>self.library_scroll(f32::MAX),
                Key::Named(NamedKey::End)=>self.library_scroll(-f32::MAX),
                Key::Named(NamedKey::ArrowLeft|NamedKey::ArrowRight)=>{
                    let right=matches!(&ev.logical_key,Key::Named(NamedKey::ArrowRight));
                    let scale=self.scale;
                    if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){
                        if modifiers.shift_key(){r.reader.reading_move_caret(right,true);}else{
                            r.reader.saved.horizontal=(r.reader.saved.horizontal+if right{48.0*scale}else{-48.0*scale}).clamp(0.0,r.reader.saved.horizontal_max);
                        }
                    }}
                    self.dirty=true;
                },
                _=>{},
            }
            return true;
        }
        if matches!(&ev.logical_key,Key::Named(NamedKey::ArrowDown|NamedKey::ArrowUp)) {
            let (query,filter,selected)=self.library_home().map(|h|(h.input.clone(),h.library_ui.filter,h.library_ui.selected.clone())).unwrap();
            let rows=self.library_rows_for(&query,filter);
            let down=matches!(&ev.logical_key,Key::Named(NamedKey::ArrowDown));
            let at=selected.as_ref().and_then(|id|rows.iter().position(|e|&e.id==id));
            let next=if down{at.map(|i|(i+1).min(rows.len().saturating_sub(1))).unwrap_or(0)}else{at.unwrap_or(0).saturating_sub(1)};
            if let Some(e)=rows.get(next){if let Some(h)=self.library_home_mut(){h.library_ui.selected=Some(e.id.clone());h.library_ui.focus=Some(Hit::Row(e.id.clone()));h.library_ui.reveal=true;h.sel=next+1;}}
            self.dirty=true;return true;
        }
        if matches!(&ev.logical_key,Key::Named(NamedKey::Escape)) {
            if let Some(h)=self.library_home_mut(){h.input.clear();h.sel=0;h.library_ui.selected=None;h.library_scroll=0.0;h.library_ui.focus=Some(Hit::Search);}
            self.dirty=true;return true;
        }
        if let Some(h)=self.library_home_mut(){
            let took=crate::field::edit(&mut h.input,ev,modifiers,2000);
            if took.taken(){if took.changed(){h.sel=0;h.library_ui.selected=None;h.library_scroll=0.0;}h.library_ui.focus=Some(Hit::Search);self.dirty=true;return true;}
        }
        false
    }
    fn library_record_position(&mut self) {
        let position=self.library_home().and_then(|h|h.reading.as_ref()).filter(|r|r.available).and_then(|r|r.reader.reading_position(&r.version).map(|p|(r.id.clone(),p)));
        if let Some((id,p))=position {
            if let Some(e)=self.library.entries.get_mut(&id){e.progress=p.fraction;e.anchor=p.quote.clone();e.position=Some(p.clone());}
            self.library.dirty.insert(id,(crate::clock::now(),p));
        }
    }
    pub(crate) fn library_scroll(&mut self,dy:f32) {
        if !dy.is_finite(){return;}
        if let Some(h)=self.library_home_mut(){
            if let Some(r)=h.reading.as_mut(){let height=r.reader.saved.viewport.map(|p|p.h).unwrap_or(h.rect.h);let max=(r.reader.height-height).max(0.0);r.reader.scroll=(r.reader.scroll-dy).clamp(0.0,max);}
            else{h.library_scroll=(h.library_scroll-dy).clamp(0.0,h.library_reach);h.library_ui.reveal=false;}
        }
        self.library_record_position();self.dirty=true;
    }
    pub(crate) fn library_wheel(&mut self,x:f32,y:f32,dx:f32,dy:f32)->bool {
        if !self.library_focus_at(x,y){return false;}
        if self.mods.shift_key()||dx.abs()>dy.abs(){
            let delta=if self.mods.shift_key() && dx.abs()<1.0{dy}else{dx};
            if delta.is_finite(){if let Some(h)=self.library_home_mut(){if let Some(r)=h.reading.as_mut(){r.reader.saved.horizontal=(r.reader.saved.horizontal-delta).clamp(0.0,r.reader.saved.horizontal_max);}}}
            self.dirty=true;
        }else{self.library_scroll(dy);}
        true
    }
    pub(crate) fn library_click(&mut self,x:f32,y:f32)->bool {
        if !self.library_focus_at(x,y){return false;}
        let hit=self.library_home().and_then(|h|h.library_ui.hits.iter().find(|(r,_)|r.contains(x,y))).map(|(_,h)|h.clone());
        if let Some(hit)=hit {
            if let Some(h)=self.library_home_mut(){h.library_ui.focus=Some(hit.clone());if let Hit::Row(id)=&hit{h.library_ui.selected=Some(id.clone());}}
            self.library_action(hit);return true;
        }
        if self.library_home().is_some_and(|h|h.library_ui.confirm){return true;}
        let fonts=&self.fonts;
        if let Some(t)=self.tabs.get_mut(self.active){if let Pane::Home(h)=t.focused(){if let Some(r)=h.reading.as_mut(){
            if r.reader.saved.viewport.is_some_and(|r|r.contains(x,y)) {
                if let Some(point)=r.reader.reading_point(fonts,x,y){r.reader.saved.selection=Some((point,point));r.reader.saved.dragging=true;h.library_ui.focus=None;}
            }
        }}}
        self.dirty=true;true
    }
    pub(crate) fn library_mouse(&mut self,button:winit::event::MouseButton,state:winit::event::ElementState,x:f32,y:f32)->bool {
        if button!=winit::event::MouseButton::Left{return false;}
        if state==winit::event::ElementState::Pressed{return self.library_click(x,y);}
        let mut dragged=false;
        for t in &mut self.tabs{for p in std::iter::once(&mut t.left).chain(t.right.as_mut()){
            if let Pane::Home(h)=p{if let Some(r)=h.reading.as_mut(){dragged|=r.reader.saved.dragging;r.reader.saved.dragging=false;}}
        }}
        dragged
    }
    pub(crate) fn library_pointer(&mut self,x:f32,y:f32)->bool {
        let fonts=&self.fonts;
        let Some(t)=self.tabs.get_mut(self.active) else{return false;};
        let Pane::Home(h)=t.focused() else{return false;};
        let Some(r)=h.reading.as_mut() else{return false;};
        if !r.reader.saved.dragging{return false;}
        if let Some(point)=r.reader.reading_point(fonts,x,y){let start=r.reader.saved.selection.map(|(a,_)|a).unwrap_or(point);r.reader.saved.selection=Some((start,point));}
        self.dirty=true;true
    }
    pub(crate) fn library_label(&self,hit:&Hit)->String {
        let e=self.library_home().and_then(|h|h.reading.as_ref()).and_then(|r|self.library.entries.get(&r.id));
        match hit {
            Hit::Search=>"Search title or source".into(),Hit::Filter(0)=>"Unfinished".into(),Hit::Filter(1)=>"All saved".into(),Hit::Filter(2)=>"Finished".into(),Hit::Filter(_)=>"Archived".into(),
            Hit::Row(id)=>self.library.entries.get(id).map(|e|format!("Open saved {}",e.title)).unwrap_or_else(||"Open saved article".into()),
            Hit::Back=>"Library".into(),Hit::Original=>"Open original".into(),Hit::Finished=>if e.is_some_and(|e|e.finished){"Mark unfinished"}else{"Mark finished"}.into(),
            Hit::Archive=>if e.is_some_and(|e|e.archived){"Unarchive"}else{"Archive"}.into(),Hit::Refresh=>"Refresh saved copy".into(),Hit::Remove=>"Remove…".into(),Hit::ConfirmRemove=>"Confirm removal".into(),Hit::CancelRemove=>"Keep article".into(),Hit::Undo=>"Undo removal".into(),Hit::Find=>"Find in article".into(),Hit::Copy=>"Copy selection".into(),Hit::Smaller=>"A−".into(),Hit::Larger=>"A+".into(),Hit::Link(u)=>format!("Open link: {u}"),Hit::Code(_)=>"Copy code or table".into(),
        }
    }
    fn library_controls(&mut self,scene:&mut Scene,h:&mut HomePane,controls:&[(Hit,String)],top:f32)->f32 {
        let r=h.rect;let scale=self.scale;let px=|v:f32|v*scale;let gap=px(8.0);let left=r.x+px(16.0);let right=r.right()-px(16.0);let mut x=left;let mut y=top;
        let label=self.label();let ink=self.theme.ink;let dim=self.theme.dim;
        for (hit,text) in controls {
            let width=(self.fonts.measure(label,text)+px(20.0)).min((right-left).max(1.0));
            if x>left && x+width>right{y+=px(44.0);x=left;}
            let cell=Rect::new(x,y,width,px(40.0));
            let focused=h.library_ui.focus.as_ref()==Some(hit);let selected=matches!(hit,Hit::Filter(f) if *f==h.library_ui.filter);
            if selected||cell.contains(self.mouse.0,self.mouse.1){scene.rect(cell,self.theme.tint);}
            if focused{scene.outline(cell,px(1.5),self.surface.signal);}
            let text=self.fit(label,text,(width-px(12.0)).max(1.0));
            self.fonts.draw(scene,Style{color:if selected||focused{ink}else{dim},..label},x+px(8.0),y+px(26.0),&text);
            let clipped=cell.intersect(&r);if clipped.w>0.0&&clipped.h>0.0{h.library_ui.hits.push((clipped,hit.clone()));}
            x+=width+gap;
        }
        y+px(48.0)
    }
    pub(crate) fn draw_library(&mut self,scene:&mut Scene,h:&mut HomePane) {
        let outer=scene.clip();let r=h.rect;let scale=self.scale;let px=|v:f32|v*scale;
        let ink=self.theme.ink;let dim=self.theme.dim;let paper=self.paper();let label=self.label();
        scene.layer(Some(r));scene.rect(r,paper);h.hits.clear();h.keys.clear();h.library_ui.hits.clear();
        if h.library_ui.size==0.0{h.library_ui.size=1.0;}
        let current=h.reading.as_ref().and_then(|rd|self.library.entries.get(&rd.id)).cloned();
        let mut y=r.y+px(12.0);
        if h.reading.is_some() {
            let mut controls=if h.library_ui.confirm{vec![(Hit::CancelRemove,"Keep article".into()),(Hit::ConfirmRemove,"Confirm removal".into())]}else{
                vec![(Hit::Back,"← Library".into()),(Hit::Original,"Open original".into()),
                    (Hit::Finished,if current.as_ref().is_some_and(|e|e.finished){"Mark unfinished"}else{"Mark finished"}.into()),
                    (Hit::Archive,if current.as_ref().is_some_and(|e|e.archived){"Unarchive"}else{"Archive"}.into()),
                    (Hit::Refresh,"Refresh saved copy".into()),(Hit::Remove,"Remove…".into())]
            };
            if !h.library_ui.confirm && h.reading.as_ref().is_some_and(|r|r.available){controls.extend([(Hit::Find,"Find".into()),(Hit::Copy,"Copy selection".into()),(Hit::Smaller,"A−".into()),(Hit::Larger,"A+".into())]);}
            y=self.library_controls(scene,h,&controls,y);
            let reading=h.reading.as_mut().unwrap();
            let newer=reading.available && current.as_ref().is_some_and(|e|e.snapshot.as_ref().is_some_and(|hash|hash!=&reading.version));
            let note=if h.library_ui.confirm{"Remove this item from the list? Its stored copies are retained for Undo; this is not secure deletion."}else if newer{"A newer saved copy is available. Return to Library and reopen this item to read it."}else{&reading.note};
            for line in crate::reader::wrap(&self.fonts,label,note,(r.w-px(40.0)).max(1.0)){self.fonts.draw(scene,Style{color:dim,..label},r.x+px(20.0),y+px(14.0),&line);y+=px(19.0);}
            if !self.library.status.is_empty(){let text=self.fit(label,&self.library.status,(r.w-px(40.0)).max(1.0));self.fonts.draw(scene,Style{color:dim,..label},r.x+px(20.0),y+px(14.0),&text);y+=px(23.0);}
            if let Some(q)=reading.reader.saved.find.as_ref(){
                let search=Rect::new(r.x+px(16.0),y,(r.w-px(32.0)).max(1.0),px(40.0));
                scene.outline(search,px(1.0),self.surface.signal);
                let text=self.fit(label,&format!("Find: {q}  · Enter next / Shift+Enter previous"),(search.w-px(16.0)).max(1.0));
                self.fonts.draw(scene,label,search.x+px(8.0),search.y+px(26.0),&text);
                h.library_ui.hits.push((search.intersect(&r),Hit::Find));y+=px(48.0);
            }
            let body_top=(y+px(6.0)).min(r.bottom());
            let body=Rect::new(r.x,body_top,r.w,(r.bottom()-body_top).max(0.0));
            if reading.available && body.h>0.0 {
                let f=self.reader_fonts();let rs=scale*h.library_ui.size;
                let width=crate::reader::column_width(body.w,rs);
                let font_key=[f.serif,f.serif_italic,f.mono,f.mono_strong];
                let reflow=reading.reader.laid_for!=(width,rs)||reading.reader.saved.font_key!=Some(font_key);
                let anchor=reading.resume.take().or_else(||if reflow{reading.reader.reading_position(&reading.version)}else{None});
                reading.reader.layout(&self.fonts,&f,width,rs,ink);
                reading.reader.saved.viewport=Some(body);reading.reader.saved.scale=rs;
                if let Some(p)=anchor{if !reading.reader.restore_reading_position(&p,&reading.version){reading.note="Saved copy · reading position restored approximately".into();}}
                reading.reader.draw(scene,&mut self.fonts,&f,body,rs,ink,dim,paper,self.surface.signal);
                scene.layer(Some(r));
                if !h.library_ui.confirm{for (rect,hit) in &reading.reader.saved.hits{h.library_ui.hits.push((*rect,match hit{TextHit::Link(u)=>Hit::Link(u.clone()),TextHit::Code(i)=>Hit::Code(*i)}));}}
            }else{
                let title=self.fit(self.ui_strong(),&reading.reader.article.title,(body.w-px(40.0)).max(1.0));
                self.fonts.draw(scene,self.ui_strong(),body.x+px(20.0),body.y+px(34.0),&title);
            }
            h.library_ui.hits.retain(|(r,_)|r.w>0.0&&r.h>0.0);
            scene.layer(outer);return;
        }
        let width=px(760.0).min((r.w-px(40.0)).max(1.0));let x=r.x+(r.w-width)/2.0;
        let title=self.fit(Style{font:self.f.wordmark,px:px(36.0),color:ink,tracking:0.0},"Your reading library",width);
        self.fonts.draw(scene,Style{font:self.f.wordmark,px:px(36.0),color:ink,tracking:0.0},x,y+px(40.0),&title);y+=px(66.0);
        for line in crate::reader::wrap(&self.fonts,label,"Save something worth returning to. Saved copies open here without visiting the original.",width){self.fonts.draw(scene,Style{color:dim,..label},x,y,&line);y+=px(20.0);}
        let controls=[(Hit::Filter(0),"Unfinished".into()),(Hit::Filter(1),"All saved".into()),(Hit::Filter(2),"Finished".into()),(Hit::Filter(3),"Archived".into())];
        y=self.library_controls(scene,h,&controls,y+px(8.0));
        let search=Rect::new(x,y,width,px(44.0));
        scene.outline(search,px(1.0),if h.library_ui.focus==Some(Hit::Search){self.surface.signal}else{dim});
        let text=self.fit(label,if h.input.is_empty(){"Search title or source"}else{&h.input},(width-px(20.0)).max(1.0));
        self.fonts.draw(scene,Style{color:if h.input.is_empty(){dim}else{ink},..label},x+px(10.0),y+px(28.0),&text);
        h.library_ui.hits.push((search.intersect(&r),Hit::Search));y+=px(56.0);
        if self.library.undo.is_some(){y=self.library_controls(scene,h,&[(Hit::Undo,"Undo removal".into())],y);}
        if !self.library.status.is_empty(){let message=self.fit(label,&self.library.status,width);self.fonts.draw(scene,Style{color:dim,..label},x,y+px(14.0),&message);y+=px(28.0);}
        let rows=self.library_rows_for(&h.input,h.library_ui.filter);
        let area=Rect::new(x,y,width,(r.bottom()-y-px(16.0)).max(1.0));let row_h=px(82.0);
        h.library_reach=(rows.len() as f32*row_h-area.h).max(0.0);
        if h.library_ui.reveal{if let Some(i)=h.library_ui.selected.as_ref().and_then(|key|rows.iter().position(|e|&e.id==key)){
            let top=i as f32*row_h;h.library_scroll=h.library_scroll.min(top).max((top+row_h-area.h).max(0.0));
        }h.library_ui.reveal=false;}
        h.library_scroll=h.library_scroll.clamp(0.0,h.library_reach);
        scene.layer(Some(area.intersect(&r)));
        if rows.is_empty(){let message=if !self.library.loaded{"Reading the saved library…"}else if self.library.entries.values().all(|e|e.deleted){"Save a page or text file using ‘Save to reading library’ in the command palette."}else{"No matching items in this view."};for (i,line) in crate::reader::wrap(&self.fonts,label,message,width).iter().enumerate(){self.fonts.draw(scene,label,x,y+px(28.0)+i as f32*px(22.0),line);}}
        for (i,e) in rows.iter().enumerate(){
            let row=Rect::new(x,y+i as f32*row_h-h.library_scroll,width,row_h);
            if row.bottom()<area.y||row.y>area.bottom(){continue;}
            let selected=h.library_ui.selected.as_ref()==Some(&e.id);
            if selected||row.contains(self.mouse.0,self.mouse.1){scene.rect(row,self.theme.tint);}
            let title=self.fit(self.ui_strong(),&e.title,(width-px(20.0)).max(1.0));
            self.fonts.draw(scene,self.ui_strong(),x+px(10.0),row.y+px(27.0),&title);
            let state=if e.words==0 && e.snapshot.is_none(){"Link only".into()}else{format!("~{} min · {} · saved text",e.words.div_ceil(220),if e.finished{"finished"}else if e.progress>0.0{"continue reading"}else{"unread"})};
            let text=self.fit(label,&format!("{state} · {}",e.source),(width-px(20.0)).max(1.0));
            self.fonts.draw(scene,Style{color:dim,..label},x+px(10.0),row.y+px(53.0),&text);
            scene.hline(x,row.bottom()-px(1.0),width,px(1.0),self.theme.tint);
            let clipped=row.intersect(&area).intersect(&r);
            if clipped.w>0.0&&clipped.h>0.0{h.hits.push((clipped,i));h.library_ui.hits.push((clipped,Hit::Row(e.id.clone())));}
        }
        scene.layer(outer);
    }
}

fn capture_article(v:&serde_json::Value)->Result<(Article,String),String> {
    let doc=serde_json::to_string(&v["document"]).map_err(|e|e.to_string())?;
    let mut article=Article::parse(&doc).filter(|a|a.words()>0).ok_or("No readable article")?;
    let media=v["media"].as_array().ok_or("Invalid image manifest")?;
    if media.len()>12{return Err("Too many saved images".into());}
    let mut images=BTreeMap::new();let mut bytes=0usize;let mut pixels=0u64;
    for m in media {
        let name=m["id"].as_str().filter(|s|s.len()<=32).ok_or("Invalid image identifier")?;
        let hex=m["png"].as_str().ok_or("Invalid image data")?;
        let(w,h,png)=hex_png(hex)?;bytes+=png.len();pixels+=u64::from(w)*u64::from(h);
        if bytes>196608||pixels>2_097_152||m["width"].as_u64()!=Some(w as u64)||m["height"].as_u64()!=Some(h as u64){return Err("Saved images exceed their budget".into());}
        if images.insert(name.to_string(),format!("nus-png:{hex}")).is_some(){return Err("Duplicate image identifier".into());}
    }
    let mut missing=0;
    for b in &mut article.blocks {
        match b {
            Block::Image(_,src)=>{*src=images.get(src).cloned().unwrap_or_else(||{missing+=1;String::new()});},
            Block::Link(_,u)=>{let parsed=url::Url::parse(u).map_err(|_|"Invalid link")?;if !matches!(parsed.scheme(),"http"|"https")||!parsed.username().is_empty()||parsed.password().is_some(){return Err("Unsafe saved link".into());}},
            _=>{},
        }
    }
    let note=if missing>0{format!("{missing} image(s) unavailable offline. No additional image requests were made.")}else{"Text and eligible loaded images saved without additional requests.".into()};
    // Serialization/size is checked again by Store immediately before commit.
    Ok((article,note))
}

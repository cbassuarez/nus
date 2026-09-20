//! Browser downloads: one process-wide queue, persistent history, and local UI.
use std::path::{Path,PathBuf};
use std::sync::atomic::{AtomicU8,AtomicU64,Ordering};
use serde::{Serialize,Deserialize};
use nus_render::{Rect,Scene,Style};
use crate::app::{App,Pane};
use cef::ImplDownloadItemCallback;

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,Serialize,Deserialize)]
pub enum Rename { #[default] Off, All, Selective }
static RENAME:AtomicU8=AtomicU8::new(0);
pub static REVISION:AtomicU64=AtomicU64::new(0);
pub fn set_rename(mode:Rename){RENAME.store(mode as u8,Ordering::Relaxed);}
pub fn rename_mode()->Rename{match RENAME.load(Ordering::Relaxed){1=>Rename::All,2=>Rename::Selective,_=>Rename::Off}}

#[derive(Clone,Debug,Default,Serialize,Deserialize)]
#[serde(default)]
pub struct Download {
    pub key:u64, pub id:u32, pub name:String, pub original:String, pub path:String,
    pub url:String, pub title:String, pub received:i64, pub total:i64, pub speed:i64,
    pub done:bool, pub cancelled:bool, pub interrupted:bool, pub paused:bool,
    pub started:u64,
    #[serde(skip)] pub live:bool,
}
impl Download {
    pub fn active(&self)->bool {self.live&&!self.done&&!self.cancelled&&!self.interrupted}
    pub fn status(&self)->String {
        if self.done {return format!("Complete · {}",bytes(self.received.max(self.total)));}
        if self.cancelled {return "Cancelled".into();}
        if self.interrupted {return "Interrupted · download again from the source".into();}
        if self.paused {return format!("Paused · {} received",bytes(self.received));}
        let amount=if self.total>0 {format!("{} of {} · {}%",bytes(self.received),bytes(self.total),(100*self.received/self.total).clamp(0,100))} else {format!("{} received",bytes(self.received))};
        if self.speed>0 {format!("{amount} · {}/s",bytes(self.speed))} else {format!("{amount} · waiting for data")}
    }
}
pub fn bytes(n:i64)->String {
    let n=n.max(0) as f64;
    if n>=1_073_741_824.0 {format!("{:.1} GB",n/1_073_741_824.0)} else if n>=1_048_576.0 {format!("{:.1} MB",n/1_048_576.0)} else if n>=1024.0 {format!("{:.0} KB",n/1024.0)} else {format!("{n:.0} B")}
}
fn history_path()->PathBuf{PathBuf::from("profile/downloads.json")}
pub fn init(){
    static ONCE:std::sync::Once=std::sync::Once::new();
    ONCE.call_once(||{
        let mut rows:Vec<Download>=std::fs::read(history_path()).ok().and_then(|v|serde_json::from_slice(&v).ok()).unwrap_or_default();
        for d in &mut rows {if !d.done&&!d.cancelled {d.interrupted=true;d.paused=false;} d.live=false;}
        *crate::browser::DOWNLOADS.lock().unwrap()=rows;
    });
}
pub fn save(rows:&[Download]){
    if let Ok(data)=serde_json::to_vec_pretty(rows){
        let path=history_path();let tmp=path.with_extension("json.tmp");
        if std::fs::write(&tmp,data).is_ok(){let _=std::fs::rename(tmp,path);}
    }
}
pub fn list()->Vec<Download>{crate::browser::DOWNLOADS.lock().unwrap().iter().rev().cloned().collect()}
pub fn changed(){REVISION.fetch_add(1,Ordering::Relaxed);}
thread_local!{static CALLBACKS:std::cell::RefCell<std::collections::HashMap<u64,cef::DownloadItemCallback>>=std::cell::RefCell::new(Default::default());}
pub fn track(key:u64,cb:Option<&mut cef::DownloadItemCallback>,active:bool){CALLBACKS.with(|c|{let mut c=c.borrow_mut();if active {if let Some(cb)=cb{c.insert(key,cb.clone());}}else{c.remove(&key);}});}

/// Keep filenames portable and confined to Downloads, even with hostile headers.
pub fn safe_name(raw:&str)->String{
    let raw=raw.rsplit(['/', '\\']).next().unwrap_or("");
    let mut name=String::new();
    for c in raw.chars(){if c.is_control()||matches!(c,':'|'*'|'?'|'"'|'<'|'>'|'|') {name.push(' ');}else{name.push(c);}}
    let name=name.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut name=name.trim_matches([' ','.']).to_string();
    while name.len()>220 {name.pop();}
    if name.is_empty(){name="download".into();}
    let stem=name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(stem.as_str(),"CON"|"PRN"|"AUX"|"NUL"|"COM1"|"COM2"|"COM3"|"COM4"|"COM5"|"COM6"|"COM7"|"COM8"|"COM9"|"LPT1"|"LPT2"|"LPT3"|"LPT4"|"LPT5"|"LPT6"|"LPT7"|"LPT8"|"LPT9"){name.insert(0,'_');}
    name
}
fn extension(name:&str)->String{
    let lower=name.to_ascii_lowercase();
    for ext in [".tar.gz",".tar.bz2",".tar.xz",".tar.zst"] {if lower.ends_with(ext){return name[name.len()-ext.len()..].into();}}
    Path::new(name).extension().map(|s|format!(".{}",s.to_string_lossy())).unwrap_or_default()
}
pub fn filename(original:&str,title:&str,mode:Rename)->String{
    let original=safe_name(original);
    if mode==Rename::Off{return original;}
    let ext=extension(&original);
    if mode==Rename::Selective {
        let readable=matches!(ext.to_ascii_lowercase().as_str(),".pdf"|".epub"|".doc"|".docx"|".odt"|".ppt"|".pptx"|".png"|".jpg"|".jpeg"|".webp"|".gif"|".avif"|".mp3"|".m4a"|".mp4"|".mov"|".webm");
        // Versioned names, checksums and signed companion names retain their identity.
        let technical=original.split(|c:char| !c.is_ascii_alphanumeric()).any(|s|s.len()>=12&&s.chars().all(|c|c.is_ascii_hexdigit())) || original.to_lowercase().contains("signed") || original.split('.').filter(|s| !s.is_empty()&&s.chars().all(|c|c.is_ascii_digit())).count()>=2;
        if !readable||technical{return original;}
    }
    let title=title.trim();
    if title.is_empty()||title.contains("://")||matches!(title.to_lowercase().as_str(),"download"|"downloads"|"untitled"|"home"|"new tab"){return original;}
    // A page title is text, not a path. Slashes become separators, not directories.
    let title=safe_name(&title.replace(['/','\\']," - "));
    let stem=if !ext.is_empty()&&title.to_lowercase().ends_with(&ext.to_lowercase()){title[..title.len()-ext.len()].to_string()}else{title};
    let mut stem=stem;
    while stem.len()+ext.len()>220 {stem.pop();}
    format!("{}{ext}",stem.trim_end())
}
pub fn available_path(dir:&Path,name:&str,rows:&[Download])->PathBuf{
    let ext=extension(name);let stem=&name[..name.len()-ext.len()];
    let mut path=dir.join(name);let mut n=1;
    while path.exists()||rows.iter().any(|d|d.active()&&Path::new(&d.path)==path){n+=1;path=dir.join(format!("{stem} ({n}){ext}"));}
    path
}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Hit{Close,Page,Folder,Clear,Pause(u64),Resume(u64),Cancel(u64),Reveal(u64),Source(u64)}
impl Hit{pub fn label(self)->&'static str{match self{Self::Close=>"Close downloads",Self::Page=>"Open Downloads page",Self::Folder=>"Open Downloads folder",Self::Clear=>"Clear finished download history; keep files",Self::Pause(_)=>"Pause download",Self::Resume(_)=>"Resume download",Self::Cancel(_)=>"Cancel download",Self::Reveal(_)=>"Show downloaded file in folder",Self::Source(_)=>"Open download source"}}}
pub struct DownloadsPane{pub rect:Rect,pub scroll:f32}
impl Default for DownloadsPane{fn default()->Self{Self{rect:Rect::new(0.0,0.0,1.0,1.0),scroll:0.0}}}
#[derive(Default)]
pub struct Ui{pub hits:Vec<(Rect,Hit)>,pub anchor:Option<Rect>,pub rect:Option<Rect>,pub scroll:f32,pub reach:f32,pub revision:u64,pub hover_since:Option<std::time::Instant>,pub focus:Option<usize>}
impl App {
    pub(crate) fn open_downloads(&mut self){
        self.close_menus();
        if let Some(i)=self.tabs.iter().position(|t|matches!(t.left,Pane::Downloads(_))){self.activate(i);return;}
        let tab=self.make_tab(Pane::Downloads(DownloadsPane::default()),None);self.tabs.push(tab);self.activate(self.tabs.len()-1);self.layout();self.save_session();
    }
    pub(crate) fn download_action(&mut self,hit:Hit){
        match hit {
            Hit::Close=>self.close_menus(),Hit::Page=>self.open_downloads(),
            Hit::Folder=>{reveal(&crate::browser::downloads_dir(),false);},
            Hit::Clear=>{let mut rows=crate::browser::DOWNLOADS.lock().unwrap();rows.retain(|d|d.active());save(&rows);changed();},
            Hit::Pause(key)|Hit::Resume(key)|Hit::Cancel(key)=>{
                // Clone before calling CEF: it may synchronously update the list.
                let cb=CALLBACKS.with(|c|c.borrow().get(&key).cloned());
                if let Some(cb)=cb {match hit{Hit::Pause(_)=>cb.pause(),Hit::Resume(_)=>cb.resume(),_=>cb.cancel()}}
            },
            Hit::Reveal(key)|Hit::Source(key)=>{
                let row=list().into_iter().find(|d|d.key==key);
                if let Some(d)=row{if matches!(hit,Hit::Reveal(_))&&d.done {reveal(Path::new(&d.path),true);}else if matches!(hit,Hit::Source(_)){self.open_url(&d.url,true);}}
            }
        }
        self.dirty=true;
    }
    pub(crate) fn download_click(&mut self,x:f32,y:f32)->bool{
        if let Some((_,hit))=self.download_ui.hits.iter().rev().find(|(r,_)|r.contains(x,y)).copied(){self.download_action(hit);return true;}
        if self.dl_menu {if !self.download_ui.rect.is_some_and(|r|r.contains(x,y)){self.close_menus();}return true;}false
    }
    pub(crate) fn download_button(&mut self,scene:&mut Scene,r:Rect,label:&str,hit:Hit){
        let hot=r.contains(self.mouse.0,self.mouse.1)||self.download_ui.focus==Some(self.download_ui.hits.len());let ink=self.theme.ink;
        scene.rect(r,if hot{self.theme.tint}else{self.paper()});scene.outline(r,self.px(1.0),ink);
        let st=self.label();let label=self.fit(st,label,r.w-self.px(12.0));let w=self.fonts.measure(st,&label);
        self.fonts.draw(scene,st,r.x+(r.w-w)*0.5,r.y+r.h*0.5+self.px(4.0),&label);
        self.download_ui.hits.push((r,hit));
    }
    pub(crate) fn draw_downloads(&mut self,scene:&mut Scene,r:Rect,scroll:f32,modal:bool)->f32{
        let rows=list();let pad=self.px(if r.w<self.px(500.0){16.0}else{28.0});let ink=self.theme.ink;
        scene.layer(Some(r));scene.rect(r,self.paper());
        let title=Style{px:self.px(22.0),..self.ui_strong()};self.fonts.draw(scene,title,r.x+pad,r.y+self.px(38.0),"Downloads");
        let active=rows.iter().filter(|d|d.active()).count();
        let desc=if r.w<self.px(280.0){if active>0{format!("{active} in progress")}else{"Saved on this device.".into()}}else if active>0{format!("{active} in progress · {} {} in history",rows.len(),if rows.len()==1{"file"}else{"files"})}else{"Saved to your Downloads folder. Files keep their original names by default.".into()};
        let st=Style{color:self.theme.dim,..self.label()};let text=self.fit(st,&desc,r.w-pad*2.0);self.fonts.draw(scene,st,r.x+pad,r.y+self.px(62.0),&text);
        let bh=self.px(28.0);let gap=self.px(8.0);let stacked=r.w<self.px(280.0);let bw=if stacked{r.w-pad*2.0}else{((r.w-pad*2.0-gap)/2.0).min(self.px(170.0))};
        self.download_button(scene,Rect::new(r.x+pad,r.y+self.px(78.0),bw,bh),"Open folder",Hit::Folder);
        let second=if stacked{Rect::new(r.x+pad,r.y+self.px(114.0),bw,bh)}else{Rect::new(r.x+pad+bw+gap,r.y+self.px(78.0),bw,bh)};
        self.download_button(scene,second,if modal{"All downloads"}else{"Clear finished"},if modal{Hit::Page}else{Hit::Clear});
        if modal{self.download_button(scene,Rect::new(r.right()-pad-self.px(24.0),r.y+self.px(16.0),self.px(24.0),self.px(24.0)),"×",Hit::Close);}
        let extra=if stacked{self.px(36.0)}else{0.0};
        let body=Rect::new(r.x+pad,r.y+self.px(122.0)+extra,r.w-pad*2.0,(r.h-self.px(138.0)-extra).max(1.0));
        let row_h=self.px(118.0);let reach=(rows.len() as f32*row_h-body.h).max(0.0);let scroll=scroll.clamp(0.0,reach);
        scene.layer(Some(body));
        if rows.is_empty(){self.fonts.draw(scene,self.ui_strong(),body.x,body.y+self.px(30.0),"No downloads yet");let s=self.fit(st,if r.w<self.px(280.0){"Ready for your files."}else{"Files you download will appear here with their progress and source."},body.w);self.fonts.draw(scene,st,body.x,body.y+self.px(54.0),&s);}
        for (i,d) in rows.iter().enumerate(){
            let y=(body.y+i as f32*row_h-scroll).round();if y+row_h<body.y||y>body.bottom(){continue;}
            let icon=if d.done{nus_render::text::icons::CHECK}else if d.cancelled||d.interrupted{nus_render::text::icons::CLOSE}else{nus_render::text::icons::DOWNLOAD};
            self.fonts.draw_icon(scene,icon,self.px(16.0),body.x,y+self.px(9.0),if d.active(){self.surface.signal}else{ink});
            let text=self.fit(self.ui_strong(),&d.name,body.w-self.px(28.0));self.fonts.draw(scene,self.ui_strong(),body.x+self.px(26.0),y+self.px(22.0),&text);
            let status=self.fit(st,&d.status(),body.w);self.fonts.draw(scene,st,body.x,y+self.px(43.0),&status);
            let source=crate::sites::host_of(&d.url);let source=if d.original!=d.name{format!("{source} · originally {}",d.original)}else{source};let source=self.fit(st,&source,body.w);self.fonts.draw(scene,st,body.x,y+self.px(60.0),&source);
            let bar=Rect::new(body.x,y+self.px(68.0),body.w,self.px(2.0));scene.rect(bar,self.theme.tint);
            let progress=if d.done{1.0}else if d.total>0{(d.received as f32/d.total as f32).clamp(0.0,1.0)}else{0.0};scene.rect(Rect::new(bar.x,bar.y,bar.w*progress,bar.h),self.surface.signal);
            let mut actions=vec![];
            if d.active(){actions.push((if d.paused{"Resume"}else{"Pause"},if d.paused{Hit::Resume(d.key)}else{Hit::Pause(d.key)}));actions.push(("Cancel",Hit::Cancel(d.key)));}else if d.done{actions.push(("Show in folder",Hit::Reveal(d.key)));}
            actions.push(("Source",Hit::Source(d.key)));
            let bw=((body.w-gap*(actions.len()-1) as f32)/actions.len() as f32).min(self.px(136.0));
            for (j,(label,hit)) in actions.into_iter().enumerate(){let button=Rect::new(body.x+j as f32*(bw+gap),y+self.px(80.0),bw,self.px(25.0));if button.y>=body.y&&button.bottom()<=body.bottom(){self.download_button(scene,button,if label=="Show in folder"&&bw<self.px(100.0){"Show file"}else{label},hit);}}
            scene.hline(body.x,y+row_h-self.px(1.0),body.w,self.px(1.0),self.theme.tint);
        }
        if reach>0.0 {let h=(body.h*body.h/(body.h+reach)).max(self.px(24.0));let y=body.y+(body.h-h)*scroll/reach;scene.rect(Rect::new(r.right()-self.px(6.0),y,self.px(3.0),h),self.theme.dim);}
        scene.layer(None);reach
    }
    pub(crate) fn draw_download_overlay(&mut self,scene:&mut Scene){
        if self.dl_menu {
            self.tip=None;
            let full=Rect::new(0.0,0.0,self.target.size.0 as f32,self.target.size.1 as f32);let w=self.px(640.0).min(full.w-self.px(24.0));let h=self.px(142.0+if w<self.px(280.0){36.0}else{0.0}+list().len().clamp(1,3) as f32*118.0).min(full.h-self.px(48.0));let r=Rect::new(((full.w-w)*0.5).round(),((full.h-h)*0.5).round(),w,h);
            scene.layer(None);scene.rect(full,crate::app::fade(self.theme.ink,0.22));scene.rect(Rect::new(r.x+self.px(4.0),r.y+self.px(4.0),r.w,r.h),self.theme.ink);
            self.download_ui.hits.clear();self.download_ui.rect=Some(r);self.download_ui.reach=self.draw_downloads(scene,r,self.download_ui.scroll,true);scene.outline(r,self.px(1.0),self.theme.ink);return;
        }
        self.download_ui.rect=None;
        let hot=self.download_ui.anchor.filter(|r|r.contains(self.mouse.0,self.mouse.1));
        if let Some(anchor)=hot {
            let since=*self.download_ui.hover_since.get_or_insert_with(std::time::Instant::now);
            if since.elapsed().as_millis()<250{self.dirty=true;return;}
            let rows=list();let active:Vec<_>=rows.iter().filter(|d|d.active()).collect();let shown:Vec<_>=if active.is_empty(){rows.iter().take(2).collect()}else{active.into_iter().take(3).collect()};
            let w=self.px(340.0).min(self.target.size.0 as f32-self.px(16.0));let h=self.px(44.0+shown.len() as f32*44.0);let x=(anchor.x-w*0.5).clamp(self.px(8.0),(self.target.size.0 as f32-w-self.px(8.0)).max(self.px(8.0)));let y=(anchor.y-h-self.px(8.0)).max(self.px(8.0));let r=Rect::new(x,y,w,h);
            scene.layer(None);scene.rect(r,self.paper());scene.outline(r,self.px(1.0),self.theme.ink);let st=self.label();let title=if rows.is_empty(){"Downloads · no files yet".into()}else{format!("Downloads · {} in progress",rows.iter().filter(|d|d.active()).count())};self.fonts.draw(scene,self.label_strong(),x+self.px(12.0),y+self.px(23.0),&title);
            for(i,d)in shown.iter().enumerate(){let y=y+self.px(46.0+i as f32*44.0);let name=self.fit(st,&d.name,w-self.px(24.0));self.fonts.draw(scene,st,x+self.px(12.0),y,&name);let note=self.fit(st,&d.status(),w-self.px(24.0));self.fonts.draw(scene,Style{color:self.theme.dim,..st},x+self.px(12.0),y+self.px(17.0),&note);}
        }else{self.download_ui.hover_since=None;}
    }
}
fn reveal(path:&Path,file:bool){
    #[cfg(target_os="macos")] {let mut c=std::process::Command::new("open");if file{c.arg("-R");}let _=c.arg(path).spawn();}
    #[cfg(windows)] {let mut c=std::process::Command::new("explorer");if file{c.arg(format!("/select,{}",path.display()));}else{c.arg(path);}let _=c.spawn();}
    #[cfg(all(unix,not(target_os="macos")))] {let p=if file{path.parent().unwrap_or(path)}else{path};let _=std::process::Command::new("xdg-open").arg(p).spawn();}
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn renaming_is_opt_in_and_preserves_extensions(){assert_eq!(filename("Report Final.pdf","New title",Rename::Off),"Report Final.pdf");assert_eq!(filename("archive.tar.gz","Project / Release",Rename::All),"Project - Release.tar.gz");assert_eq!(filename("d.pdf","Readable report.pdf",Rename::All),"Readable report.pdf");assert_eq!(filename("d.pdf","Downloads",Rename::All),"d.pdf");}
    #[test]fn selective_preserves_technical_identity(){for n in ["app.dmg","source.tar.gz","package.json","app-1.2.3.pdf","0123456789abcdef.pdf","signed-copy.pdf","id.csv"]{assert_eq!(filename(n,"Readable title",Rename::Selective),n);}assert_eq!(filename("document.pdf","Annual report",Rename::Selective),"Annual report.pdf");}
    #[test]fn names_cannot_escape_the_download_folder(){for n in ["../../bad.exe","C:\\folder\\file.pdf","...","foo:bar?.txt","CON.txt"]{let v=filename(n,"title",Rename::Off);assert!(!v.contains('/')&&!v.contains('\\')&&!v.contains(':'));assert!(!v.starts_with('.'));}assert_eq!(safe_name("CON.txt"),"_CON.txt");}
    #[test]fn pending_downloads_reserve_their_filename(){let dir=Path::new("/tmp/nus-collision-test");let d=Download{path:dir.join("report.pdf").to_string_lossy().into(),live:true,..Default::default()};assert_eq!(available_path(dir,"report.pdf",&[d]),dir.join("report (2).pdf"));}
}

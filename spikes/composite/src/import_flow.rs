//! An optional local import page. Reading a chosen export only prepares a
//! preview; applying it never launches commands, opens URLs or copies secrets.
use crate::{app::App, me::{CardHit, Step}};
use nus_render::{Scene, Rect, Style, text::icons};
use std::{path::Path, time::Instant};

pub const APPS: [(&str, u8); 12] = [("Arc",0),("Ghostty",1),("VS Code",2),("Dia",0),("Terminal",1),("Safari",0),("kitty",1),("Chrome",0),("WezTerm",1),("Alacritty",1),("Cursor",2),("Zed",2)];
pub struct Flow {
    pub at: Instant,
    pub phase: f32,
    pub focus: usize,
    pub source: usize,
    pub picker: Option<crate::pick::Picker>,
    pub pending: Option<std::sync::mpsc::Receiver<Result<Plan,String>>>,
    pub plan: Option<Plan>,
    pub message: String,
}
impl Default for Flow {fn default()->Self{Self{at:crate::clock::now(),phase:0.0,focus:0,source:0,picker:None,pending:None,plan:None,message:String::new()}}}
#[derive(Clone,Debug)]
pub enum Plan { Bookmarks(Vec<crate::folders::Item>), Theme(String) }
impl Plan {pub fn summary(&self)->String{match self{Self::Bookmarks(v)=>format!("{} saved links → sidebar folder",v.len()),Self::Theme(_)=>"One color theme → Appearance imports".into()}}}

fn entity(s:&str)->String {
    let re=regex::Regex::new(r"&(#x[0-9a-fA-F]+|#[0-9]+|amp|lt|gt|quot|apos);").unwrap();
    re.replace_all(s,|c:&regex::Captures|match &c[1]{"amp"=>"&".into(),"lt"=>"<".into(),"gt"=>">".into(),"quot"=>"\"".into(),"apos"=>"'".into(),v=>{let n=if let Some(h)=v.strip_prefix("#x"){u32::from_str_radix(h,16).ok()}else{v.strip_prefix('#').and_then(|s|s.parse().ok())};n.and_then(char::from_u32).map(|c|c.to_string()).unwrap_or_else(||c[0].into())}}).into_owned()
}
fn add(items:&mut Vec<crate::folders::Item>,title:&str,raw:&str){
    if items.len()>=5000{return;}
    let Ok(url)=url::Url::parse(raw) else{return};
    if !matches!(url.scheme(),"http"|"https") || !url.username().is_empty() || url.password().is_some(){return;}
    let url=url.to_string();if items.iter().any(|i|i.url==url){return;}
    items.push(crate::folders::Item{title:if title.trim().is_empty(){url.clone()}else{title.trim().chars().filter(|c|!c.is_control()).take(240).collect()},url,detail:String::new()});
}
fn json_links(value:&serde_json::Value,items:&mut Vec<crate::folders::Item>,depth:u8){
    if depth>32 || items.len()>=5000{return;}
    match value {
        serde_json::Value::Object(m)=>{if let Some(url)=m.get("url").and_then(|v|v.as_str()){add(items,m.get("name").or_else(||m.get("title")).and_then(|v|v.as_str()).unwrap_or(""),url);}
            for v in m.values(){if v.is_object()||v.is_array(){json_links(v,items,depth+1);}}},
        serde_json::Value::Array(a)=>for v in a{json_links(v,items,depth+1)}, _=>{}
    }
}
pub fn parse(source:usize,text:&str)->Result<Plan,String>{
    if text.len()>8*1024*1024{return Err("Choose an export smaller than 8 MB.".into());}
    let (name,kind)=APPS.get(source).copied().ok_or("Choose an application.")?;
    if kind==0 {
        let mut items=Vec::new();
        if let Ok(value)=serde_json::from_str::<serde_json::Value>(text){json_links(&value,&mut items,0);}
        else {
            let links=regex::Regex::new(r#"(?is)<a\s+[^>]*href\s*=\s*["']([^"']+)["'][^>]*>(.*?)</a\s*>"#).unwrap();
            let tags=regex::Regex::new(r"<[^>]*>").unwrap();
            for c in links.captures_iter(text){add(&mut items,&entity(&tags.replace_all(&c[2],"")),&entity(&c[1]));}
        }
        if items.len()>=5000{return Err("This export reaches the 5,000-link limit. Split it into smaller exports before importing.".into());}
        if items.is_empty(){return Err("No web bookmarks found. Choose a bookmarks HTML export or a Chromium Bookmarks JSON file.".into());}
        Ok(Plan::Bookmarks(items))
    }else{
        // Copy only recognized colors, never shell hooks, credentials or executable config.
        let theme=crate::theme_edit::parse_theme(name,text).ok_or("No supported color theme found. Choose a Ghostty, VS Code, Windows Terminal or base16 theme export. Other configuration is not imported.")?;
        let hex=|c:nus_render::Color|format!("#{:02x}{:02x}{:02x}",(c[0]*255.0).round() as u8,(c[1]*255.0).round() as u8,(c[2]*255.0).round() as u8);
        let mut out=String::new();
        if let Some(c)=theme.paper{out.push_str(&format!("background = {}\n",hex(c)));}
        if let Some(c)=theme.ink{out.push_str(&format!("foreground = {}\n",hex(c)));}
        if let Some(colors)=theme.ansi{for (i,c) in colors.into_iter().enumerate(){out.push_str(&format!("palette = {i}={}\n",hex(c)));}}
        Ok(Plan::Theme(out))
    }
}
fn read(source:usize,path:&Path)->Result<Plan,String>{
    use std::io::Read;
    let file=std::fs::File::open(path).map_err(|e|format!("Could not open export: {e}"))?;
    if !file.metadata().map_err(|e|e.to_string())?.is_file(){return Err("Choose a regular export file.".into());}
    let mut text=String::new();file.take(8*1024*1024+1).read_to_string(&mut text).map_err(|_|"Choose a UTF-8 text export, not a database or binary settings file.")?;
    parse(source,&text)
}

impl App {
    pub(crate) fn import_access_tree(&mut self)->Option<accesskit::TreeUpdate>{
        use accesskit::{Action,Node,NodeId,Role,TreeId,TreeInfo,TreeUpdate};
        if !self.me_card.open||!matches!(self.me_card.step,Some(Step::Import|Step::ImportSources|Step::ImportReview)){return None;}
        self.access_map.clear();let mut children=Vec::new();let mut nodes=Vec::new();let mut focus=NodeId(1);
        for(i,(rect,hit))in self.me_card.hits.iter().enumerate(){let id=NodeId(100+i as u64);let mut n=Node::new(Role::Button);
            let name=match hit{CardHit::ImportOpen=>"Choose applications to import from".into(),CardHit::ImportSource(i)=>format!("{}{}",APPS[*i].0,if *i==self.me_card.import.source{" selected"}else{""}),CardHit::ImportPick=>"Choose export file".into(),CardHit::ImportApply=>"Import reviewed items".into(),CardHit::Next=>"Continue".into(),CardHit::Back=>"Back".into(),_=>"Close".into()};
            n.set_label(name);n.set_bounds(accesskit::Rect{x0:rect.x as f64,y0:rect.y as f64,x1:rect.right() as f64,y1:rect.bottom() as f64});n.add_action(Action::Click);n.add_action(Action::Focus);
            if (*hit==CardHit::ImportOpen&&self.me_card.import.focus==1)||matches!(hit,CardHit::ImportSource(i) if *i==self.me_card.import.source){focus=id;}
            self.access_map.insert(id.0,crate::access::Target::Import(*hit));children.push(id);nodes.push((id,n));
        }
        let mut root=Node::new(Role::Dialog);root.set_modal();root.set_label(format!("Import from another application. {} {}",self.me_card.import.plan.as_ref().map(Plan::summary).unwrap_or_default(),self.me_card.import.message));root.set_children(children);nodes.push((NodeId(1),root));
        Some(TreeUpdate{nodes,tree:Some(TreeInfo::new(NodeId(1))),tree_id:TreeId::ROOT,focus})
    }
    pub(crate) fn import_hit(&mut self,hit:CardHit){
        match hit {
            CardHit::ImportOpen=>{self.me_card.step=Some(Step::ImportSources);self.me_card.import.focus=0;},
            CardHit::ImportSource(i)=>{if i<APPS.len()&&self.me_card.import.picker.is_none()&&self.me_card.import.pending.is_none(){self.me_card.import.source=i;self.me_card.import.focus=i;}},
            CardHit::ImportPick=>{
                if self.me_card.import.picker.is_some()||self.me_card.import.pending.is_some(){return;}
                let name=APPS[self.me_card.import.source].0;
                match crate::pick::file(&self.window,&format!("Import from {name} · choose an export")) {Ok(p)=>{self.me_card.import.picker=Some(p);self.me_card.import.message="Choose an export file. Nothing is saved until you review it.".into();},Err(e)=>self.me_card.import.message=e}
            }
            CardHit::ImportApply=>self.apply_import(),
            _=>{}
        }
        self.dirty=true;
    }
    pub(crate) fn tend_import(&mut self){
        if let Some(result)=self.me_card.import.picker.as_mut().and_then(|p|p.poll()){
            self.me_card.import.picker=None;
            match result {Ok(Some(path))=>{let source=self.me_card.import.source;let(tx,rx)=std::sync::mpsc::channel();self.me_card.import.pending=Some(rx);std::thread::spawn(move||{let _=tx.send(read(source,&path));});},Ok(None)=>self.me_card.import.message.clear(),Err(e)=>self.me_card.import.message=e}
            self.dirty=true;
        }
        let result=self.me_card.import.pending.as_ref().and_then(|r|match r.try_recv(){Ok(v)=>Some(v),Err(std::sync::mpsc::TryRecvError::Empty)=>None,Err(_)=>Some(Err("Import preview stopped. Choose the export again.".into()))});
        if let Some(result)=result{self.me_card.import.pending=None;match result{Ok(plan)=>{self.me_card.import.plan=Some(plan);self.me_card.import.message.clear();self.me_card.step=Some(Step::ImportReview);},Err(e)=>self.me_card.import.message=e}self.dirty=true;}
        if self.me_card.open&&self.me_card.step==Some(Step::Import)&&!self.motion.reduced(){
            let now=crate::clock::now();let delta=now.duration_since(self.me_card.import.at).as_secs_f32();self.me_card.import.at=now;
            let hover=self.me_card.hits.iter().any(|(r,h)|*h==CardHit::ImportOpen&&r.contains(self.mouse.0,self.mouse.1));
            if !hover&&self.me_card.import.focus==0{let previous=self.me_card.import.phase;self.me_card.import.phase+=delta;let phase=self.me_card.import.phase;if phase%2.5>1.75||previous%2.5>1.75||(phase/2.5)as usize!=(previous/2.5)as usize{self.dirty=true;}}
        }else{self.me_card.import.at=crate::clock::now();}
    }
    fn apply_import(&mut self){
        let Some(plan)=self.me_card.import.plan.as_ref() else{return};
        let name=format!("Imported from {}",APPS[self.me_card.import.source].0);
        let result=match plan {
            Plan::Theme(text)=>{
                let path=crate::theme_edit::themes_dir().join(format!("{}-{}.theme",APPS[self.me_card.import.source].0.replace(' ',"-"),crate::journal::now()));
                std::fs::create_dir_all(crate::theme_edit::themes_dir()).and_then(|_|crate::store::write_atomic(&path,text.as_bytes()))
            }
            Plan::Bookmarks(items)=>{
                let mut folders=self.folders.clone();
                let i=folders.iter().position(|f|f.kind==crate::folders::Kind::Plain&&f.name==name);
                let at=i.unwrap_or_else(||{folders.push(crate::folders::Folder{id:self.next_folder_id,name:name.clone(),kind:crate::folders::Kind::Plain,items:vec![],open:false,note:String::new()});folders.len()-1});
                for item in items{if !folders[at].items.iter().any(|v|v.url==item.url){folders[at].items.push(item.clone());}}
                let disk:Vec<_>=folders.iter().filter(|f|f.kind==crate::folders::Kind::Plain).map(|f|serde_json::json!({"name":f.name,"items":f.items,"open":f.open})).collect();
                let result=crate::store::write_json(Path::new("profile/folders.json"),&disk);
                if result.is_ok(){self.folders=folders;if i.is_none(){self.next_folder_id+=1;}}
                result
            }
        };
        match result{Ok(())=>{self.me_card.import.message=format!("Imported. {}",plan.summary());self.me_card.import.plan=None;self.me_card.step=Some(Step::Import);},Err(e)=>self.me_card.import.message=format!("Could not save import: {e}")}
        self.dirty=true;
    }
    pub(crate) fn draw_import_page(&mut self,scene:&mut Scene,bx:f32,mut y:f32,bw:f32,foot:f32){
        let step=self.me_card.step;let ui=self.ui();let dim=Style{color:self.theme.dim,..ui};let title=Style{font:self.f.serif,px:self.px(23.0),color:self.theme.ink,tracking:0.0};
        if step==Some(Step::Import){
            self.fonts.draw(scene,title,bx,y+self.px(22.0),"Make yourself at home.");y+=self.px(46.0);
            for line in crate::reader::wrap(&self.fonts,dim,"Bring your saved links and color themes.",bw){self.fonts.draw(scene,dim,bx,y,&line);y+=self.px(18.0);}
            y+=self.px(21.0);let st=Style{color:self.surface.signal,px:self.px(17.0),..ui};
            let lead=self.fonts.draw(scene,st,bx,y,"Import from");let rx=bx+lead+self.px(9.0);let width=(bw-lead-self.px(33.0)).min(self.px(144.0));let clip=Rect::new(rx,y-self.px(23.0),width,self.px(36.0));
            let time=self.me_card.import.phase;let period=2.5;let index=(time/period) as usize%APPS.len();
            let elapsed=if self.motion.reduced(){0.0}else{((time%period-1.75)/0.75).clamp(0.0,1.0)*crate::split_flap::duration(10)};
            let next=(index+1)%APPS.len();let scale=self.scale;
            let paper=crate::surface::mix(self.theme.paper,self.theme.ink,if self.theme.mode==nus_render::Mode::Paper{0.90}else{0.05});
            let ink=if self.theme.mode==nus_render::Mode::Paper{self.theme.paper}else{self.theme.ink};
            let icon=|i:usize|match APPS[i].1{0=>icons::GLOBE,1=>icons::TERMINAL,_=>icons::CODE};
            let ir=Rect::new(rx,clip.y+self.px(2.0),self.px(24.0),self.px(32.0));
            let st=Style{font:self.f.term,px:self.px(14.0),color:ink,tracking:0.0};
            let outer=scene.clip();scene.layer(Some(outer.map_or(clip,|r|r.intersect(&clip))));
            crate::split_flap::cell(&mut self.fonts,scene,st,ir,crate::split_flap::Face::Icon(icon(index)),crate::split_flap::Face::Icon(icon(next)),elapsed/crate::split_flap::TURN,paper,self.surface.signal,scale);
            let text_r=Rect::new(ir.right()+self.px(3.0),ir.y,(width-ir.w-self.px(3.0)).max(1.0),ir.h);
            let cw=(text_r.w/9.0).floor().max(1.0);
            let st=Style{px:st.px.min(((cw-self.px(2.0))/0.61).max(1.0)),..st};
            crate::split_flap::text(&mut self.fonts,scene,st,text_r,&APPS[index].0.to_uppercase(),&APPS[next].0.to_uppercase(),cw,elapsed-crate::split_flap::STAGGER,paper,self.surface.signal,scale);
            scene.layer(outer);self.fonts.draw(scene,Style{color:self.surface.signal,..ui},rx+width+self.px(5.0),y,"↗");
            let hit=Rect::new(bx,y-self.px(26.0),bw,self.px(42.0));self.me_card.hits.push((hit,CardHit::ImportOpen));
            if self.me_card.import.focus==1{scene.outline(hit,self.px(1.0),self.surface.signal);}
            self.fonts.draw(scene,dim,bx,y+self.px(30.0),"Browsers, terminals and editors");
            y+=self.px(58.0);
            self.me_button(scene,bx,foot,"CONTINUE",true,CardHit::Next);
        }else if step==Some(Step::ImportSources){
            self.fonts.draw(scene,self.ui_strong(),bx,y+self.px(12.0),"Where are you coming from?");y+=self.px(30.0);
            let cols=3;let cw=(bw-self.px(16.0))/cols as f32;
            for (i,(name,_)) in APPS.iter().enumerate(){let r=Rect::new(bx+(i%cols)as f32*(cw+self.px(8.0)),y+(i/cols)as f32*self.px(36.0),cw,self.px(29.0));let selected=i==self.me_card.import.source;
                scene.rect(r,if selected{crate::app::fade(self.surface.signal,0.1)}else{self.theme.paper});scene.outline(r,self.px(1.0),if selected{self.surface.signal}else{self.theme.tint});
                self.fonts.draw(scene,ui,r.x+self.px(7.0),r.y+self.px(20.0),name);self.me_card.hits.push((r,CardHit::ImportSource(i)));}
            y+=self.px(158.0);
            let hint=if APPS[self.me_card.import.source].1==0{"Bookmarks HTML or Chromium Bookmarks JSON. Saved links go into a sidebar folder."}else{"Color themes: Ghostty, VS Code, Windows Terminal or base16. Colors only; running processes stay in the original app."};
            for line in crate::reader::wrap(&self.fonts,dim,hint,bw){self.fonts.draw(scene,dim,bx,y,&line);y+=self.px(18.0);}
            self.me_button(scene,bx,foot,"CHOOSE EXPORT",true,CardHit::ImportPick);self.me_button(scene,bx+self.px(156.0),foot,"BACK",false,CardHit::Back);
        }else{
            self.fonts.draw(scene,title,bx,y+self.px(22.0),"Review your import.");y+=self.px(49.0);
            if let Some(plan)=&self.me_card.import.plan{
                for line in crate::reader::wrap(&self.fonts,ui,&plan.summary(),bw){self.fonts.draw(scene,ui,bx,y,&line);y+=self.px(20.0);}
                if let Plan::Bookmarks(items)=plan {for item in items.iter().take(4){let text=self.fit(dim,&format!("{} · {}",item.title,item.url),bw);self.fonts.draw(scene,dim,bx,y+self.px(12.0),&text);y+=self.px(23.0);}}
            }
            y+=self.px(20.0);for line in crate::reader::wrap(&self.fonts,dim,"Existing items stay. Duplicate links are skipped. Nothing opens or runs, and your original files are unchanged.",bw){self.fonts.draw(scene,dim,bx,y,&line);y+=self.px(18.0);}
            self.me_button(scene,bx,foot,"IMPORT",true,CardHit::ImportApply);self.me_button(scene,bx+self.px(106.0),foot,"BACK",false,CardHit::Back);
        }
        for line in crate::reader::wrap(&self.fonts,dim,&self.me_card.import.message,bw).into_iter().take(3){if y+self.px(16.0)>foot-self.px(34.0){break;}self.fonts.draw(scene,dim,bx,y+self.px(16.0),&line);y+=self.px(18.0);}
    }
}

#[cfg(test)]mod tests{use super::*;
    #[test]fn bookmarks_are_previewed_filtered_and_deduplicated(){let Plan::Bookmarks(v)=parse(0,r#"<A HREF="https://example.org/?a=1&amp;b=2">One &amp; two</A><a href="javascript:alert(1)">bad</a><a href="https://example.org/?a=1&amp;b=2">duplicate</a>"#).unwrap() else{panic!()};assert_eq!(v.len(),1);assert_eq!(v[0].title,"One & two");assert!(v[0].url.contains("&b=2"));}
    #[test]fn nested_json_and_credentials(){let Plan::Bookmarks(v)=parse(0,r#"{"roots":{"bar":{"children":[{"url":"https://user:secret@example.org","name":"secret"},{"url":"https://example.org","name":"safe"}]}}}"#).unwrap() else{panic!()};assert_eq!(v.len(),1);assert_eq!(v[0].title,"safe");}
    #[test]fn themes_copy_colors_only(){let Plan::Theme(v)=parse(1,"background = #112233\nforeground = #eeeeee\ncommand = curl secret\npassword=secret").unwrap() else{panic!()};assert!(!v.contains("secret"));assert!(!v.contains("command"));assert!(crate::theme_edit::parse_theme("test",&v).is_some());}
}

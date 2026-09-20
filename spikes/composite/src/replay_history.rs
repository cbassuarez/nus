//! Command history is the default view. A terminal snapshot and a portable
//! player are explicit choices; no action here ever writes to the live PTY.
use super::*;
use crate::app::{fade, TermPane};
use nus_render::text::Style;
use winit::keyboard::{Key,NamedKey};

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum HistoryHit {Search,Record(usize),CopyCommand,CopyOutput,Snapshot,Playback,Live,Map}
impl HistoryHit {
    pub(crate) fn label(self)-> &'static str {match self {Self::Search=>"Search commands and output",Self::Record(_)=>"Command",Self::CopyCommand=>"Copy command",Self::CopyOutput=>"Copy output",Self::Snapshot=>"Terminal view",Self::Playback=>"Open playback and export",Self::Live=>"Return to live shell",Self::Map=>"Session map: drag to browse, arrow keys step between commands"}}
}
fn field(v:&Value,key:&str)->String {v[key].as_str().unwrap_or("").to_string()}
fn metadata(v:&Value)->String {
    let status=match v["exit"].as_i64(){Some(0)=>"Completed".into(),Some(n)=>format!("Exit {n}"),None=>"Recorded".into()};
    let duration=v["ms"].as_u64().map(|n|format!(" · {:.1}s",n as f64/1000.0)).unwrap_or_default();
    format!("{status}{duration} · {}",field(v,"cwd"))
}
impl Timeline {
    pub(crate) fn matches(&self)->Vec<usize>{
        let terms:Vec<String>=self.query.split_whitespace().map(str::to_lowercase).collect();
        self.records.iter().enumerate().filter(|(_,v)|{
            let text=format!("{} {} {} {}",field(v,"cmd"),field(v,"cwd"),field(v,"output"),metadata(v)).to_lowercase();
            terms.iter().all(|q|text.contains(q))
        }).map(|(i,_)|i).collect()
    }
    pub(crate) fn filter_changed(&mut self){let matches=self.matches();if !matches.contains(&self.at){if let Some(i)=matches.first(){self.at=*i;}}self.detail_scroll=0.0;self.reveal=true;}
}
impl App {
    pub(crate) fn timeline_key(&mut self,key:&Key)->bool {
        let page=self.px(240.0);
        let Some(tl)=self.timeline.as_mut() else{return false;};
        let chord=self.mods.control_key() || self.mods.super_key();
        if chord {
            match key {
                Key::Character(c) if c.eq_ignore_ascii_case("f")=>tl.focus=Some(HistoryHit::Search),
                Key::Character(c) if c.eq_ignore_ascii_case("c")=>{self.timeline_action(HistoryHit::CopyOutput);return true;},
                _=>{}
            }
            self.dirty=true;return true;
        }
        match key {
            Key::Named(NamedKey::Escape)=>{
                if tl.snapshot {tl.snapshot=false;}else if !tl.query.is_empty(){tl.query.clear();tl.filter_changed();}else{self.close_timeline();return true;}
            }
            Key::Named(NamedKey::Tab)=>{
                let order=[HistoryHit::Search,HistoryHit::Record(tl.at),HistoryHit::CopyCommand,HistoryHit::CopyOutput,HistoryHit::Snapshot,HistoryHit::Playback,HistoryHit::Map,HistoryHit::Live];let n=order.len();
                let back=self.mods.shift_key();let i=tl.focus.and_then(|h|order.iter().position(|v|*v==h)).map(|i|if back{(i+n-1)%n}else{(i+1)%n}).unwrap_or(0);tl.focus=Some(order[i]);
            }
            Key::Named(NamedKey::Enter|NamedKey::Space) if tl.focus!=Some(HistoryHit::Search)=>{let hit=tl.focus.unwrap_or(HistoryHit::Snapshot);self.timeline_action(hit);return true;},
            Key::Named(NamedKey::ArrowUp|NamedKey::ArrowLeft|NamedKey::ArrowDown|NamedKey::ArrowRight|NamedKey::Home|NamedKey::End)=>{
                let matches=tl.matches();let n=matches.len();
                if n>0 {let i=matches.iter().position(|i|*i==tl.at).unwrap_or(0);let next=match key {Key::Named(NamedKey::Home)=>0,Key::Named(NamedKey::End)=>n-1,Key::Named(NamedKey::ArrowUp|NamedKey::ArrowLeft)=>i.saturating_sub(1),_=>(i+1).min(n-1)};
                    tl.at=matches[next];tl.reveal=true;if matches!(tl.focus,Some(HistoryHit::Record(_))){tl.focus=Some(HistoryHit::Record(tl.at));}}
            }
            Key::Named(NamedKey::PageDown)=>{tl.detail_scroll=(tl.detail_scroll+page).min(tl.detail_max);tl.follow_scroll=true;},
            Key::Named(NamedKey::PageUp)=>{tl.detail_scroll=(tl.detail_scroll-page).max(0.0);tl.follow_scroll=true;},
            Key::Named(NamedKey::Backspace) if tl.focus==Some(HistoryHit::Search)=>{tl.query.pop();tl.filter_changed();},
            Key::Character(c) if tl.snapshot && c.eq_ignore_ascii_case("b")=>tl.mode=match tl.mode {Compare::After=>Compare::Before,Compare::Before=>Compare::Diff,Compare::Diff=>Compare::After},
            Key::Character(c) if !self.mods.alt_key()=>{tl.snapshot=false;tl.focus=Some(HistoryHit::Search);tl.query.push_str(c);tl.filter_changed();},
            Key::Named(NamedKey::Space) if tl.focus==Some(HistoryHit::Search)=>{tl.query.push(' ');tl.filter_changed();},
            _=>{}
        }
        if tl.snapshot {tl.rebuild(&self.theme);self.timeline_apply();}
        self.dirty=true;true
    }
    pub(crate) fn timeline_action(&mut self,hit:HistoryHit) {
        let Some(tl)=self.timeline.as_mut() else{return;};
        tl.focus=Some(hit);
        if matches!(hit,HistoryHit::CopyCommand|HistoryHit::CopyOutput|HistoryHit::Snapshot)&&tl.matches().is_empty(){return;}
        match hit {
            HistoryHit::Live=>{self.close_timeline();return;},
            HistoryHit::Search|HistoryHit::Map=>{},
            HistoryHit::Record(i)=>{if i<tl.count(){tl.at=i;tl.reveal=true;}},
            HistoryHit::Snapshot=>{tl.snapshot=!tl.snapshot;},
            HistoryHit::Playback=>{self.run(crate::app::Action::ShareReplay);return;},
            HistoryHit::CopyCommand|HistoryHit::CopyOutput=>{
                let text=tl.records.get(tl.at).map(|v|field(v,if hit==HistoryHit::CopyCommand{"cmd"}else{"output"})).unwrap_or_default();
                match arboard::Clipboard::new().and_then(|mut cb|cb.set_text(text)) {Ok(())=>self.notice(if hit==HistoryHit::CopyCommand{"Command copied"}else{"Output copied"}),Err(_)=>self.notice("Could not access the clipboard")};return;
            }
        }
        if tl.snapshot{tl.rebuild(&self.theme);self.timeline_apply();}
        self.dirty=true;
    }
    pub(crate) fn timeline_mouse(&mut self,button:winit::event::MouseButton,state:winit::event::ElementState,x:f32,y:f32)->bool {
        let Some(tl)=self.timeline.as_mut() else{return false;};
        if !tl.area.contains(x,y) && tl.map_drag.is_none(){return false;}
        let left=button==winit::event::MouseButton::Left;let pressed=state==winit::event::ElementState::Pressed;
        if left && !pressed && tl.map_drag.take().is_some(){self.timeline_detent();return true;}
        if !left || !pressed{return true;}
        if !tl.snapshot && tl.list_area.contains(x,y){
            let total=(tl.detail_max+tl.detail_area.h).max(1.0);let map=tl.list_area;
            let top=map.y+tl.detail_scroll/total*map.h;let height=(tl.detail_area.h/total*map.h).max(8.0).min(map.h);
            tl.focus=Some(HistoryHit::Map);tl.reveal=false;tl.follow_scroll=true;
            if y>=top && y<=top+height{tl.map_drag=Some(y-top);}else{
                // Clicking the map lands at a command boundary, not an arbitrary frame.
                let at=((y-map.y)/map.h*total).max(0.0);
                if let Some((i,start,_))=tl.ranges.iter().min_by(|a,b|(a.1-at).abs().total_cmp(&(b.1-at).abs())) {tl.at=*i;tl.detail_scroll=start.min(tl.detail_max);}
                tl.map_drag=Some(0.0);
            }
            self.dirty=true;return true;
        }
        if let Some((_,hit))=tl.hits.iter().find(|(r,_)|r.contains(x,y)).copied(){self.timeline_action(hit);}
        true
    }
    pub(crate) fn timeline_pointer(&mut self,x:f32,y:f32)->bool {
        let Some(tl)=self.timeline.as_mut() else{return false;};
        if let Some(grab)=tl.map_drag {let total=(tl.detail_max+tl.detail_area.h).max(1.0);tl.detail_scroll=((y-tl.list_area.y-grab)/tl.list_area.h.max(1.0)*total).clamp(0.0,tl.detail_max);tl.reveal=false;tl.follow_scroll=true;self.dirty=true;return true;}
        tl.area.contains(x,y)
    }
    pub(crate) fn timeline_detent(&mut self){
        let threshold=self.px(12.0);let Some(tl)=&mut self.timeline else{return;};
        if let Some((_,start,_))=tl.ranges.iter().min_by(|a,b|(a.1-tl.detail_scroll).abs().total_cmp(&(b.1-tl.detail_scroll).abs())) {if (start-tl.detail_scroll).abs()<threshold{tl.detail_scroll=start.min(tl.detail_max);self.dirty=true;}}
    }
    pub(crate) fn timeline_wheel(&mut self,x:f32,y:f32,dy:f32)->bool {
        let Some(tl)=&mut self.timeline else{return false;};if !tl.area.contains(x,y){return false;}
        tl.detail_scroll=(tl.detail_scroll-dy).clamp(0.0,tl.detail_max);tl.reveal=false;tl.follow_scroll=true;self.dirty=true;true
    }
    pub(crate) fn draw_timeline_ruler(&mut self,scene:&mut Scene,p:&TermPane,r:Rect) {
        if !p.replay{return;}
        let Some(mut tl)=self.timeline.take() else{return;};
        let header=if p.show_header{self.header_h()}else{0.0};
        let r=Rect::new(r.x,r.y+header,r.w,(r.h-header).max(0.0));tl.area=r;tl.hits.clear();
        let pad=self.px(18.0);let line=self.px(22.0);
        let label=self.label();let mut foot_rows=1;let mut used=pad;
        for words in ["Copy command","Copy output","Terminal view","Playback / export","Return to live"]{let w=self.fonts.measure(label,words)+self.px(23.0);if used+w>r.w-self.px(8.0){foot_rows+=1;used=pad;}used+=w;}
        let foot_h=self.px(foot_rows as f32*34.0+10.0);
        let head_h=self.px(96.0);let paper=self.paper();let ink=self.theme.ink;let dim=self.theme.dim;let signal=self.surface.signal;
        scene.layer(Some(r));
        if !tl.snapshot{scene.rect(r,paper);}else{scene.rect(Rect::new(r.x,r.y,r.w,head_h),paper);}
        let body=Style{font:self.f.term,px:self.px(13.0),color:ink,tracking:0.0};
        self.fonts.draw(scene,Style{px:self.px(19.0),..label},r.x+pad,r.y+self.px(28.0),if tl.snapshot{"Terminal snapshot"}else{"Command history"});
        let matches=tl.matches();let n=matches.len();
        let subtitle=if tl.snapshot{format!("{:?} · ← / → commands · B compares page stills",tl.mode)}else{format!("{n} of {} commands · ↑ / ↓ steps commands · drag the map to browse",tl.count())};
        let subtitle=self.fit(Style{color:dim,..label},&subtitle,(r.w-pad*2.0).max(1.0));
        self.fonts.draw(scene,Style{color:dim,..label},r.x+pad,r.y+self.px(49.0),&subtitle);
        let search=Rect::new(r.x+pad,r.y+self.px(59.0),(r.w-2.0*pad).max(1.0),self.px(27.0));
        scene.rect(search,fade(ink,0.04));scene.hline(search.x,search.bottom(),search.w,self.px(1.0),if tl.focus==Some(HistoryHit::Search){signal}else{fade(ink,0.25)});
        let words=if tl.query.is_empty(){"Search commands, folders, output…"}else{&tl.query};let fit=self.fit(label,words,search.w-pad);
        self.fonts.draw(scene,Style{color:if tl.query.is_empty(){dim}else{ink},..label},search.x+self.px(8.0),search.y+self.px(18.0),&fit);tl.hits.push((search,HistoryHit::Search));
        let content=Rect::new(r.x,r.y+head_h,r.w,(r.h-head_h-foot_h).max(0.0));
        if !tl.snapshot && content.h>0.0 {
            let map_w=self.px(if r.w<self.px(500.0){76.0}else{150.0}).min(content.w*0.28);
            let map=Rect::new(content.right()-map_w,content.y,map_w,content.h);
            let detail=Rect::new(content.x,content.y,content.w-map_w,content.h);
            tl.list_area=map;tl.detail_area=detail;
            let cw=self.fonts.measure(body,"M").max(1.0);let cols=((detail.w-pad*2.0)/cw).max(1.0)as usize;
            // One continuous document. The map uses these exact same line positions.
            let mut lines:Vec<(usize,String,u8)>=Vec::new();tl.ranges.clear();
            for i in &matches {
                let v=&tl.records[*i];let start=lines.len()as f32*line;
                for (text,kind) in [(format!("$ {}",field(v,"cmd")),1),(metadata(v),2),(String::new(),0),(field(v,"output"),0),(String::new(),0)] {
                    for text in text.split('\n') {let chars:Vec<char>=text.chars().collect();if chars.is_empty(){lines.push((*i,String::new(),kind));}else{for chunk in chars.chunks(cols){lines.push((*i,chunk.iter().collect(),kind));}}}
                }
                tl.ranges.push((*i,start,lines.len()as f32*line));
            }
            let total=(lines.len()as f32*line+pad*2.0).max(detail.h);tl.detail_max=(total-detail.h).max(0.0);
            if tl.reveal {tl.follow_scroll=false;if let Some((_,start,_))=tl.ranges.iter().find(|(i,_,_)|*i==tl.at){tl.detail_scroll=*start;}tl.reveal=false;}
            tl.detail_scroll=tl.detail_scroll.clamp(0.0,tl.detail_max);
            if tl.follow_scroll {if let Some((i,_,_))=tl.ranges.iter().rev().find(|(_,start,_)|*start<=tl.detail_scroll+line){tl.at=*i;}tl.follow_scroll=false;}
            scene.layer(Some(detail));scene.rect(detail,paper);
            for (i,(_,text,kind)) in lines.iter().enumerate(){let y=detail.y+pad+i as f32*line-tl.detail_scroll;if y+line<detail.y || y>detail.bottom(){continue;}
                if *kind==1 {scene.rect(Rect::new(detail.x,y,detail.w,line),fade(signal,0.055));}
                let found=!tl.query.is_empty()&&text.to_lowercase().contains(&tl.query.to_lowercase());if found {scene.rect(Rect::new(detail.x+pad,y,(detail.w-pad*2.0).max(1.0),line),fade(signal,0.14));}
                self.fonts.draw(scene,Style{color:match kind{1=>signal,2=>dim,_=>ink},..body},detail.x+pad,y+line*0.78,text);
            }
            if n==0{self.fonts.draw(scene,Style{color:dim,..label},detail.x+pad,detail.y+pad+line,"No matching commands");}
            // The document at a glance: actual output line lengths, command ticks,
            // failures, and a viewport whose size reflects the visible history.
            scene.layer(Some(map));scene.rect(map,fade(ink,0.035));scene.rect(Rect::new(map.x,map.y,self.px(1.0),map.h),fade(ink,0.2));
            let mini=line/total*map.h;let left=map.x+self.px(12.0);let width=(map.w-self.px(24.0)).max(1.0);
            let stride=(lines.len() as f32/(map.h.max(1.0)*1.5)).ceil().max(1.0)as usize;
            for (row,(_,text,kind)) in lines.iter().enumerate().step_by(stride){if text.is_empty(){continue;}
                let y=map.y+(pad+row as f32*line)/total*map.h;
                let indent=text.chars().take_while(|c|c.is_whitespace()).count()as f32/cols as f32*width;
                let w=(text.trim().chars().count()as f32/cols as f32*width).min(width-indent).max(1.0);
                scene.rect(Rect::new(left+indent,y,w,(mini*0.7).clamp(0.6,self.px(2.0))),fade(if *kind==1{signal}else{ink},if *kind==1{0.8}else{0.25}));
            }
            for (i,start,_) in &tl.ranges {let y=map.y+*start/total*map.h;let failed=tl.records[*i]["exit"].as_i64().is_some_and(|n|n!=0);
                scene.rect(Rect::new(map.x,y,self.px(if failed{7.0}else{4.0}),self.px(if failed{4.0}else{2.0})),if failed{signal}else{fade(ink,0.6)});
                if map.contains(self.mouse.0,self.mouse.1)&&(self.mouse.1-y).abs()<self.px(6.0){self.tip_words(Rect::new(map.x,y,map.w,self.px(8.0)),&format!("{} · {}",field(&tl.records[*i],"cmd"),metadata(&tl.records[*i])));}
            }
            let view=Rect::new(map.x,map.y+tl.detail_scroll/total*map.h,map.w,(detail.h/total*map.h).max(self.px(8.0)).min(map.h));
            scene.rect(view,fade(signal,0.09));scene.push(nus_render::Instance::stroke(view,0.0,self.px(if tl.focus==Some(HistoryHit::Map){2.0}else{1.0}),fade(signal,0.85),None,0.0));
            tl.hits.push((map,HistoryHit::Map));
            for (i,start,end) in &tl.ranges {let y=detail.y+pad+*start-tl.detail_scroll;let bottom=detail.y+pad+*end-tl.detail_scroll;if bottom>detail.y && y<detail.bottom(){tl.hits.push((Rect::new(detail.x,y.max(detail.y),detail.w,bottom.min(detail.bottom())-y.max(detail.y)),HistoryHit::Record(*i)));}}
        }
        let foot=Rect::new(r.x,r.bottom()-foot_h,r.w,foot_h);scene.layer(Some(foot));scene.rect(foot,paper);scene.hline(foot.x,foot.y,foot.w,self.px(1.0),fade(ink,0.25));
        let mut x=foot.x+pad;let mut y=foot.y+self.px(8.0);
        for (hit,words) in [(HistoryHit::CopyCommand,"Copy command"),(HistoryHit::CopyOutput,"Copy output"),(HistoryHit::Snapshot,if tl.snapshot{"History"}else{"Terminal view"}),(HistoryHit::Playback,"Playback / export"),(HistoryHit::Live,"Return to live")]{
            let width=self.fonts.measure(label,words)+self.px(16.0);if x+width>foot.right()-self.px(8.0){x=foot.x+pad;y+=self.px(34.0);}
            let b=Rect::new(x,y,width,self.px(27.0));let focused=tl.focus==Some(hit);scene.rect(b,fade(if hit==HistoryHit::Live{signal}else{ink},if focused{0.18}else{0.055}));
            if focused{scene.push(nus_render::Instance::stroke(b,0.0,self.px(1.0),signal,None,0.0));}
            self.fonts.draw(scene,Style{color:if hit==HistoryHit::Live{signal}else{ink},..label},x+self.px(8.0),y+self.px(18.0),words);tl.hits.push((b,hit));x+=width+self.px(7.0);
        }
        scene.layer(None);self.timeline=Some(tl);
    }
}

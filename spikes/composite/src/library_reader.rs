//! Saved-reading interaction state layered on the existing Newsreader renderer.
//! It owns no browser, does no disk I/O, and never resolves a remote image URL.
use std::collections::HashMap;
use std::sync::Arc;
use nus_render::text::{FontId, Style};
use nus_render::{FontSystem, Rect};
use crate::reader::{Block, Kind, Reader};
use crate::library::store::{self as reading, Position};

pub struct Picture { pub width:u32, pub height:u32, pub texture:Arc<wgpu::BindGroup> }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point { pub line:usize, pub byte:usize }
#[derive(Clone, Debug)]
pub enum Hit { Link(String), Code(usize) }
#[derive(Default)]
pub struct Extras {
    pub offline:bool,
    pub pictures:HashMap<String,Picture>,
    pub image_heights:HashMap<usize,f32>,
    pub links:HashMap<usize,String>,
    pub code:HashMap<usize,String>,
    /// (first line, exclusive last line, article block index).
    pub blocks:Vec<(usize,usize,usize)>,
    pub hits:Vec<(Rect,Hit)>,
    pub drawn:Vec<(usize,Rect,Style)>,
    pub selection:Option<(Point,Point)>,
    pub dragging:bool,
    pub viewport:Option<Rect>,
    pub scale:f32,
    pub font_key:Option<[FontId;4]>,
    /// Horizontal navigation is confined to preformatted blocks.
    pub horizontal:f32,
    pub horizontal_max:f32,
    pub find:Option<String>,
    pub found:Option<usize>,
}
impl Extras {
    pub fn reset_layout(&mut self) {
        self.image_heights.clear(); self.links.clear(); self.code.clear();
        self.blocks.clear(); self.hits.clear(); self.drawn.clear();
        self.selection=None; self.dragging=false; self.horizontal_max=0.0;
    }
}
fn block_text(b:&Block)->String {
    match b {
        Block::Heading(_,s)|Block::Para(s)|Block::Pre(s)|Block::Item(s)|Block::Quote(s)|Block::Caption(s)=>s.clone(),
        Block::Link(s,u)=>format!("{s} · {u}"),
        Block::Image(s,_)=>s.clone(),
    }
}
fn normalized(s:&str)->String {s.split_whitespace().collect::<Vec<_>>().join(" ")}
fn char_tail(s:&str,n:usize)->String {s.chars().rev().take(n).collect::<Vec<_>>().into_iter().rev().collect()}
fn char_head(s:&str,n:usize)->String {s.chars().take(n).collect()}

impl Reader {
    /// Match wrapped runs back to normalized source without inventing spaces
    /// between long-word/CJK fragments. Block identity is retained separately.
    fn source_spans(&self,block:usize)->Vec<(usize,usize,usize)> {
        let Some(source)=self.article.blocks.get(block) else{return vec![];};
        if matches!(source,Block::Image(..)){return vec![];}
        let text=normalized(&block_text(source));
        let Some(&(a,b,_))=self.saved.blocks.iter().find(|(_,_,i)|*i==block) else{return vec![];};
        let mut cursor=0usize;let mut spans=Vec::new();
        for i in a..b {
            let line=&self.lines[i];
            if matches!(line.kind,Kind::Image|Kind::CodeAction|Kind::Rule){continue;}
            let part=normalized(&line.text);
            if part.is_empty(){continue;}
            if let Some(offset)=text[cursor..].find(&part){
                let start=cursor+offset;cursor=start+part.len();spans.push((i,start,cursor));
            }
        }
        spans
    }
    pub fn reading_position(&self,snapshot:&str)->Option<Position> {
        let r=self.saved.viewport?;
        if self.lines.is_empty(){return None;}
        let target=(self.scroll-28.0*self.saved.scale).max(0.0);
        let (first,last,block)=self.saved.blocks.iter().copied().find(|(a,b,block)|
            *a<*b && self.lines[*b-1].y>=target && !matches!(self.article.blocks.get(*block),Some(Block::Image(..))))?;
        let _=(first,last);
        let spans=self.source_spans(block);
        let (_,offset,_)=spans.iter().copied().find(|(i,_,_)|self.lines[*i].y>=target).or_else(||spans.last().copied())?;
        let text=normalized(&block_text(self.article.blocks.get(block)?));
        let quote=char_head(&text[offset..],96);
        if quote.is_empty(){return None;}
        let end=offset+quote.len();
        Some(Position{quote,before:char_tail(&text[..offset],32),after:char_head(&text[end..],32),
            fraction:(self.scroll/(self.height-r.h).max(1.0)).clamp(0.0,1.0),snapshot:snapshot.into(),
            block:Some(block as u32),offset:offset as u32,block_hash:reading::digest(text.as_bytes())})
    }
    /// Returns false only when an approximate scroll-fraction fallback was used.
    pub fn restore_reading_position(&mut self,p:&Position,snapshot:&str)->bool {
        let Some(r)=self.saved.viewport else {return false};
        let mut located=None;
        if let Some(i)=p.block.filter(|_|p.snapshot==snapshot).map(|b|b as usize) {
            if let Some(b)=self.article.blocks.get(i) {
                let t=normalized(&block_text(b));
                if reading::digest(t.as_bytes())==p.block_hash && (p.offset as usize)<=t.len() && t.is_char_boundary(p.offset as usize) {
                    located=Some((i,p.offset as usize));
                }
            }
        }
        if located.is_none() && !p.quote.is_empty() {
            let hits:Vec<_>=self.article.blocks.iter().enumerate().filter_map(|(i,b)|reading::locate_quote(&normalized(&block_text(b)),p).map(|at|(i,at))).collect();
            if hits.len()==1 {located=Some(hits[0]);}
        }
        if let Some((block,offset))=located {
            let spans=self.source_spans(block);
            if let Some((i,_,_))=spans.iter().copied().find(|(_,a,b)|offset>=*a&&offset<*b).or_else(||spans.iter().copied().find(|(_,a,_)|*a>=offset)) {
                self.scroll=(self.lines[i].y+28.0*self.saved.scale).clamp(0.0,(self.height-r.h).max(0.0));
                return true;
            }
        }
        self.scroll=p.fraction.clamp(0.0,1.0)*(self.height-r.h).max(0.0);
        false
    }
    pub fn reading_point(&self,fonts:&FontSystem,x:f32,y:f32)->Option<Point> {
        let (i,r,style)=self.saved.drawn.iter().min_by(|a,b| {
            let distance=|r:&Rect|if y<r.y {r.y-y}else if y>r.bottom(){y-r.bottom()}else{0.0};
            distance(&a.1).total_cmp(&distance(&b.1))
        })?;
        let text=&self.lines[*i].text;
        let mut boundaries:Vec<_>=text.char_indices().map(|(i,_)|i).collect();boundaries.push(text.len());
        let goal=(x-r.x).max(0.0);
        let (mut lo,mut hi)=(0usize,boundaries.len()-1);
        while lo<hi {
            let mid=(lo+hi+1)/2;
            if fonts.measure(*style,&text[..boundaries[mid]])<=goal {lo=mid;}else{hi=mid-1;}
        }
        Some(Point{line:*i,byte:boundaries[lo]})
    }
    pub fn reading_selected_text(&self)->String {
        let Some((a,b))=self.saved.selection else {return String::new()};
        let (a,b)=if a<=b{(a,b)}else{(b,a)};
        let mut out=String::new();
        let mut seams=HashMap::new();
        for (block,source) in self.article.blocks.iter().enumerate(){
            if matches!(source,Block::Pre(_)|Block::Image(..)){continue;}
            let text=normalized(&block_text(source));let spans=self.source_spans(block);
            for pair in spans.windows(2){
                let (_,_,end)=pair[0];let (line,start,_)=pair[1];
                if let Some(gap)=text.get(end..start){seams.insert(line,gap.to_string());}
            }
        }
        for i in a.line..=b.line.min(self.lines.len().saturating_sub(1)) {
            let Some(line)=self.lines.get(i) else {continue};
            if matches!(line.kind,Kind::Image|Kind::CodeAction|Kind::Rule){continue;}
            let start=if i==a.line{a.byte}else{0};let end=if i==b.line{b.byte}else{line.text.len()};
            if let Some(part)=line.text.get(start..end) {
                if !out.is_empty() {
                    let same=self.saved.blocks.iter().any(|(s,e,_)|i>*s && i<*e);
                    if same && line.kind!=Kind::Mono{out.push_str(seams.get(&i).map(String::as_str).unwrap_or(" "));}else{out.push('\n');}
                }
                out.push_str(part);
            }
        }
        out
    }
    pub fn reading_select_all(&mut self) {
        if let Some(last)=self.lines.last() {
            self.saved.selection=Some((Point::default(),Point{line:self.lines.len()-1,byte:last.text.len()}));
        }
    }
    pub fn reading_find(&mut self,forward:bool) {
        let Some(q)=self.saved.find.as_ref().filter(|q|!q.is_empty()) else {self.saved.found=None;return;};
        let q=normalized(q).to_lowercase();
        if q.is_empty(){self.saved.found=None;return;}
        let mut hits:Vec<usize>=self.lines.iter().enumerate().filter(|(_,l)|matches!(l.kind,Kind::Title|Kind::Byline)&&normalized(&l.text).to_lowercase().contains(&q)).map(|(i,_)|i).collect();
        for (block,b) in self.article.blocks.iter().enumerate(){
            if matches!(b,Block::Image(..)){continue;}
            let text=normalized(&block_text(b));let lower=text.to_lowercase();
            // Build the byte-offset map once. Re-lowercasing each progressively
            // longer prefix would be quadratic for one very long paragraph.
            let mut offsets=Vec::new();let mut folded_len=0usize;
            for (at,c) in text.char_indices(){offsets.push((at,folded_len));folded_len+=c.to_lowercase().map(char::len_utf8).sum::<usize>();}
            offsets.push((text.len(),folded_len));
            let at=|byte:usize|offsets.binary_search_by_key(&byte,|(a,_)|*a).ok().map(|i|offsets[i].1);
            let spans:Vec<_>=self.source_spans(block).into_iter().filter_map(|(i,a,b)|Some((i,at(a)?,at(b)?))).collect();
            for (offset,_) in lower.match_indices(&q){
                if let Some((i,_,_))=spans.iter().find(|(_,a,b)|offset>=*a&&offset<*b){hits.push(*i);}
            }
        }
        hits.sort_unstable();hits.dedup();
        let next=if forward {hits.iter().copied().find(|&i|self.saved.found.is_none_or(|old|i>old)).or_else(||hits.first().copied())}
                 else {hits.iter().rev().copied().find(|&i|self.saved.found.is_none_or(|old|i<old)).or_else(||hits.last().copied())};
        self.saved.found=next;
        if let Some(i)=next {
            let h=self.saved.viewport.map(|r|r.h).unwrap_or(0.0);
            self.scroll=(self.lines[i].y-48.0*self.saved.scale).clamp(0.0,(self.height-h).max(0.0));
        }
    }
    /// Keyboard selection uses UTF-8 boundaries; it never slices through a scalar.
    pub fn reading_move_caret(&mut self,right:bool,extend:bool) {
        if self.lines.is_empty(){return;}
        let (anchor,mut end)=self.saved.selection.unwrap_or((Point::default(),Point::default()));
        end.line=end.line.min(self.lines.len()-1);
        let line=&self.lines[end.line].text;
        if right {
            if end.byte<line.len() {end.byte=line.char_indices().map(|(i,_)|i).find(|i|*i>end.byte).unwrap_or(line.len());}
            else if end.line+1<self.lines.len(){end.line+=1;end.byte=0;}
        } else if end.byte>0 {end.byte=line.char_indices().map(|(i,_)|i).take_while(|i|*i<end.byte).last().unwrap_or(0);}
        else if end.line>0{end.line-=1;end.byte=self.lines[end.line].text.len();}
        self.saved.selection=Some((if extend{anchor}else{end},end));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::{Article, Line};

    fn laid(source: &str, lines: &[&str]) -> Reader {
        let mut r=Reader::new(Article{title:"Title".into(),blocks:vec![Block::Para(source.into())],..Default::default()});
        r.lines=lines.iter().enumerate().map(|(i,s)|Line{kind:Kind::Body,text:(*s).into(),dx:0.0,y:30.0+i as f32*30.0}).collect();
        r.saved.blocks=vec![(0,lines.len(),0)];r.saved.scale=1.0;r.saved.viewport=Some(Rect::new(0.0,0.0,300.0,90.0));r.height=500.0;
        r
    }
    #[test]
    fn split_cjk_runs_keep_their_source_offsets() {
        let r=laid("日本語の記事です",&["日本語の","記事です"]);
        assert_eq!(r.source_spans(0),vec![(0,0,12),(1,12,24)]);
    }
    #[test]
    fn copying_wrapped_cjk_does_not_invent_spaces() {
        let mut r=laid("日本語の記事です",&["日本語の","記事です"]);
        r.reading_select_all();assert_eq!(r.reading_selected_text(),"日本語の記事です");
    }
    #[test]
    fn copying_ordinary_wraps_keeps_word_boundaries() {
        let mut r=laid("one two three four",&["one two","three four"]);
        r.reading_select_all();assert_eq!(r.reading_selected_text(),"one two three four");
    }
    #[test]
    fn find_can_cross_a_cjk_wrap_boundary() {
        let mut r=laid("日本語の記事です",&["日本語の","記事です"]);
        r.saved.find=Some("語の記事".into());r.reading_find(true);assert_eq!(r.saved.found,Some(0));
    }
    #[test]
    fn semantic_position_survives_reflow() {
        let mut before=laid("one two three four",&["one two","three four"]);
        before.scroll=88.0;let p=before.reading_position("version").unwrap();assert_eq!(p.offset,8);
        let mut after=laid("one two three four",&["one","two three","four"]);
        assert!(after.restore_reading_position(&p,"version"));assert_eq!(after.scroll,88.0);
    }
    #[test]
    fn missing_quote_is_explicit_fraction_fallback() {
        let mut r=laid("one two",&["one two"]);
        let p=Position{quote:"not present".into(),fraction:0.5,..Default::default()};
        assert!(!r.restore_reading_position(&p,"new"));assert_eq!(r.scroll,205.0);
    }
    #[test]
    fn keyboard_selection_remains_on_utf8_boundaries() {
        let mut r=laid("é語",&["é語"]);
        r.reading_move_caret(true,true);assert_eq!(r.reading_selected_text(),"é");
        r.reading_move_caret(true,true);assert_eq!(r.reading_selected_text(),"é語");
        r.reading_move_caret(false,true);assert_eq!(r.reading_selected_text(),"é");
    }
}

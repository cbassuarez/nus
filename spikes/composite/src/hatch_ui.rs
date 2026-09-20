use super::*;
use crate::hatch_work::Status;

impl App {
    pub(crate) fn hatch_access_tree(&mut self) -> accesskit::TreeUpdate {
        use accesskit::{Action, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};
        let mut nodes=Vec::new();let mut children=Vec::new();let mut focus=NodeId(1);self.hatch_state.access.clear();
        if let Some(h)=&self.hatch {
            for (n,(r,hit)) in h.hits.iter().enumerate() {
                let label=match hit {
                    Hit::Job(target)=>self.hatch_state.work.iter().find(|i|i.target==*target).map(|i|format!("{}, {}, {}, {}",i.title,i.status.label(),i.space,i.cwd)).unwrap_or_else(||"Session no longer available".into()),
                    Hit::Work=>"Ongoing work".into(),Hit::Terminal=>"Return to terminal".into(),Hit::New=>"New shell".into(),Hit::Land=>"Expand into nus".into(),Hit::Close=>"Hide Hatch".into(),Hit::Pin=>if h.pinned {"Unpin Hatch"} else {"Pin Hatch"}.into(),Hit::Previous=>"Previous session".into(),Hit::Next=>"Next session".into(),Hit::Main=>"Open nus window".into(),Hit::Quit=>"Quit nus".into(),_=>continue,
                };
                let id=NodeId(n as u64+10);let mut node=Node::new(Role::Button);node.set_label(label);node.add_action(Action::Click);
                node.set_bounds(accesskit::Rect{x0:r.x as f64,y0:r.y as f64,x1:r.right() as f64,y1:r.bottom() as f64});
                self.hatch_state.access.insert(id.0,*hit);children.push(id);nodes.push((id,node));
                if let Hit::Job(target)=hit {if self.hatch_state.overview && self.hatch_state.work.get(self.hatch_state.selected).is_some_and(|i|i.target==*target){focus=id;}}
            }
        }
        if !self.hatch_state.overview {
            if let Some(tab)=self.hatch_tab().map(|i|&self.tabs[i]) {
                for (right,pane) in std::iter::once((false,&tab.left)).chain(tab.right.as_ref().map(|p|(true,p))) {
                    let id=NodeId(1_000_000+u64::from(right));
                    let mut node=match pane {
                        Pane::Term(t)=>{let mut node=Node::new(Role::Terminal);node.set_label(format!("Terminal {}",t.title));node.set_value(crate::app::last_lines(&t.term,6).join("\n"));node},
                        Pane::Web(w)=>{let mut node=Node::new(Role::WebView);node.set_label(w.tab.shared.borrow().url.clone());node},
                        _=>Node::new(Role::Group),
                    };
                    let r=pane.rect();node.set_bounds(accesskit::Rect{x0:r.x as f64,y0:r.y as f64,x1:r.right() as f64,y1:r.bottom() as f64});
                    children.push(id);nodes.push((id,node));if right==tab.focus_right {focus=id;}
                }
            }
        }
        let mut root=Node::new(Role::Window);root.set_label("Hatch · ongoing work");root.set_children(children);nodes.push((NodeId(1),root));
        TreeUpdate{nodes,tree:Some(TreeInfo::new(NodeId(1))),tree_id:TreeId::ROOT,focus}
    }

    pub(crate) fn hatch_access_action(&mut self, request:accesskit::ActionRequest) {
        if request.action==accesskit::Action::Click {if let Some(hit)=self.hatch_state.access.get(&request.target_node.0).copied(){self.hatch_click(hit);}}
    }

    pub(crate) fn show_hatch_work(&mut self) {
        self.hatch_state.overview = true;
        self.show_hatch();
    }

    pub(crate) fn open_hatch_target(&mut self, target: WorkTarget) -> bool {
        if target.window != u64::from(self.window.id()) { return false; }
        let Some(i) = self.tabs.iter().position(|t| t.id == target.tab && t.peek.is_none()) else { return false; };
        if target.right && self.tabs[i].right.is_none() { return false; }
        for t in &mut self.tabs { t.hatch = false; }
        self.tabs[i].hatch = true;
        self.tabs[i].focus_right = target.right;
        let p = if target.right { self.tabs[i].right.as_mut().unwrap() } else { &mut self.tabs[i].left };
        if let Pane::Term(t) = p { t.waiting = false; }
        if self.active == i {
            if let Some(next) = self.tabs.iter().position(|t| !t.hatch) { self.activate(next); } else { self.open_home(); }
        }
        self.hatch_state.overview = false;
        self.layout();
        self.show_hatch();
        self.hatch_layout();
        self.apply_term_resizes(true);
        true
    }

    pub(crate) fn hatch_click(&mut self, hit: Hit) {
        match hit {
            Hit::Work => { self.hatch_state.overview = true; self.dirty = true; }
            Hit::Terminal => { self.hatch_state.overview = false; self.show_hatch(); }
            Hit::New => {
                if let Some(i) = self.hatch_tab() { self.tabs[i].hatch = false; }
                self.hatch_state.overview = false;
                self.show_hatch();
            }
            Hit::Job(target) => { let _ = self.proxy.send_event(crate::UserEvent::HatchSelect(target)); }
            Hit::Previous | Hit::Next => {
                let current = self.hatch_tab().map(|i| self.tabs[i].id);
                let rows = &self.hatch_state.work;
                if !rows.is_empty() {
                    let n = rows.iter().position(|r| r.target.window == u64::from(self.window.id()) && Some(r.target.tab) == current).unwrap_or(0);
                    let next = if hit == Hit::Next { (n+1)%rows.len() } else { (n+rows.len()-1)%rows.len() };
                    let _ = self.proxy.send_event(crate::UserEvent::HatchSelect(rows[next].target));
                }
            }
            Hit::Land => self.land(),
            Hit::Pin => self.toggle_pin(),
            Hit::Close => self.hide_hatch(),
            Hit::Main => { let _ = self.proxy.send_event(crate::UserEvent::HatchMain); }
            Hit::Quit => { let _ = self.proxy.send_event(crate::UserEvent::HatchQuit); }
            _ => {}
        }
        self.dirty = true;
    }

    pub(super) fn hatch_visible_rows(&self) -> usize {
        self.hatch.as_ref().map(|h| ((h.target.size.1 as f32 / h.window.scale_factor() as f32 - 146.0) / 64.0).floor().max(1.0) as usize).unwrap_or(5)
    }

    pub(super) fn draw_hatch(&mut self, h: &mut Hatch) {
        let scale = h.window.scale_factor() as f32;
        let px = |n:f32| (n*scale).round();
        let (w, height) = (h.target.size.0 as f32, h.target.size.1 as f32);
        let ink = self.theme.ink; let paper = self.theme.paper; let dim = self.theme.dim;
        let edge = px(if h.look == HatchLook::Card {16.0} else {2.0});
        let label = Style {font:self.f.ui,px:px(12.0),color:ink,tracking:0.0};
        let muted = Style {color:dim,..label};
        h.scene.clear(); h.scene.layer(None); h.hits.clear();
        h.scene.rect(Rect::new(0.0,0.0,w,height),paper);
        h.scene.push(nus_render::Instance::stroke(Rect::new(0.0,0.0,w,height),0.0,px(2.0),ink,None,0.0));
        let word = Style{font:self.f.wordmark,px:px(25.0),color:ink,tracking:0.0};
        self.fonts.draw(&mut h.scene,word,edge+px(14.0),px(32.0),"hatch");
        let summary = crate::hatch_work::summary(&self.hatch_state.work);
        let summary = self.fit(muted,&summary,(w-px(400.0)).max(0.0));
        if w > px(620.0) { self.fonts.draw(&mut h.scene,muted,px(112.0),px(30.0),&summary); }
        let mut right = w-edge-px(10.0);
        for (text,hit) in [("HIDE",Hit::Close),(if h.pinned {"PINNED"} else {"PIN"},Hit::Pin),(if w<px(420.0) {"↑"} else {"EXPAND"},Hit::Land)] {
            let width = self.fonts.measure(label,text)+px(18.0);
            right -= width;
            let rect = Rect::new(right,px(8.0),width-px(3.0),px(30.0));
            if rect.contains(h.pos.0,h.pos.1) {h.scene.rect(rect,crate::surface::mix(paper,ink,0.08));}
            self.fonts.draw(&mut h.scene,label,right+px(6.0),px(29.0),text);
            h.hits.push((rect,hit));
        }
        h.scene.hline(edge,px(44.0),w-edge*2.0,px(1.0),ink);
        let mut x = edge+px(8.0);
        for (text,hit,on) in [("WORK",Hit::Work,self.hatch_state.overview),("TERMINAL",Hit::Terminal,!self.hatch_state.overview),("+ SHELL",Hit::New,false),("‹",Hit::Previous,false),("›",Hit::Next,false)] {
            let width = self.fonts.measure(label,text)+px(20.0);
            let r=Rect::new(x,px(45.0),width,px(35.0));
            if on { h.scene.rect(Rect::new(x,px(77.0),width,px(3.0)),self.surface.signal); }
            self.fonts.draw(&mut h.scene,if on {label} else {muted},x+px(8.0),px(67.0),text);
            h.hits.push((r,hit)); x+=width;
        }
        h.scene.hline(edge,px(81.0),w-edge*2.0,px(1.0),crate::surface::mix(paper,ink,0.2));
        if self.hatch_state.overview {
            let row_h=px(64.0);
            let available=((height-px(146.0))/row_h).floor().max(1.0) as usize;
            self.hatch_state.scroll=self.hatch_state.scroll.min(self.hatch_state.work.len().saturating_sub(available));
            if self.hatch_state.work.is_empty() {
                self.fonts.draw(&mut h.scene,Style{font:self.f.wordmark,px:px(26.0),..label},edge+px(22.0),px(137.0),"Your work will appear here.");
                let text=self.fit(muted,"Open a shell to get started. Running commands and attention stay one shortcut away.",w-edge*2.0-px(44.0));
                self.fonts.draw(&mut h.scene,muted,edge+px(22.0),px(169.0),&text);
            }
            for (n,item) in self.hatch_state.work.iter().enumerate().skip(self.hatch_state.scroll).take(available) {
                let y=px(88.0)+(n-self.hatch_state.scroll) as f32*row_h;
                let r=Rect::new(edge+px(8.0),y,w-edge*2.0-px(16.0),row_h);
                if n==self.hatch_state.selected || r.contains(h.pos.0,h.pos.1) {h.scene.rect(r,crate::surface::mix(paper,ink,0.06));}
                let status=match item.progress {Some(p)=>format!("{} {p}%",item.status.label()),None=>item.status.label().into()};
                let status_w=self.fonts.measure(muted,&status);
                let title_w=(r.w-status_w-px(50.0)).max(px(40.0));
                let title=self.fit(label,&item.title,title_w);
                let color=if item.status==Status::Failed { nus_render::theme::signal::RED } else { self.surface.signal };
                h.scene.rect(Rect::new(r.x+px(10.0),y+px(18.0),px(6.0),px(6.0)),color);
                self.fonts.draw(&mut h.scene,label,r.x+px(26.0),y+px(25.0),&title);
                self.fonts.draw(&mut h.scene,muted,r.right()-status_w-px(8.0),y+px(25.0),&status);
                let suffix=match (item.status,item.exit) { (Status::Failed,Some(code))=>format!(" · exit {code}"),(Status::Finished,None)=>" · exit status unavailable".into(),_=>String::new() };
                let detail=self.fit(muted,&format!("{} · {}{}",item.space,item.cwd,suffix),r.w-px(34.0));
                self.fonts.draw(&mut h.scene,muted,r.x+px(26.0),y+px(47.0),&detail);
                h.scene.hline(r.x,y+row_h-px(1.0),r.w,px(1.0),crate::surface::mix(paper,ink,0.15));
                h.hits.push((r,Hit::Job(item.target)));
            }
        } else if let Some(i)=self.hatch_tab() {
            let saved=(self.active,self.mouse,self.scale);
            self.active=i;self.mouse=h.pos;self.scale=scale;
            let name=self.tab_label(i);let look=self.tabs[i].look.clone();
            let mut tabs=std::mem::take(&mut self.tabs);
            let tab=&mut tabs[i];let split=tab.right.is_some();
            self.draw_pane(&mut h.scene,&mut tab.left,&name,h.focused&&!tab.focus_right,&look,split);
            if let Some(p)=tab.right.as_mut(){ self.draw_pane(&mut h.scene,p,&name,h.focused&&tab.focus_right,&look,true); }
            self.tabs=tabs;self.active=saved.0;self.mouse=saved.1;self.scale=saved.2;
        }
        h.scene.layer(None);
        let foot=height-px(32.0);
        h.scene.hline(edge,foot,w-edge*2.0,px(1.0),crate::surface::mix(paper,ink,0.2));
        let hint=if self.hatch_state.overview { "↑ ↓ select · Enter opens the existing session".into() } else { self.hatch_tab().map(|i|self.tabs[i].title()).unwrap_or_default() };
        let hint=self.fit(muted,&hint,(w-px(180.0)).max(0.0));
        self.fonts.draw(&mut h.scene,muted,edge+px(14.0),foot+px(20.0),&hint);
        for (text,hit,offset) in [("NUS",Hit::Main,100.0),("QUIT",Hit::Quit,48.0)] {
            let x=w-edge-px(offset);self.fonts.draw(&mut h.scene,muted,x,foot+px(20.0),text);
            h.hits.push((Rect::new(x-px(5.0),foot,px(44.0),px(26.0)),hit));
        }
        let lip=Rect::new(0.0,height-px(6.0),w,px(6.0));h.scene.rect(lip,self.surface.signal);
        h.hits.push((Rect::new(0.0,height-px(10.0),w,px(10.0)),Hit::Lip));
        if h.look==HatchLook::Card { h.hits.push((Rect::new(0.0,0.0,right,px(43.0)),Hit::Frame)); }
        h.scene.finish();
        if let Some(notch)=h.notch {
            let reveal=h.slide.value();
            let width=notch.width as f32+(w-notch.width as f32)*reveal;
            h.scene.clip_all(Rect::new((w-width)/2.0,0.0,width,height*reveal));
        }
        for (x,y,w,h,data) in self.fonts.uploads.drain(..) {self.gpu.upload_glyph(x,y,w,h,&data);}
        self.gpu.render(&mut h.target,&h.scene,if h.notch.is_some(){[0.0;4]}else{paper});
    }

    pub(crate) fn attach_hatch_overlay(&mut self, window:Arc<Window>, badge:bool) {
        crate::hatch_native::configure(&window);
        crate::macos::prepare_window(&window);
        let Ok(target)=self.gpu.target(window.clone()) else {return;};
        let overlay=Overlay{window,target,scene:Scene::new(),visible:false,text:String::new(),position:None};
        if badge {self.hatch_state.badge=Some(overlay);} else {self.hatch_state.shade=Some(overlay);self.hatch_shade();}
    }

    pub(crate) fn hatch_shade(&mut self) {
        let show=self.behavior.hatch_dim && self.behavior.hatch_look==HatchLook::Card && self.hatch.as_ref().is_some_and(|h|h.visible&&!h.hiding);
        let (_,_,(x,y,w,h,_))=self.hatch_geometry();
        let Some(shade)=&mut self.hatch_state.shade else {return;};
        if show {
            shade.window.set_outer_position(winit::dpi::PhysicalPosition::new(x,y));
            let _=shade.window.request_inner_size(winit::dpi::PhysicalSize::new(w,h));
            shade.target.resize(&self.gpu.device,w,h);
            shade.scene.clear();shade.scene.finish();
            self.gpu.render(&mut shade.target,&shade.scene,[0.0,0.0,0.0,0.35]);
        }
        shade.visible=show;
        if show {crate::hatch_native::show_passive(&shade.window);} else {shade.window.set_visible(false);}
    }

    pub(crate) fn hatch_badge_frame(&mut self) {
        if self.hatch_state.completion.as_ref().is_some_and(|(_,at)|crate::clock::since(at).as_secs()>=6) || !self.behavior.hatch_notify {self.hatch_state.completion=None;}
        let notice=self.hatch_state.completion.is_some();
        let summary=self.hatch_state.completion.as_ref().map(|(item,_)|format!("{} · {}",item.title,item.status.label().to_lowercase())).unwrap_or_else(||crate::hatch_work::summary(&self.hatch_state.work));
        let open=self.hatch_state.island_open.is_some();
        let mon=self.hatch_state.island_open.unwrap_or_else(||self.hatch_geometry().2);
        let (x,y,w,_,scale)=mon;
        let top=crate::hatch_native::top_area(x,y,scale);
        let attached=open && top.notch.is_some();
        let show=attached || !self.hatch_state.badge_suppressed && (self.behavior.hatch_status && (summary!="Hatch · ready" || self.hatch_state.main_hidden) || notice)
            && self.hatch.as_ref().is_none_or(|h|!h.visible);
        let px=|v:f32|(v*scale).round();
        let (width,height,pos)=if let Some(n)=top.notch {
            let width=(n.width+px(if attached {12.0}else{116.0}) as u32).min(w);
            let height=n.height+px(if attached {2.0}else if notice {30.0}else{8.0}) as u32;
            (width,height,(x+n.left+(n.width as i32-width as i32)/2,y))
        } else {
            let width=px(330.0).min(w as f32).max(1.0) as u32;
            (width,px(30.0).max(1.0) as u32,(x+(w as i32-width as i32)/2,y+top.menu))
        };
        let background=if top.notch.is_some(){[0.0,0.0,0.0,1.0]}else{self.theme.ink};
        let foreground=if top.notch.is_some(){[0.95,0.95,0.95,1.0]}else{self.theme.paper};
        let label=Style{font:self.f.ui,px:px(12.0),color:foreground,tracking:0.0};
        let summary=self.fit(label,&summary,width as f32-px(if notice{48.0}else{16.0}));
        let key=format!("{summary}:{attached}:{notice}:{background:?}:{:?}",self.surface.signal);
        let notice_icon=if show&&notice{self.desktop_icon()}else{None};
        let running=self.hatch_state.work.iter().filter(|i|i.status==Status::Running).count();
        let attention=self.hatch_state.work.iter().filter(|i|i.status.attention()&&i.unread).count();
        let finished=self.hatch_state.work.iter().filter(|i|i.status==Status::Finished&&i.unread).count();
        let Some(badge)=&mut self.hatch_state.badge else {return;};
        if !show {if badge.visible {badge.window.set_visible(false);badge.visible=false;}return;}
        crate::hatch_native::island_level(&badge.window,top.notch.is_some());
        if badge.position!=Some(pos) {badge.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0,pos.1));badge.position=Some(pos);}
        let changed=badge.text!=key || badge.target.size!=(width,height) || !badge.visible;
        if !changed {return;}
        badge.text=key;
        let _=badge.window.request_inner_size(winit::dpi::PhysicalSize::new(width,height));
        badge.target.resize(&self.gpu.device,width,height);
        // Resize can move the top edge on AppKit: pin it again to screen.frame.
        badge.window.set_outer_position(winit::dpi::PhysicalPosition::new(pos.0,pos.1));
        badge.scene.clear();badge.scene.layer(None);
        if let Some(notch)=top.notch {
            badge.scene.push(nus_render::Instance::rounded(Rect::new(0.0,-px(14.0),width as f32,height as f32+px(14.0)),px(14.0),background));
            if !attached {
                let left=if running>0 {format!("{running}")}else{"nus".into()};
                let right=if attention>0 {format!("! {attention}")}else if finished>0 {format!("✓ {finished}")}else{"↓".into()};
                let wing=(width-notch.width) as f32/2.0;
                let baseline=notch.height as f32/2.0+px(4.0);
                self.fonts.draw(&mut badge.scene,label,((wing-self.fonts.measure(label,&left))/2.0).max(px(4.0)),baseline,&left);
                self.fonts.draw(&mut badge.scene,label,width as f32-wing+(wing-self.fonts.measure(label,&right))/2.0,baseline,&right);
                if notice {
                    let tw=self.fonts.measure(label,&summary);let x=((width as f32-tw-px(28.0))/2.0).max(px(6.0));
                    if let Some(icon)=notice_icon.clone(){badge.scene.texture(Rect::new(x,notch.height as f32+px(4.0),px(22.0),px(22.0)),icon,None);badge.scene.layer(None);}
                    self.fonts.draw(&mut badge.scene,label,x+px(28.0),notch.height as f32+px(20.0),&summary);
                }
            }
        } else {
            badge.scene.rect(Rect::new(0.0,0.0,width as f32,height as f32),background);
            let tw=self.fonts.measure(label,&summary);
            let x=if notice{((width as f32-tw-px(28.0))/2.0).max(px(6.0))}else{((width as f32-tw)/2.0).max(px(4.0))};
            if let Some(icon)=notice_icon {badge.scene.texture(Rect::new(x,px(4.0),px(22.0),px(22.0)),icon,None);badge.scene.layer(None);}
            self.fonts.draw(&mut badge.scene,label,x+if notice{px(28.0)}else{0.0},px(20.0),&summary);
        }
        badge.scene.finish();
        for(x,y,w,h,data)in self.fonts.uploads.drain(..){self.gpu.upload_glyph(x,y,w,h,&data);}
        self.gpu.render(&mut badge.target,&badge.scene,[0.0;4]);
        crate::hatch_native::show_passive(&badge.window);badge.visible=true;
    }
}

//! One collection, with the same identity in Home, the palette and Settings.
use crate::app::{Action, App, PaletteRow};
use nus_render::{Rect, Scene, Style};

fn can_insert(command:&str)->bool{!command.chars().any(char::is_control)}

impl App {
    pub(crate) fn saved_use(&mut self, index: usize, run: bool) {
        let Some(value) = self.behavior.prompt.saved.get(index).cloned() else { return };
        let Some(row) = self.prompt_action(&value) else { return };
        if let Action::PromptShell(command) = row.action {
            if run { self.open_prompt_shell(&command); }
            else {
                if !can_insert(&command) { self.notice(nus_render::text::icons::TERMINAL,"Edit To One Line","this command has line breaks or control characters · edit it, or use Run");return; }
                match self.new_term_pane(false, self.behavior.default_profile) {
                    Ok(mut pane) => {
                        pane.type_at_prompt = Some(command);
                        let tab = self.make_tab(crate::app::Pane::Term(pane), None);
                        self.tabs.push(tab); self.activate(self.tabs.len()-1); self.layout();
                    }
                    Err(error) => self.notice_problem("Could Not Open Terminal", error.to_string()),
                }
            }
        } else { self.run(row.action); }
    }

    pub(crate) fn saved_edit(&mut self, index: usize, name: bool, value: String) {
        let value = value.trim();
        let c = &mut self.behavior.prompt;
        let Some(previous) = c.saved.get(index).cloned() else { return };
        if name {
            if value.is_empty() { c.saved_names.remove(&previous); }
            else { c.saved_names.insert(previous, value.into()); }
        } else {
            if value.is_empty() { self.notice(nus_render::text::icons::PENCIL, "Enter A Command", "a command, URL or assistant route"); return; }
            if c.saved.iter().enumerate().any(|(i,v)|i!=index && v==value) { self.notice(nus_render::text::icons::PENCIL, "Already Saved", ""); return; }
            c.saved[index] = value.into();
            if let Some(label) = c.saved_names.remove(&previous) { c.saved_names.insert(value.into(), label); }
        }
        self.save_prefs(); self.dirty = true;
    }

    pub(crate) fn saved_detail(&self, row: &PaletteRow) -> Option<(String, &'static str)> {
        match row.action {
            Action::SavedUse(i, run) => {
                let value = self.behavior.prompt.saved.get(i)?;
                let verb = match self.prompt_action(value).map(|r|r.action) {
                    Some(Action::PromptShell(_)) => if run { "RUN" } else { "INSERT" },
                    Some(Action::AssistantDraft(..)) => "REVIEW",
                    _ => "OPEN",
                };
                Some((value.clone(), verb))
            }
            _ if row.num == "saved-library" => Some((format!("{} commands, links and prompts",self.behavior.prompt.saved.len()), "MANAGE")),
            _ => None,
        }
    }

    /// Bookmark edge and action label survive even in compact mode.
    pub(crate) fn draw_saved_row(&mut self, scene: &mut Scene, r: Rect, row: &PaletteRow, selected: bool, preview: bool) {
        let Some((detail, verb)) = self.saved_detail(row) else { return };
        let t = self.theme.clone();
        let fg = if selected { self.on_fill(t.ink) } else { t.ink };
        let signal = if selected { fg } else { self.surface.signal };
        if !selected { scene.rect(r, crate::app::fade(self.surface.signal, 0.045)); }
        scene.vline(r.x, r.y+self.px(6.0),r.h-self.px(12.0),self.px(2.0),signal);
        // Folded bookmark, with the same proportions at every density.
        let x=r.x+self.px(15.0);let y=r.y+(r.h-self.px(15.0))/2.0;
        scene.poly(&[[x,y],[x+self.px(10.0),y],[x+self.px(10.0),y+self.px(15.0)],[x+self.px(5.0),y+self.px(11.0)],[x,y+self.px(15.0)]],signal);
        let tx=r.x+self.px(39.0);
        let badge=Style {color:crate::app::fade(fg,0.7),px:self.px(9.0),..self.label()};
        let vw=self.fonts.measure(badge,verb);
        self.fonts.draw(scene,badge,r.right()-self.px(14.0)-vw,r.y+r.h/2.0+self.px(3.0),verb);
        let available=(r.w-self.px(67.0)-vw).max(0.0);
        let title=Style{color:fg,..self.ui_strong()};
        let base=r.y+if preview {self.px(20.0)} else {r.h/2.0+self.px(4.0)};
        self.fonts.draw(scene,title,tx,base,&self.fit(title,&row.text,available));
        if preview {
            let code=Style{font:self.f.term,px:self.px(11.0),color:crate::app::fade(fg,0.67),tracking:0.0};
            self.fonts.draw(scene,code,tx,base+self.px(18.0),&self.fit(code,&detail,available));
        }
    }

    fn saved_controls(&self,index:usize)->Vec<(&'static str,crate::settings::workspace::Hit)> {
        use crate::settings::workspace::Hit;
        let mut controls=match self.behavior.prompt.saved.get(index).and_then(|s|self.prompt_action(s)).map(|r|r.action){
            Some(Action::PromptShell(_))=>vec![("Insert",Hit::SavedUse(index,false)),("Run",Hit::SavedUse(index,true))],
            Some(Action::AssistantDraft(..))=>vec![("Review",Hit::SavedUse(index,false))],
            _=>vec![("Open",Hit::SavedUse(index,false))],
        };
        controls.extend([("Name",Hit::SavedEdit(index,true)),("Edit",Hit::SavedEdit(index,false)),("Copy",Hit::SavedCopy(index)),("↑",Hit::SavedMove(index,-1)),("↓",Hit::SavedMove(index,1)),("Remove",Hit::Unpin(index))]);
        controls
    }
    pub(crate) fn saved_card_height(&self,index:usize,width:f32)->f32{
        let style=Style{px:self.px(11.0),tracking:0.0,..self.ui()};let mut x=0.0;let mut lines=1;
        for (name,_)in self.saved_controls(index){let w=self.fonts.measure(style,name)+self.px(19.0);if x>0.0&&x+w>width-self.px(24.0){lines+=1;x=0.0;}x+=w;}
        self.px(70.0+lines as f32*29.0)
    }
    pub(crate) fn draw_saved_card(&mut self,scene:&mut Scene,r:Rect,index:usize){
        let Some(value)=self.behavior.prompt.saved.get(index)else{return};
        let row=PaletteRow{num:"saved".into(),text:self.behavior.prompt.saved_names.get(value).unwrap_or(value).clone(),action:Action::SavedUse(index,self.behavior.prompt.saved_run)};
        self.draw_saved_row(scene,Rect::new(r.x,r.y,r.w,self.px(56.0)),&row,false,true);
        let style=Style{px:self.px(11.0),tracking:0.0,..self.ui()};let mut x=r.x+self.px(12.0);let mut y=r.y+self.px(58.0);
        for (name,hit)in self.saved_controls(index){let w=self.fonts.measure(style,name)+self.px(19.0);if x>r.x+self.px(12.0)&&x+w>r.right()-self.px(12.0){x=r.x+self.px(12.0);y+=self.px(29.0);}
            let rect=Rect::new(x,y,w,self.px(27.0));let hot=rect.contains(self.mouse.0,self.mouse.1);
            if hot{scene.rect(rect,crate::app::fade(self.surface.signal,0.1));}
            self.fonts.draw(scene,Style{color:if hot{self.surface.signal}else{self.theme.dim},..style},x+self.px(8.0),y+self.px(18.0),name);
            self.settings_hits.push((rect,crate::settings::Hit::Workspace(hit)));x+=w;
        }
    }
}

#[cfg(test)]mod tests{
    #[test]fn insertion_cannot_submit_or_send_terminal_controls(){
        assert!(super::can_insert("cargo test --workspace"));
        for text in ["echo one\necho two","echo one\r","echo\targ","\u{1b}[200~whoami"]{assert!(!super::can_insert(text));}
    }
    #[test]fn old_saved_strings_load_without_loss(){
        let config:crate::prompt::Config=serde_json::from_str(r#"{"saved":["> cargo test","https://docs.rs"]}"#).unwrap();
        assert_eq!(config.saved.len(),2);assert!(!config.saved_run);assert!(config.saved_preview);assert!(config.saved_names.is_empty());
    }
}

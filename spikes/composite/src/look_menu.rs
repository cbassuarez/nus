use crate::app::{App, SideHit, Quick, IconMotion, hover_key, fade};
use nus_render::{Rect, Scene, Style};
use nus_render::text::icons;

impl App {
    pub(crate) fn footer_theme_names(&self) -> Vec<String> {
        self.behavior.footer_themes.clone().unwrap_or_else(|| crate::themes::all().into_iter().filter(|t|!t.port).take(9).map(|t|t.name).collect())
    }

    pub(crate) fn draw_look_menu(&mut self, scene: &mut Scene, sb: Rect) {
        let a=self.look_anim.value();
        if a<=0.001 {self.look_rect=None; return;}
        let Some(chip)=self.side_hits.iter().find(|(_,h)|*h==SideHit::Look).map(|(r,_)|*r) else {return;};
        let all=crate::themes::all();
        let names=self.footer_theme_names();
        let themes:Vec<_>=names.iter().filter_map(|name|all.iter().enumerate().find(|(_,t)|&t.name==name)).collect();
        let pad=self.px(8.0); let gap=self.px(6.0);
        let w=(sb.w-self.px(16.0)).min(self.px(320.0)).max(self.px(232.0)).min(self.target.size.0 as f32-self.px(16.0));
        let tile_w=(w-2.0*pad-2.0*gap)/3.0;
        let row_h=self.px(58.0);
        let grid_h=(row_h*3.0-gap).min((chip.y-self.strip_rect().bottom()-self.px(90.0)).max(self.px(58.0)));
        let h=grid_h+self.px(70.0);
        let x=if sb.w>=w+2.0*pad {(chip.x-pad).clamp(sb.x+pad,sb.right()-w-pad)}else if self.sidebar_right(){sb.right()-w-pad}else{sb.x+pad};
        let x=x.clamp(pad,(self.target.size.0 as f32-w-pad).max(pad));
        let r=Rect::new(x,(chip.y-h-self.px(6.0)).max(self.strip_rect().bottom()),w,h);
        self.look_rect=self.look_menu.then_some(Rect::new(r.x,r.y,r.w,r.h+self.px(8.0)));
        let outer=scene.clip(); scene.layer(None);
        let ink=self.theme.ink;let paper=self.paper();
        scene.rect(Rect::new(r.x+self.px(3.0),r.y+self.px(3.0),r.w,r.h),fade(ink,a));
        scene.rect(r,paper);scene.outline(r,self.px(1.5),ink);
        let grid=Rect::new(r.x+pad,r.y+pad,w-2.0*pad,grid_h);
        self.look_scroll_max=(themes.len().max(9).div_ceil(3) as f32*row_h-gap-grid_h).max(0.0);
        self.look_scroll=self.look_scroll.clamp(0.0,self.look_scroll_max);
        scene.layer(Some(grid));
        for slot in 0..themes.len().max(9) {
            let tile=Rect::new(grid.x+(slot%3) as f32*(tile_w+gap),grid.y+(slot/3) as f32*row_h-self.look_scroll,tile_w,row_h-gap);
            if tile.bottom()<=grid.y || tile.y>=grid.bottom() {continue;}
            let swatch=Rect::new(tile.x,tile.y,tile.w,self.px(32.0));
            if let Some((index,theme))=themes.get(slot) {
                let ramp=theme.surface.ramp(theme.ink.ink);
                scene.push(nus_render::Instance::rounded_stops(swatch,0.0,&ramp,theme.surface.angle,0.0,false));
                scene.outline(swatch,self.px(1.0),ink);
                let style=Style{px:self.px(9.0),..self.label()};
                let name=self.fit(style,&theme.name,tile.w-self.px(3.0));
                self.fonts.draw(scene,style,tile.x+self.px(1.0),tile.y+self.px(46.0),&name);
                if theme.name==self.preset_name {
                    let d=self.px(15.0);scene.rect(Rect::new(swatch.right()-d,swatch.y,d,d),ink);
                    self.fonts.draw_icon(scene,icons::CHECK,self.px(11.0),swatch.right()-d+self.px(2.0),swatch.y+self.px(2.0),paper);
                }
                if self.look_menu {self.side_hits.push((tile.intersect(&grid),SideHit::LookPreset(*index)));}
                if tile.intersect(&grid).contains(self.mouse.0,self.mouse.1) {self.foot_tip(hover_key("footer-theme",*index),tile.intersect(&grid),theme.name.clone());}
            } else {
                scene.outline(swatch,self.px(1.0),self.theme.dim);
                self.fonts.draw_icon(scene,icons::PLUS,self.px(12.0),swatch.x+(swatch.w-self.px(12.0))/2.0,swatch.y+self.px(10.0),self.theme.dim);
                if self.look_menu {self.side_hits.push((tile.intersect(&grid),SideHit::LookStudio));}
            }
        }
        scene.layer(Some(r));
        if self.look_scroll_max>0.0 {
            let thumb=grid_h*grid_h/(grid_h+self.look_scroll_max);
            scene.rect(Rect::new(r.right()-self.px(4.0),grid.y+(grid_h-thumb)*self.look_scroll/self.look_scroll_max,self.px(2.0),thumb),ink);
        }
        let qy=grid.bottom()+self.px(8.0);let cell=(w-2.0*pad)/5.0;
        for (i,(icon,q)) in [(icons::SHUFFLE,Quick::Shuffle),(icons::RELOAD,Quick::Rotate),(if self.theme.mode==nus_render::Mode::Ink {icons::SUN}else{icons::MOON},Quick::Flip),(icons::BRUSH,Quick::Texture),(icons::UNDO,Quick::Reset)].into_iter().enumerate() {
            let hit=Rect::new(r.x+pad+i as f32*cell,qy,cell,self.px(22.0));
            self.icon_button(scene,icon,self.px(14.0),hit.x+(cell-self.px(14.0))/2.0,hit.y+self.px(3.0),ink,hit,hover_key("quick",i),IconMotion::Still);
            if self.look_menu {self.side_hits.push((hit,SideHit::LookQuick(q)));}
        }
        let config=Rect::new(r.x+pad,qy+self.px(25.0),w-2.0*pad,self.px(22.0));
        let style=Style{px:self.px(10.0),..self.label()};
        let text=self.fit(style,"Choose themes in Settings",config.w);
        self.fonts.draw(scene,style,config.x,config.y+self.px(13.0),&text);
        if self.look_menu {self.side_hits.push((config,SideHit::LookStudio));}
        scene.layer(outer);
    }
}

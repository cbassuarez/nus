//! Accessibility extension for the existing Home-based reading surface.
//! Base chrome keeps its existing tree/dispatcher; library IDs are stable and
//! allocated above the base tree's transient range, per window.
use accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeUpdate};
use super::{Hit, App, Block};

fn rect(r:nus_render::Rect)->accesskit::Rect {
    accesskit::Rect{x0:r.x as f64,y0:r.y as f64,x1:r.right() as f64,y1:r.bottom() as f64}
}
impl App {
    fn library_node_id(&mut self,key:String)->NodeId {
        let next=(1u64<<48)+self.library.access_ids.len() as u64+1;
        NodeId(*self.library.access_ids.entry(key).or_insert(next))
    }
    fn library_access_visible(&self)->bool {
        self.library_home().is_some() && self.palette.is_none() && self.start.is_none()
            && !self.me_card.open && !self.dl_menu && self.timeline.is_none() && self.page_menu.is_none()
    }
    pub(crate) fn library_access_tree(&mut self)->TreeUpdate {
        let mut tree=self.access_tree();
        self.library.access_map.clear();
        if !self.library_access_visible(){return tree;}
        let h=self.library_home().unwrap();
        let hits=h.library_ui.hits.clone();let focus=h.library_ui.focus.clone();let area=h.rect;
        let query=h.input.clone();let filter=h.library_ui.filter;let confirming=h.library_ui.confirm;let selected=h.library_ui.selected.clone();
        let reading=h.reading.as_ref().map(|r|(r.id.clone(),r.reader.article.clone(),r.reader.saved.find.clone()));
        let group=self.library_node_id("reading-library".into());
        let mut children=Vec::new();let mut rows=Vec::new();
        for (bounds,hit) in hits {
            // A wrapped reference may occupy several lines: one semantic node.
            let key=format!("control:{hit:?}");let id=self.library_node_id(key);
            if children.contains(&id){continue;}
            let role=match &hit {Hit::Search=>Role::TextInput,Hit::Find if reading.as_ref().is_some_and(|(_,_,q)|q.is_some())=>Role::TextInput,Hit::Row(_)=>Role::ListBoxOption,_=>Role::Button};
            let mut n=Node::new(role);n.set_label(self.library_label(&hit));n.set_bounds(rect(bounds));n.add_action(Action::Click);n.add_action(Action::Focus);
            if role==Role::TextInput {
                n.add_action(Action::SetValue);
                n.set_value(if hit==Hit::Search{query.clone()}else{reading.as_ref().and_then(|(_,_,q)|q.clone()).unwrap_or_default()});
            }
            if let Hit::Row(key)=&hit{n.set_selected(selected.as_ref()==Some(key));}
            if let Hit::Filter(value)=&hit{n.set_toggled(if *value==filter{accesskit::Toggled::True}else{accesskit::Toggled::False});}
            if focus.as_ref()==Some(&hit){tree.focus=id;}
            self.library.access_map.insert(id.0,hit);
            tree.nodes.push((id,n));if role==Role::ListBoxOption{rows.push(id);}else{children.push(id);}
        }
        if !rows.is_empty(){let id=self.library_node_id("library-list".into());let mut list=Node::new(Role::ListBox);list.set_label("Saved articles");list.set_children(rows);tree.nodes.push((id,list));children.push(id);}
        if let Some((key,article,_))=reading.filter(|_|!confirming) {
            let doc_id=self.library_node_id(format!("article:{key}"));
            let mut doc=Node::new(Role::Document);doc.set_label(article.title);doc.set_bounds(rect(area));
            let mut blocks=Vec::new();
            for (i,b) in article.blocks.iter().enumerate(){
                let (role,text)=match b {
                    Block::Heading(_,s)=>(Role::Heading,s.clone()),
                    Block::Image(alt,_)=>(Role::Image,if alt.is_empty(){"Article image".into()}else{alt.clone()}),
                    Block::Link(label,url)=>(Role::Label,format!("{label} · {url}")),
                    Block::Para(s)|Block::Pre(s)|Block::Item(s)|Block::Quote(s)|Block::Caption(s)=>(Role::Label,s.clone()),
                };
                let id=self.library_node_id(format!("article:{key}:{i}"));let mut node=Node::new(role);node.set_label(text);tree.nodes.push((id,node));blocks.push(id);
            }
            doc.set_children(blocks);tree.nodes.push((doc_id,doc));children.push(doc_id);
        }
        let mut node=Node::new(if confirming{Role::Dialog}else{Role::Group});if confirming{node.set_modal();}node.set_label("Reading library");node.set_bounds(rect(area));node.set_children(children);
        tree.nodes.push((group,node));
        if let Some((_,root))=tree.nodes.iter_mut().find(|(id,_)|id.0==1){let mut kids=root.children().to_vec();kids.push(group);root.set_children(kids);}
        tree
    }
    pub(crate) fn library_access_action(&mut self,req:ActionRequest) {
        if !self.library_access_visible(){self.access_action(req);return;}
        let Some(hit)=self.library.access_map.get(&req.target_node.0).cloned() else{self.access_action(req);return;};
        match req.action {
            Action::Click=>self.library_action(hit),
            Action::Focus=>if let Some(h)=self.library_home_mut(){if let Hit::Row(id)=&hit{h.library_ui.selected=Some(id.clone());h.library_ui.reveal=true;}h.library_ui.focus=Some(hit);},
            Action::SetValue=>if let Some(accesskit::ActionData::Value(value))=req.data{
                let text:String=value.chars().take(2000).collect();
                if let Some(h)=self.library_home_mut(){match hit{
                    Hit::Search=>{h.input=text;h.library_ui.selected=None;h.library_scroll=0.0;h.sel=0;},
                    Hit::Find=>if let Some(r)=h.reading.as_mut(){r.reader.saved.find=Some(text);r.reader.saved.found=None;r.reader.reading_find(true);},
                    _=>{},
                }}
            },
            _=>{},
        }
        self.dirty=true;
    }
}

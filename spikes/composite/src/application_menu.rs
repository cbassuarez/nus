//! One command catalog for the native application menu and F10 menu.
//! Linux keeps an in-window menu without introducing a GTK runtime.
use crate::app::{Action, App, KeyIn, PaletteMode, Pane};
use winit::keyboard::{Key, KeyCode, ModifiersState, PhysicalKey};
use cef::{ImplBrowser, ImplFrame};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command { Settings, Quit, NewTab, NewShell, NewWindow, NewPrivateWindow, ReportBug, RequestFeature, CloseTab, CloseWindow, Reopen, Undo, Redo, Cut, Copy, Paste, SelectAll, Find, ZoomIn, ZoomOut, ZoomReset, Home, Downloads, Palette, Sidebar, Reload, Fullscreen, Split, History, Devtools, Hatch, Minimize, NextTab, PreviousTab, Welcome }

pub struct Item { pub group: &'static str, pub label: &'static str, pub command: Command, pub key: Option<&'static str> }
macro_rules! item { ($group:literal,$label:literal,$command:ident,$key:expr) => { Item{group:$group,label:$label,command:Command::$command,key:$key} }; }
pub const ITEMS: &[Item] = &[
    item!("nus","Settings…",Settings,Some("CmdOrCtrl+Comma")),
    item!("nus","Quit nus",Quit,Some("CmdOrCtrl+KeyQ")),
    item!("File","New Tab",NewTab,Some("CmdOrCtrl+KeyT")),
    item!("File","New Shell",NewShell,None),
    item!("File","New Window",NewWindow,Some("CmdOrCtrl+KeyN")),
    item!("File","New Incognito Window",NewPrivateWindow,Some("CmdOrCtrl+Shift+KeyN")),
    item!("File","Close Tab",CloseTab,Some("CmdOrCtrl+KeyW")),
    item!("File","Close Window",CloseWindow,Some("CmdOrCtrl+Shift+KeyW")),
    item!("File","Reopen Closed Tab",Reopen,Some("CmdOrCtrl+Shift+KeyT")),
    item!("Edit","Undo",Undo,Some("CmdOrCtrl+KeyZ")),
    item!("Edit","Redo",Redo,Some("CmdOrCtrl+Shift+KeyZ")),
    item!("Edit","Cut",Cut,Some("CmdOrCtrl+KeyX")),
    item!("Edit","Copy",Copy,Some("CmdOrCtrl+KeyC")),
    item!("Edit","Paste",Paste,Some("CmdOrCtrl+KeyV")),
    item!("Edit","Select All",SelectAll,Some("CmdOrCtrl+KeyA")),
    item!("Edit","Find…",Find,Some("CmdOrCtrl+KeyF")),
    item!("View","Zoom In",ZoomIn,Some("CmdOrCtrl+Equal")),
    item!("View","Zoom Out",ZoomOut,Some("CmdOrCtrl+Minus")),
    item!("View","Reset Zoom",ZoomReset,Some("CmdOrCtrl+Digit0")),
    item!("View","Home",Home,None),
    item!("View","Downloads",Downloads,None),
    item!("View","Command Palette…",Palette,Some("CmdOrCtrl+KeyK")),
    item!("View","Show / Hide Sidebar",Sidebar,None),
    item!("View","Reload Page",Reload,Some("CmdOrCtrl+KeyR")),
    item!("View","Toggle Full Screen",Fullscreen,None),
    item!("View","Split Pane",Split,None),
    item!("View","Shell History",History,None),
    item!("View","Developer Tools",Devtools,None),
    item!("Window","Show / Hide Hatch",Hatch,None),
    item!("Window","Minimize",Minimize,Some("CmdOrCtrl+KeyM")),
    item!("Window","Next Tab",NextTab,None),
    item!("Window","Previous Tab",PreviousTab,None),
    item!("Help","Welcome to nus",Welcome,None),
    item!("Help","Report a Bug…",ReportBug,None),
    item!("Help","Request a Feature…",RequestFeature,None),
];

pub fn from_id(id: &str) -> Option<Command> { id.strip_prefix("nus-app-")?.parse::<usize>().ok().and_then(|i|ITEMS.get(i)).map(|i|i.command) }

#[cfg(any(windows,target_os="macos"))]
pub struct NativeMenu { menu: tray_icon::menu::Menu, items: Vec<tray_icon::menu::MenuItem> }
#[cfg(not(any(windows,target_os="macos")))]
pub struct NativeMenu;

impl NativeMenu {
    pub fn new(proxy: winit::event_loop::EventLoopProxy<crate::UserEvent>) -> Self {
        #[cfg(any(windows,target_os="macos"))] {
            use tray_icon::menu::{Menu, Submenu, MenuItem, MenuEvent, PredefinedMenuItem};
            // muda has one process-wide handler shared with the status icon.
            MenuEvent::set_event_handler(Some(move |event:MenuEvent| {
                let action = if let Some(command)=from_id(&event.id.0) { crate::UserEvent::ApplicationCommand(command) } else {
                    match event.id.0.as_str() { "nus-drawer"=>crate::UserEvent::MenuDrawer(None), "hatch-show"=>crate::UserEvent::Hatch, "hatch-main"=>crate::UserEvent::HatchMain, "hatch-quit"=>crate::UserEvent::HatchQuit, _=>return }
                };
                let _=proxy.send_event(action);
            }));
            let menu=Menu::new(); let mut items=Vec::new();
            #[cfg(target_os="macos")] menu.init_for_nsapp();
            for group in ["nus","File","Edit","View","Window","Help"] {
                let submenu=Submenu::new(group,true);
                #[cfg(target_os="macos")]
                if group=="nus" { let _=submenu.append(&PredefinedMenuItem::about(Some("About nus"),None)); let _=submenu.append(&PredefinedMenuItem::separator()); }
                for (i,item) in ITEMS.iter().enumerate().filter(|(_,item)|item.group==group) {
                    // Shell shortcuts on Windows/Linux use Ctrl+Shift; native
                    // menus must not take Ctrl+C away from terminal programs.
                    let key=if cfg!(target_os="macos") {item.key.and_then(|s|s.parse().ok())} else {None};
                    if matches!(item.command,Command::Quit) {
                        #[cfg(target_os="macos")] {
                            let _=submenu.append(&PredefinedMenuItem::separator());
                            let _=submenu.append(&PredefinedMenuItem::services(Some("Services")));
                            let _=submenu.append(&PredefinedMenuItem::hide(Some("Hide nus")));
                            let _=submenu.append(&PredefinedMenuItem::hide_others(None));
                            let _=submenu.append(&PredefinedMenuItem::show_all(None));
                        }
                        let _=submenu.append(&PredefinedMenuItem::separator());
                    }
                    let native=MenuItem::with_id(format!("nus-app-{i}"),item.label,true,key);
                    let _=submenu.append(&native); items.push(native);
                }
                let _=menu.append(&submenu);
                #[cfg(target_os="macos")] {
                    if group=="Window" {submenu.set_as_windows_menu_for_nsapp();}
                    if group=="Help" {submenu.set_as_help_menu_for_nsapp();}
                }
            }
            Self{menu,items}
        }
        #[cfg(not(any(windows,target_os="macos")))] { let _=proxy; Self }
    }
    pub fn attach(&self, window: &winit::window::Window) {
        #[cfg(windows)] {
            use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
            if let Ok(handle)=window.window_handle() {if let RawWindowHandle::Win32(h)=handle.as_raw(){let _=unsafe{self.menu.init_for_hwnd(h.hwnd.get())};}}
        }
        let _=window;
    }
    pub fn refresh(&self, app: &App) {
        #[cfg(any(windows,target_os="macos"))]
        for (native,item) in self.items.iter().zip(ITEMS) { native.set_enabled(app.command_enabled(item.command)); }
        let _=app;
    }
    pub fn activate_for_check(&self, command:Command) {
        #[cfg(target_os="macos")] {
            use objc2::{msg_send, runtime::AnyObject};
            use tray_icon::menu::ContextMenu;
            let item=ITEMS.iter().position(|i|i.command==command).unwrap();
            let group=ITEMS[item].group;
            let top=["nus","File","Edit","View","Window","Help"].iter().position(|g|*g==group).unwrap();
            let mut index=ITEMS[..item].iter().filter(|i|i.group==group).count();
            if group=="nus" {index+=2; if command==Command::Quit {index+=6;}}
            unsafe {
                let menu=self.menu.ns_menu() as *mut AnyObject;
                let app:*mut AnyObject=msg_send![objc2::class!(NSApplication), sharedApplication];
                let installed:*mut AnyObject=msg_send![app, mainMenu];
                assert_eq!(installed,menu,"nus menu must be installed on NSApplication");
                let parent:*mut AnyObject=msg_send![menu, itemAtIndex:top as isize];
                let submenu:*mut AnyObject=msg_send![parent, submenu];
                let _:()=msg_send![submenu, performActionForItemAtIndex:index as isize];
            }
        }
        #[cfg(not(target_os="macos"))] {let _=command;}
    }
    pub fn labels(&self) -> Vec<String> {
        #[cfg(any(windows,target_os="macos"))] { use tray_icon::menu::MenuItemKind; self.menu.items().iter().filter_map(|i|if let MenuItemKind::Submenu(s)=i{Some(s.text())}else{None}).collect() }
        #[cfg(not(any(windows,target_os="macos")))] { ["nus","File","Edit","View","Window","Help"].map(String::from).to_vec() }
    }
}

impl App {
    pub(crate) fn command_enabled(&self, command:Command)->bool {
        if crate::private::enabled() && matches!(command, Command::Settings | Command::NewShell | Command::History | Command::Hatch) { return false; }
        let index=if self.hatch.as_ref().is_some_and(|h|h.window.has_focus()) {self.hatch_tab().unwrap_or(self.active)} else {self.active};
        let pane=self.tabs.get(index).map(|t|t.focused_ref());
        let field=self.palette.is_some() || matches!(pane,Some(Pane::Home(_)));
        match command {
            Command::Reload|Command::Devtools=>matches!(pane,Some(Pane::Web(_))),
            Command::History=>matches!(pane,Some(Pane::Term(_))),
            Command::Undo|Command::Redo=>!field && matches!(pane,Some(Pane::Editor(_)|Pane::Web(_))),
            Command::Cut=>field || matches!(pane,Some(Pane::Editor(_)|Pane::Web(_))),
            Command::SelectAll=>!field && matches!(pane,Some(Pane::Editor(_)|Pane::Web(_))),
            Command::Find=>matches!(pane,Some(Pane::Term(_)|Pane::Web(_)|Pane::Editor(_)|Pane::Settings(_))),
            Command::Reopen=>!self.closed.is_empty(),
            Command::NextTab|Command::PreviousTab=>self.tabs.iter().filter(|t|!t.hatch).count()>1,
            _=>true,
        }
    }

    pub(crate) fn application_command(&mut self, command:Command) {
        if !self.command_enabled(command) {return;}
        use Command::*;
        match command {
            Settings=>self.open_settings(), Quit=>{let _=self.proxy.send_event(crate::UserEvent::HatchQuit);},
            NewTab=>self.open_start_page(false), NewShell=>self.run(Action::NewTerminal(self.behavior.default_profile)),
            NewWindow=>self.run(Action::NewWindow), CloseTab=>self.run(Action::CloseTab), CloseWindow=>{let _=self.proxy.send_event(crate::UserEvent::WindowControl(self.window.id(),0));}, Reopen=>self.run(Action::Reopen),
            NewPrivateWindow=>self.run(Action::NewPrivateWindow),
            ReportBug=>self.run(Action::Report(crate::support::Kind::Bug)),
            RequestFeature=>self.run(Action::Report(crate::support::Kind::Feature)),
            ZoomIn=>self.zoom_focused(1), ZoomOut=>self.zoom_focused(-1), ZoomReset=>self.zoom_focused(0),
            Home=>self.run(Action::Home), Downloads=>self.run(Action::Downloads), Palette=>self.open_palette(PaletteMode::Go),
            Sidebar=>self.run(Action::ToggleSidebar), Fullscreen=>self.toggle_fullscreen(), Split=>self.run(Action::ToggleSplit),
            History=>self.toggle_timeline(), Devtools=>self.toggle_devtools(), Hatch=>self.toggle_hatch(),
            Minimize=>self.window.set_minimized(true), Welcome=>self.run(Action::Welcome),
            Reload=>{if let Some(Pane::Web(w))=self.tabs.get(self.active).map(|t|t.focused_ref()){w.tab.reload();}},
            NextTab|PreviousTab=>{
                let ids:Vec<_>=self.tabs.iter().enumerate().filter(|(_,t)|!t.hatch).map(|(i,_)|i).collect();
                if let Some(i)=ids.iter().position(|i|*i==self.active) {self.activate(ids[(i+if command==NextTab{1}else{ids.len()-1})%ids.len()]);}
            }
            Undo|Redo|Cut|Copy|Paste|SelectAll|Find=>self.menu_edit(command),
        }
        self.dirty=true;
    }

    fn menu_edit(&mut self, command:Command) {
        use Command::*;
        // CEF's off-screen view is not AppKit's first responder. Native Edit
        // items must address its actual focused frame (including iframes).
        if self.palette.is_none() && command!=Find {
            if let Some(Pane::Web(w))=self.tabs.get(self.active).map(|t|t.focused_ref()) {
                let target=if w.focus_devtools{w.devtools.as_ref().unwrap_or(&w.tab)}else{&w.tab};
                if let Some(frame)=target.browser.focused_frame().or_else(||target.browser.main_frame()) {
                    match command {Undo=>frame.undo(),Redo=>frame.redo(),Cut=>frame.cut(),Copy=>frame.copy(),Paste=>frame.paste(),SelectAll=>frame.select_all(),_=>{}}
                }
                return;
            }
        }
        let (code,text)=match command {Undo|Redo=>(KeyCode::KeyZ,"z"),Cut=>(KeyCode::KeyX,"x"),Copy=>(KeyCode::KeyC,"c"),Paste=>(KeyCode::KeyV,"v"),SelectAll=>(KeyCode::KeyA,"a"),Find=>(KeyCode::KeyF,"f"),_=>return};
        let saved=self.mods;
        let terminal=matches!(self.tabs.get(self.active).map(|t|t.focused_ref()),Some(Pane::Term(_)));
        self.mods=if cfg!(target_os="macos") {ModifiersState::SUPER} else {ModifiersState::CONTROL};
        if command==Redo || (terminal&&!cfg!(target_os="macos")) {self.mods|=ModifiersState::SHIFT;}
        let ev=KeyIn{physical_key:PhysicalKey::Code(code),logical_key:Key::Character(text.into()),text:None,state:winit::event::ElementState::Pressed,repeat:false};
        // Editors need their standard editing chord before global app chords.
        if !(self.palette.is_none() && self.editor_key(&ev)) {self.key_in(&ev);}
        self.mods=saved;
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn every_menu_item_has_a_unique_round_tripping_command() {
        for (i,item) in ITEMS.iter().enumerate(){assert_eq!(from_id(&format!("nus-app-{i}")),Some(item.command));assert_eq!(ITEMS.iter().filter(|v|v.command==item.command).count(),1);}
        assert_eq!(from_id("hatch-show"),None);assert_eq!(from_id("nus-app-999999"),None);
        #[cfg(any(windows,target_os="macos"))]
        for item in ITEMS {if let Some(key)=item.key{assert!(key.parse::<tray_icon::menu::accelerator::Accelerator>().is_ok(),"{key}");}}
    }
}

//! Browsing for a file, on every platform. The system's own dialog —
//! NSOpenPanel on macOS, the common item dialog on Windows, the desktop
//! portal on Linux.
//!
//! macOS uses a parented AppKit sheet; Windows uses its common item dialog;
//! Linux uses the desktop portal with an explicit runtime. Decoding runs off
//! the UI thread and replaces the saved avatar atomically.

use std::path::{Path, PathBuf};

/// Formats the picker offers and `image` can read.
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "tif", "ico"];

/// The side of the square we keep. The avatar draws at 22px; this is
/// enough for a retina footer, a settings row and the card's head.
const SIDE: u32 = 256;

/// Selection and image processing are both asynchronous to the UI.
pub struct Pending {
    dialog: Option<platform::Dialog>,
    writing: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    dest: PathBuf,
}
impl Pending {
    /// None is pending; Ok(false) is cancellation, Ok(true) is a saved image.
    pub fn poll(&mut self) -> Option<Result<bool, String>> {
        if let Some(dialog)=self.dialog.as_mut() {
            let picked=dialog.poll()?;
            self.dialog=None;
            match picked {
                Err(e)=>return Some(Err(e)),
                Ok(None)=>return Some(Ok(false)),
                Ok(Some(src))=> {
                    let dest=self.dest.clone();
                    let (tx,rx)=std::sync::mpsc::channel();
                    match std::thread::Builder::new().name("avatar-image".into()).spawn(move || {let _=tx.send(write_avatar(&src,&dest));}) {
                        Ok(_)=>self.writing=Some(rx),
                        Err(e)=>return Some(Err(short(&e.to_string()))),
                    }
                }
            }
        }
        match self.writing.as_ref()?.try_recv() {
            Ok(result)=>Some(result.map(|()|true)),
            Err(std::sync::mpsc::TryRecvError::Empty)=>None,
            Err(_)=>Some(Err("picture processing stopped".into())),
        }
    }
}

/// What a dialog is for: a picture, a folder, or a place to save a file.
#[derive(Clone, Debug)]
pub enum Kind {
    Picture,
    File,
    Folder,
    /// Save as: the folder to start in and the name to offer.
    Save { dir: PathBuf, name: String },
}

/// A dialog up for a folder or a save place; poll it each tick.
pub struct Picker {
    dialog: platform::Dialog,
}
impl Picker {
    /// None is pending; Ok(None) is cancellation.
    pub fn poll(&mut self) -> Option<Result<Option<PathBuf>, String>> {
        self.dialog.poll()
    }
}

/// The system's folder chooser.
pub fn folder(window: &winit::window::Window, title: &str) -> Result<Picker, String> {
    Ok(Picker { dialog: platform::Dialog::open(window, title, Kind::Folder)? })
}
pub fn file(window: &winit::window::Window, title: &str) -> Result<Picker, String> {
    Ok(Picker { dialog: platform::Dialog::open(window, title, Kind::File)? })
}

/// The system's save dialog, starting in `dir` with `name` offered.
pub fn save_as(window: &winit::window::Window, title: &str, dir: PathBuf, name: String) -> Result<Picker, String> {
    Ok(Picker { dialog: platform::Dialog::open(window, title, Kind::Save { dir, name })? })
}

pub fn image_file(window: &winit::window::Window, title: &str, dest: PathBuf) -> Result<Pending,String> {
    Ok(Pending{dialog:Some(platform::Dialog::open(window,title,Kind::Picture)?),writing:None,dest})
}

#[cfg(target_os="macos")]
mod platform {
    use super::*;
    use objc2::{rc::Retained,MainThreadMarker};
    use objc2_app_kit::{NSOpenPanel,NSSavePanel,NSView,NSModalResponseOK,NSModalResponseCancel};
    use objc2_foundation::{NSArray,NSString,NSURL};
    use std::{cell::RefCell,rc::Rc};
    use winit::raw_window_handle::{HasWindowHandle,RawWindowHandle};
    pub struct Dialog {panel:Retained<NSSavePanel>,result:Rc<RefCell<Option<Result<Option<PathBuf>,String>>>>,finished:bool}
    impl Dialog {
        pub fn open(window:&winit::window::Window,title:&str,kind:Kind)->Result<Self,String> {
            let mtm=MainThreadMarker::new().ok_or("open the chooser from the main window")?;
            let handle=window.window_handle().map_err(|e|short(&e.to_string()))?;
            let RawWindowHandle::AppKit(handle)=handle.as_raw() else {return Err("the window is unavailable".into());};
            let view=unsafe {&*handle.ns_view.as_ptr().cast::<NSView>()};
            let parent=view.window().ok_or("the window is unavailable")?;
            // An open panel is a save panel with more to say; the save
            // dialog is the plain one.
            let panel:Retained<NSSavePanel>=match &kind {
                Kind::Save{dir,name}=>{
                    let panel=NSSavePanel::savePanel(mtm);
                    panel.setNameFieldStringValue(&NSString::from_str(name));
                    panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()))));
                    panel.setCanCreateDirectories(true);
                    panel
                }
                Kind::Folder=>{
                    let panel=NSOpenPanel::openPanel(mtm);
                    panel.setCanChooseFiles(false);panel.setCanChooseDirectories(true);panel.setAllowsMultipleSelection(false);panel.setCanCreateDirectories(true);
                    Retained::into_super(panel)
                }
                Kind::File=>{
                    let panel=NSOpenPanel::openPanel(mtm);
                    panel.setCanChooseFiles(true);panel.setCanChooseDirectories(false);panel.setAllowsMultipleSelection(false);
                    Retained::into_super(panel)
                }
                Kind::Picture=>{
                    let panel=NSOpenPanel::openPanel(mtm);
                    panel.setCanChooseFiles(true);panel.setCanChooseDirectories(false);panel.setAllowsMultipleSelection(false);
                    let types:Vec<_>=IMAGE_EXTS.iter().map(|s|NSString::from_str(s)).collect();
                    // Available on all supported macOS versions; decoding validates content too.
                    #[allow(deprecated)] panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&types)));
                    Retained::into_super(panel)
                }
            };
            panel.setTitle(Some(&NSString::from_str(title)));
            let result=Rc::new(RefCell::new(None));let out=result.clone();let chosen=panel.clone();
            let callback=block2::RcBlock::new(move |response| {
                use std::{ffi::CStr,os::unix::ffi::OsStrExt};
                let answer=if response==NSModalResponseOK {
                    chosen.URL().map(|url| {
                        let bytes=unsafe {CStr::from_ptr(url.fileSystemRepresentation().as_ptr())}.to_bytes();
                        PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
                    }).map(Some).ok_or_else(||"the selected file is unavailable".to_string())
                } else if response==NSModalResponseCancel {Ok(None)} else {Err("the chooser could not open".into())};
                *out.borrow_mut()=Some(answer);
            });
            // Unlike rfd's run-modal fallback, a sheet cooperates with winit's pump.
            panel.beginSheetModalForWindow_completionHandler(&parent,&callback);
            Ok(Self{panel,result,finished:false})
        }
        pub fn poll(&mut self)->Option<Result<Option<PathBuf>,String>> {
            let answer=self.result.borrow_mut().take();
            if answer.is_some(){self.finished=true;}
            answer
        }
    }
    impl Drop for Dialog {fn drop(&mut self){if !self.finished {unsafe{self.panel.cancel(None);}}}}
}

#[cfg(not(target_os="macos"))]
mod platform {
    use super::*;
    use std::{future::Future,pin::Pin,task::{Context,Poll}};
    pub struct Dialog {
        future:Option<Pin<Box<dyn Future<Output=Option<rfd::FileHandle>>>>>,
        // Worker threads drive portal I/O while the UI only polls completion.
        #[cfg(target_os="linux")] runtime:&'static tokio::runtime::Runtime,
    }
    impl Drop for Dialog {fn drop(&mut self){
        #[cfg(target_os="linux")] let _guard=self.runtime.enter();
        drop(self.future.take());
    }}
    impl Dialog {
        pub fn open(window:&winit::window::Window,title:&str,kind:Kind)->Result<Self,String> {
            #[cfg(target_os="linux")]
            let runtime={
                static PORTAL_RUNTIME:std::sync::OnceLock<Result<tokio::runtime::Runtime,std::io::Error>>=std::sync::OnceLock::new();
                PORTAL_RUNTIME.get_or_init(||tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build()).as_ref().map_err(|e|short(&e.to_string()))?
            };
            #[cfg(target_os="linux")] let guard=runtime.enter();
            let dialog=rfd::AsyncFileDialog::new().set_parent(window).set_title(title);
            let future:Pin<Box<dyn Future<Output=Option<rfd::FileHandle>>>>=match kind {
                Kind::Picture=>Box::pin(dialog.add_filter("Pictures",IMAGE_EXTS).pick_file()),
                Kind::File=>Box::pin(dialog.pick_file()),
                Kind::Folder=>Box::pin(dialog.pick_folder()),
                Kind::Save{dir,name}=>Box::pin(dialog.set_directory(dir).set_file_name(name).save_file()),
            };
            #[cfg(target_os="linux")] drop(guard);
            Ok(Self{future:Some(future),#[cfg(target_os="linux")] runtime})
        }
        pub fn poll(&mut self)->Option<Result<Option<PathBuf>,String>> {
            #[cfg(target_os="linux")] let _guard=self.runtime.enter();
            let mut cx=Context::from_waker(std::task::Waker::noop());
            match self.future.as_mut()?.as_mut().poll(&mut cx) {Poll::Pending=>None,Poll::Ready(file)=>Some(Ok(file.map(|f|f.path().to_path_buf())))}
        }
    }
}

/// Read `src`, take the largest square from the middle of it, and write
/// it to `dest` as a PNG. The error is a line fit to show the user.
pub fn write_square_png(src: &Path, dest: &Path, side: u32) -> Result<(), String> {
    use image::{GenericImageView,ImageDecoder};
    if side==0 || side>4096 {return Err("picture size must be between 1 and 4096 pixels".into());}
    let meta=std::fs::metadata(src).map_err(|e|short(&e.to_string()))?;
    if !meta.is_file() || meta.len()>64*1024*1024 {return Err("choose a picture file smaller than 64 MB".into());}
    let mut reader=image::ImageReader::open(src).map_err(|e|short(&e.to_string()))?.with_guessed_format().map_err(|e|short(&e.to_string()))?;
    let mut limits=image::Limits::default();limits.max_image_width=Some(16384);limits.max_image_height=Some(16384);limits.max_alloc=Some(256*1024*1024);
    reader.limits(limits);
    let mut decoder=reader.into_decoder().map_err(|e|short(&e.to_string()))?;
    let orientation=decoder.orientation().map_err(|e|short(&e.to_string()))?;
    let mut img=image::DynamicImage::from_decoder(decoder).map_err(|e|short(&e.to_string()))?;
    img.apply_orientation(orientation);
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return Err("that picture is empty".into());
    }
    let edge = w.min(h);
    let img = image::imageops::crop_imm(&img, (w - edge) / 2, (h - edge) / 2, edge, edge).to_image();
    let img = image::imageops::resize(&img, side, side, image::imageops::FilterType::Lanczos3);
    let dir=dest.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e|short(&e.to_string()))?;
    let mut file=tempfile::NamedTempFile::new_in(dir).map_err(|e|short(&e.to_string()))?;
    image::DynamicImage::ImageRgba8(img).write_to(&mut file,image::ImageFormat::Png).map_err(|e|short(&e.to_string()))?;
    file.as_file().sync_all().map_err(|e|short(&e.to_string()))?;
    file.persist(dest).map_err(|e|short(&e.to_string()))?;
    Ok(())
}

/// The same, at the size the avatar is kept at.
pub fn write_avatar(src: &Path, dest: &Path) -> Result<(), String> {
    write_square_png(src, dest, SIDE)
}

/// One line, lowercase, no trailing stop — the shape of every other notice.
fn short(e: &str) -> String {
    let e = e.trim().trim_end_matches('.');
    let first = e.lines().next().unwrap_or(e);
    let mut s = first.to_string();
    if let Some((end,_)) = s.char_indices().nth(90) {
        s.truncate(end);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn a_wide_picture_comes_back_square() {
        let dir = std::env::temp_dir().join(format!("nus-pick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("wide.png");
        image::RgbaImage::from_pixel(120, 40, image::Rgba([10, 20, 30, 255])).save(&src).unwrap();
        let dest = dir.join("avatar.png");
        write_square_png(&src, &dest, 64).unwrap();
        assert_eq!(image::open(&dest).unwrap().dimensions(), (64, 64));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_jpeg_is_kept_as_a_png() {
        let dir = std::env::temp_dir().join(format!("nus-pick-jpg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("face.jpg");
        image::RgbImage::from_pixel(300, 200, image::Rgb([200, 30, 40])).save(&src).unwrap();
        let dest = dir.join("avatar.png");
        write_square_png(&src, &dest, 64).unwrap();
        // Written as a PNG whatever came in, because that is what the
        // avatar loader reads.
        assert_eq!(image::ImageReader::open(&dest).unwrap().format(), Some(image::ImageFormat::Png));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_picture_that_is_not_one_says_so() {
        let dir = std::env::temp_dir().join(format!("nus-pick-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("notes.png");
        std::fs::write(&src, b"not a picture").unwrap();
        assert!(write_square_png(&src, &dir.join("avatar.png"), 64).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn unicode_errors_are_safe_and_single_line() {
        assert_eq!(short(&format!("{}\nmore", "é🦀".repeat(60))).chars().count(),91);
        assert_eq!(short("  failed.\nsecond line  "),"failed.");
    }
    #[test]
    fn failed_decode_and_invalid_size_preserve_the_existing_avatar() {
        let dir=tempfile::tempdir().unwrap();let dest=dir.path().join("avatar.png");
        let original=b"previous avatar";std::fs::write(&dest,original).unwrap();
        let src=dir.path().join("broken.png");std::fs::write(&src,b"broken").unwrap();
        assert!(write_avatar(&src,&dest).is_err());assert_eq!(std::fs::read(&dest).unwrap(),original);
        assert!(write_square_png(&src,&dest,0).is_err());assert!(write_square_png(&src,&dest,4097).is_err());
        assert_eq!(std::fs::read(&dest).unwrap(),original);
    }
    #[test]
    fn replaces_existing_avatar_and_can_choose_itself() {
        let dir=tempfile::tempdir().unwrap();let dest=dir.path().join("头像 🦀.png");
        image::RgbaImage::from_pixel(40,80,image::Rgba([20,30,40,255])).save(&dest).unwrap();
        write_avatar(&dest,&dest).unwrap();assert_eq!(image::open(&dest).unwrap().dimensions(),(SIDE,SIDE));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(),1,"no temporary files left behind");
    }

}

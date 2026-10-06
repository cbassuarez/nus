//! Process-owned Get URL receiver. It does not replace winit's NSApplication
//! delegate or intercept CEF subprocesses. Install before Dock/event pumping.
use objc2::{class,define_class,msg_send,sel,MainThreadOnly,MainThreadMarker};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::{NSObject,NSObjectProtocol,NSString};

const INTERNET:u32=u32::from_be_bytes(*b"GURL");
const GET_URL:u32=u32::from_be_bytes(*b"GURL");
const DIRECT_OBJECT:u32=u32::from_be_bytes(*b"----");

define_class!(
    #[unsafe(super=NSObject)]
    #[thread_kind=MainThreadOnly]
    #[name="NusExternalWebReceiver"]
    struct Receiver;
    unsafe impl NSObjectProtocol for Receiver {}
    impl Receiver {
        #[unsafe(method(nusOpenURL:withReplyEvent:))]
        fn receive(&self,event:&AnyObject,_reply:Option<&AnyObject>) {
            let descriptor:Option<Retained<AnyObject>>=unsafe{msg_send![event,paramDescriptorForKeyword:DIRECT_OBJECT]};
            let value:Option<Retained<NSString>>=descriptor.and_then(|d|unsafe{msg_send![&*d,stringValue]});
            if let Some(value)=value {
                if let Err(e)=crate::external_open::enqueue(value.to_string()) {eprintln!("nus external activation: {e}");}
            }
        }
    }
);
/// Documents the system opens in nus (Finder, `open`, a default chosen in
/// FILE VIEWERS) come to the app delegate as `application:openURLs:`.
/// winit's delegate has no such method, so it's added to its class once
/// the event loop has made it, before launching finishes. A web link that
/// comes this way goes where the Get URL receiver sends one.
pub fn accept_documents() {
    use objc2::ffi;
    use objc2::runtime::{AnyClass,Imp,Sel};
    unsafe extern "C-unwind" fn open_urls(_this:*mut AnyObject,_sel:Sel,_app:*mut AnyObject,urls:*mut AnyObject) {
        if urls.is_null() {return;}
        let n:usize=unsafe{msg_send![urls,count]};
        for i in 0..n.min(128) {
            let url:*mut AnyObject=unsafe{msg_send![urls,objectAtIndex:i]};
            let text:Option<Retained<NSString>>=unsafe{msg_send![url,absoluteString]};
            let Some(text)=text.map(|t|t.to_string()) else {continue};
            let queued=if text.starts_with("file:") {crate::external_open::enqueue_file(text)} else {crate::external_open::enqueue(text)};
            if let Err(e)=queued {eprintln!("nus external activation: {e}");}
        }
    }
    let Some(cls)=AnyClass::get(c"WinitApplicationDelegate") else {return};
    unsafe {
        let imp:Imp=std::mem::transmute(open_urls as unsafe extern "C-unwind" fn(*mut AnyObject,Sel,*mut AnyObject,*mut AnyObject));
        let _=ffi::class_addMethod(cls as *const AnyClass as *mut AnyClass,sel!(application:openURLs:),imp,c"v@:@@".as_ptr());
    }
}

pub struct Registration { manager:Retained<AnyObject>, _receiver:Retained<Receiver> }
impl Drop for Registration {
    fn drop(&mut self){unsafe{let _:()=msg_send![&*self.manager,removeEventHandlerForEventClass:INTERNET,andEventID:GET_URL];}}
}
pub fn install()->Option<Registration> {
    let mtm=MainThreadMarker::new()?;
    // Initialize the unit ivars before calling the superclass initializer.
    // objc2 requires PartialInit here, even for a class with no stored fields.
    let allocated=Receiver::alloc(mtm).set_ivars(());
    let receiver:Retained<Receiver>=unsafe{msg_send![super(allocated),init]};
    let manager:Retained<AnyObject>=unsafe{msg_send![class!(NSAppleEventManager),sharedAppleEventManager]};
    unsafe {
        let _:()=msg_send![&*manager,setEventHandler:&*receiver,
            andSelector:sel!(nusOpenURL:withReplyEvent:),forEventClass:INTERNET,andEventID:GET_URL];
    }
    Some(Registration{manager,_receiver:receiver})
}

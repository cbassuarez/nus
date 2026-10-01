//! Elastic overscroll: a page pulled past its end stretches, showing what
//! is behind it, and springs back when the fingers stop.
//!
//! Chromium does this itself only for real trackpad gestures. The wheel
//! events nus hands CEF have no phase, so Chromium marks every scroll
//! synthetic and its rubber band never starts. Instead the page reports,
//! through the `nusOverscroll` binding, a wheel it had no room for: the
//! root at its end and nothing under the pointer left to scroll that way,
//! and `overscroll-behavior` not `none`. nus moves the drawn page, so the
//! page's own layout, fixed bars and scroll position never change.
//!
//! Sideways, the same report (`x` and the distance) is what lets a swipe
//! go back or forward (swipe.rs): a wheel something on the page could
//! still scroll is the page's, never a navigation. The script says `r`
//! once it is listening, so a page without it (still loading, an error
//! page, a PDF) never keeps a swipe waiting for a word it cannot send.
//!
//! Only a page that scrolls stretches: a root with nothing to scroll that
//! way, or held still by `overflow: hidden` (an app, an open dialog), has
//! no end to pull past.
use std::time::Instant;

/// Injected into every document with the other page scripts.
pub const JS: &str = r#"(()=>{try{
if(window.top!==window||window.__nusOverscroll)return;window.__nusOverscroll=true;
const scrolls=(el,dy)=>{const s=getComputedStyle(el);
 if(!/(auto|scroll|overlay)/.test(s.overflowY)||el.scrollHeight<=el.clientHeight+1)return 0;
 if(dy<0?el.scrollTop>0:el.scrollTop+el.clientHeight<el.scrollHeight-1)return 1;
 return s.overscrollBehaviorY==='auto'?0:1};
const scrollsX=(el,dx)=>{const s=getComputedStyle(el);
 if(!/(auto|scroll|overlay)/.test(s.overflowX)||el.scrollWidth<=el.clientWidth+1)return 0;
 const left=Math.abs(el.scrollLeft);
 if(dx<0?left>0:left+el.clientWidth<el.scrollWidth-1)return 1;
 return s.overscrollBehaviorX==='auto'?0:1};
addEventListener('wheel',e=>{try{
 if(e.defaultPrevented||e.ctrlKey)return;
 const unit=e.deltaMode===1?16:e.deltaMode===2?innerHeight:1;
 const dy=e.deltaY*unit,dx=e.deltaX*unit;
 const html=document.documentElement,body=document.body;
 if(dx&&Math.abs(dx)>Math.abs(dy)){
  for(let el=e.target instanceof Element?e.target:null;el&&el!==html&&el!==body;el=el.parentElement||(el.getRootNode()&&el.getRootNode().host)||null){if(scrollsX(el,dx))return}
  if([html,body].some(el=>el&&getComputedStyle(el).overscrollBehaviorX==='none'))return;
  const root=document.scrollingElement||html,left=Math.abs(root.scrollLeft);
  if(dx<0?left<=0:left+innerWidth>=root.scrollWidth-1)nusOverscroll('x'+dx);
  return}
 if(!dy)return;
 for(let el=e.target instanceof Element?e.target:null;el&&el!==html&&el!==body;el=el.parentElement||(el.getRootNode()&&el.getRootNode().host)||null){if(scrolls(el,dy))return}
 if([html,body].some(el=>el&&(getComputedStyle(el).overscrollBehaviorY==='none'||/(hidden|clip)/.test(getComputedStyle(el).overflowY))))return;
 const root=document.scrollingElement||html;
 if(root.scrollHeight<=innerHeight+1)return;
 if(dy<0?root.scrollTop<=0:root.scrollTop+innerHeight>=root.scrollHeight-1)nusOverscroll(String(dy));
}catch(_){}},{passive:true});
nusOverscroll('r');addEventListener('load',()=>nusOverscroll('r'));addEventListener('pageshow',()=>nusOverscroll('r'));
}catch(_){}})()"#;

/// How long after the last report the page counts as let go, where the
/// wheel has no phases to say so (a Windows touchpad).
const LET_GO_MS: u128 = 90;

/// After the fingers lift, momentum may still hit the end: the page gives
/// for this long from the first such report, then the rest is spent.
const IMPACT_MS: u128 = 120;

/// One page's stretch.
#[derive(Default)]
pub struct Bounce {
    /// Wheel distance past the end, in CSS pixels; + is past the bottom.
    pull: f32,
    /// The drawn offset, easing toward the rubber band of `pull`.
    shown: f32,
    last: Option<Instant>,
    tick: Option<Instant>,
    /// The fingers are up: only momentum is arriving.
    lifted: bool,
    /// The first momentum report past the end since they lifted.
    impact: Option<Instant>,
}

impl Bounce {
    /// A report from the page: `dy` more past the end.
    pub fn push(&mut self, dy: f32) {
        if self.lifted {
            // Momentum running into the end: one give, not a page held
            // stretched for as long as the glide lasts.
            let now = crate::clock::now();
            let at = *self.impact.get_or_insert(now);
            if now.duration_since(at).as_millis() > IMPACT_MS {
                return;
            }
        }
        // Turning around lets go of what was pulled the other way.
        if dy.signum() != self.pull.signum() && self.pull != 0.0 {
            self.pull = 0.0;
        }
        self.pull = (self.pull + dy).clamp(-4000.0, 4000.0);
        self.last = Some(crate::clock::now());
    }

    /// Advance one frame; `reach` is the most the page may move (logical
    /// px). Returns the offset to draw the page at (+ moves it up) and
    /// whether it is still moving.
    pub fn step(&mut self, reach: f32) -> (f32, bool) {
        let now = crate::clock::now();
        if self.last.is_some_and(|t| now.duration_since(t).as_millis() > LET_GO_MS) {
            self.pull = 0.0;
            self.last = None;
        }
        if self.lifted && self.impact.is_some_and(|t| now.duration_since(t).as_millis() > IMPACT_MS) {
            self.pull = 0.0;
            self.last = None;
        }
        let target = rubber(self.pull, reach);
        let dt = self.tick.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(1.0 / 60.0).min(0.1);
        self.tick = Some(now);
        // Following the fingers is immediate; the spring back is quick.
        let rate = if self.last.is_some() { 45.0 } else { 18.0 };
        self.shown += (target - self.shown) * (1.0 - (-rate * dt).exp());
        if (target - self.shown).abs() < 0.25 {
            self.shown = target;
        }
        let moving = self.shown != target || self.last.is_some();
        if !moving {
            self.tick = None;
        }
        (self.shown, moving)
    }

    pub fn stop(&mut self) {
        *self = Bounce::default();
    }

    /// The fingers lifted: spring back now, not after the glide ends.
    pub fn let_go(&mut self) {
        self.pull = 0.0;
        self.last = None;
        self.lifted = true;
        self.impact = None;
    }

    /// Fingers down for a new gesture.
    pub fn touch(&mut self) {
        self.lifted = false;
        self.impact = None;
    }
}

/// The rubber band: the further past the end, the less each pixel moves
/// it, never beyond `reach`.
fn rubber(pull: f32, reach: f32) -> f32 {
    if reach <= 0.0 {
        return 0.0;
    }
    let x = pull.abs();
    pull.signum() * (1.0 - 1.0 / (x * 0.55 / reach + 1.0)) * reach
}

#[cfg(test)]
mod tests {
    use super::{rubber, Bounce};

    #[test]
    fn lifting_springs_back_and_the_glide_gives_only_once() {
        let mut b = Bounce::default();
        b.push(80.0);
        assert!(b.pull > 0.0);
        b.let_go();
        assert_eq!(b.pull, 0.0, "fingers up: no pull left to hold the page");
        b.push(40.0);
        assert_eq!(b.pull, 40.0, "momentum into the end still gives");
        b.touch();
        assert!(!b.lifted && b.impact.is_none(), "new fingers start fresh");
    }

    #[test]
    fn the_band_resists_and_never_passes_its_reach() {
        assert_eq!(rubber(0.0, 120.0), 0.0);
        let a = rubber(50.0, 120.0);
        let b = rubber(100.0, 120.0);
        assert!(a > 0.0 && a < 50.0);
        assert!(b > a && b - a < a, "each pixel pulls less than the last");
        assert!(rubber(1.0e6, 120.0) < 120.0);
        assert_eq!(rubber(-50.0, 120.0), -a);
    }
}

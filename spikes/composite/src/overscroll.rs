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
use std::time::Instant;

/// Injected into every document with the other page scripts.
pub const JS: &str = r#"(()=>{try{
if(window.top!==window||window.__nusOverscroll)return;window.__nusOverscroll=true;
const scrolls=(el,dy)=>{const s=getComputedStyle(el);
 if(!/(auto|scroll|overlay)/.test(s.overflowY)||el.scrollHeight<=el.clientHeight+1)return 0;
 if(dy<0?el.scrollTop>0:el.scrollTop+el.clientHeight<el.scrollHeight-1)return 1;
 return s.overscrollBehaviorY==='auto'?0:1};
addEventListener('wheel',e=>{try{
 if(e.defaultPrevented||e.ctrlKey)return;
 const dy=e.deltaY*(e.deltaMode===1?16:e.deltaMode===2?innerHeight:1);
 if(!dy||Math.abs(e.deltaX)>Math.abs(dy))return;
 const html=document.documentElement,body=document.body;
 for(let el=e.target instanceof Element?e.target:null;el&&el!==html&&el!==body;el=el.parentElement||(el.getRootNode()&&el.getRootNode().host)||null){if(scrolls(el,dy))return}
 if([html,body].some(el=>el&&getComputedStyle(el).overscrollBehaviorY==='none'))return;
 const root=document.scrollingElement||html;
 if(dy<0?root.scrollTop<=0:root.scrollTop+innerHeight>=root.scrollHeight-1)nusOverscroll(String(dy));
}catch(_){}},{passive:true});
}catch(_){}})()"#;

/// How long after the last report the page counts as let go.
const LET_GO_MS: u128 = 90;

/// One page's stretch.
#[derive(Default)]
pub struct Bounce {
    /// Wheel distance past the end, in CSS pixels; + is past the bottom.
    pull: f32,
    /// The drawn offset, easing toward the rubber band of `pull`.
    shown: f32,
    last: Option<Instant>,
    tick: Option<Instant>,
}

impl Bounce {
    /// A report from the page: `dy` more past the end.
    pub fn push(&mut self, dy: f32) {
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
        let target = rubber(self.pull, reach);
        let dt = self.tick.map(|t| now.duration_since(t).as_secs_f32()).unwrap_or(1.0 / 60.0).min(0.1);
        self.tick = Some(now);
        // Following the fingers is quick; the spring back is gentler.
        let rate = if self.last.is_some() { 30.0 } else { 12.0 };
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
    use super::rubber;

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

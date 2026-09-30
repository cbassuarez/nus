//! Clock and interaction policy translated from limb-darkroom-template.html.
//! No OS, wgpu, thread, timer or wall-clock dependencies. `now` is monotonic
//! seconds supplied by the host; next_wake is a deadline, never a polling loop.
const IDLE: f64 = 65.0;
const TYPING: f64 = 6.0;
const FRAME: f64 = 1.0 / 60.0;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { Turn, Hold }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame { pub time: f32, pub phase: f32, pub blend: f32 }
#[derive(Debug)]
pub struct Motion {
    time: f64, blend: f64, from: f64, target: f64,
    elapsed: f64, duration: f64, idle: f64,
    transitioning: bool, manual: bool, held: bool,
    eligible: bool, composing: bool, reduced: bool,
    typing_until: f64, last: Option<f64>,
}
impl Default for Motion {
    fn default() -> Self { Self { time:0.0,blend:0.0,from:0.0,target:0.0,elapsed:0.0,duration:10.0,idle:0.0,
        transitioning:false,manual:false,held:false,eligible:false,composing:false,reduced:false,typing_until:0.0,last:None } }
}
impl Motion {
    /// Call on visibility/focus/composition changes, including before hiding.
    /// Stable environment calls are cheap and do not advance a dormant clock.
    pub fn environment(&mut self, now:f64, eligible:bool, composing:bool, reduced:bool) {
        if self.eligible==eligible && self.composing==composing && self.reduced==reduced {return;}
        self.advance(now);
        self.eligible=eligible;
        if self.composing && !composing {self.typing_until=now+TYPING;}
        self.composing=composing;
        if reduced {self.held=true;self.manual=false;}
        self.reduced=reduced;
        self.last=Some(now);
    }
    pub fn typed(&mut self, now:f64) { self.advance(now);self.typing_until=now+TYPING; }
    pub fn act(&mut self, action:Action, now:f64) {
        self.advance(now);
        match action {
            Action::Turn => {
                let target=if self.outward(){0.0}else{1.0};
                self.from=self.blend;self.target=target;self.elapsed=0.0;self.duration=10.0;self.idle=0.0;
                self.typing_until=0.0;self.manual=true;
                if self.reduced {self.blend=target;self.transitioning=false;self.manual=false;}
                else {self.transitioning=(self.blend-target).abs()>0.0001;}
            }
            Action::Hold => { if !self.reduced {self.held=!self.held;}self.manual=false; }
        }
        self.last=Some(now);
    }
    fn advance(&mut self, now:f64) {
        if !now.is_finite() {return;}
        let previous=self.last.replace(now).unwrap_or(now);
        if now<previous || !self.eligible || self.composing || self.reduced || (self.held&&!self.manual) {return;}
        let dt=(now-previous.max(self.typing_until)).max(0.0);
        if self.transitioning {
            let dt=dt.min(0.12);
            if !self.held {self.time+=dt;}
            self.elapsed+=dt;
            let t=(self.elapsed/self.duration).clamp(0.0,1.0);
            let ease=t*t*t*(t*(t*6.0-15.0)+10.0);
            self.blend=self.from+(self.target-self.from)*ease;
            if t>=1.0-1e-12 {self.blend=self.target;self.transitioning=false;self.manual=false;self.idle=0.0;}
        } else if !self.held {
            self.idle=(self.idle+dt).min(IDLE);
            if self.idle>=IDLE {
                self.from=self.blend;self.target=if self.blend>0.5 {0.0}else{1.0};self.elapsed=0.0;self.duration=18.0;
                self.transitioning=true;self.manual=false;
            }
        }
    }
    pub fn frame(&mut self, now:f64)->Frame {
        self.advance(now);
        Frame {time:self.time as f32,phase:(0.5+0.46*(self.time*std::f64::consts::TAU/110.0+0.30).cos()) as f32,blend:self.blend as f32}
    }
    pub fn next_wake(&self, now:f64)->Option<f64> {
        if !self.eligible || self.composing || self.reduced || (self.held&&!self.manual) {return None;}
        if now<self.typing_until {return Some(self.typing_until);}
        if self.transitioning {Some(now+FRAME)}else{Some(now+(IDLE-self.idle).max(0.0))}
    }
    pub fn outward(&self)->bool {if self.transitioning {self.target>0.5}else{self.blend>0.5}}
    pub fn held(&self)->bool {self.held}
    pub fn transitioning(&self)->bool {self.transitioning}
    pub fn action_label(&self, action:Action)->&'static str {
        match action {Action::Turn=>if self.outward(){"Return to orbit"}else{"Look outward"},
            Action::Hold=>if self.reduced{"Reduced motion"}else if self.held{"Resume motion"}else{"Hold view"}}
    }
    pub fn caption(&self, now:f64)->&'static str {
        if self.composing || now<self.typing_until {return "Still while you type";}
        if self.held&&!self.manual {return if self.transitioning {"Turn held"}else if self.blend>0.5 {"Darkroom · view held"}else{"Limb · view held"};}
        if self.transitioning {return if self.target>0.5 {"Turning toward interstellar dust"}else{"Returning to Earth"};}
        if self.blend>0.5 {"Constellations · resting between turns"}else{"Earth · resting between turns"}
    }
}
#[cfg(test)] mod tests {
    use super::*;
    fn live()->Motion {let mut m=Motion::default();m.environment(0.0,true,false,false);m}
    fn drive(m:&mut Motion,start:f64,seconds:u32) {for i in 1..=seconds*60 {m.frame(start+i as f64/60.0);}}
    #[test]fn begins_in_recovered_earth_pose(){let mut m=live();let f=m.frame(0.0);assert_eq!(f.blend,0.0);assert!((f.phase-0.9394548).abs()<1e-6);assert_eq!(m.next_wake(0.0),Some(65.0));}
    #[test]fn rest_does_not_move_clouds(){let mut m=live();assert_eq!(m.frame(0.0),m.frame(64.0));assert!(!m.transitioning());m.frame(65.0);assert!(m.transitioning());}
    #[test]fn automatic_turn_takes_eighteen_active_seconds(){let mut m=live();m.frame(65.0);drive(&mut m,65.0,18);assert_eq!(m.frame(83.0).blend,1.0);assert!(!m.transitioning());}
    #[test]fn explicit_turn_takes_ten_seconds(){let mut m=live();m.act(Action::Turn,0.0);drive(&mut m,0.0,5);assert!((m.frame(5.0).blend-0.5).abs()<1e-5);drive(&mut m,5.0,5);assert_eq!(m.frame(10.0).blend,1.0);}
    #[test]fn hold_preserves_remaining_idle_time(){let mut m=live();m.frame(20.0);m.act(Action::Hold,20.0);m.frame(100.0);assert_eq!(m.next_wake(100.0),None);m.act(Action::Hold,100.0);assert_eq!(m.next_wake(100.0),Some(145.0));}
    #[test]fn hidden_time_does_not_advance_rest(){let mut m=live();m.environment(20.0,false,false,false);m.environment(120.0,true,false,false);assert_eq!(m.next_wake(120.0),Some(165.0));}
    #[test]fn hidden_transition_does_not_catch_up(){let mut m=live();m.act(Action::Turn,0.0);drive(&mut m,0.0,2);let f=m.frame(2.0);m.environment(2.0,false,false,false);m.environment(100.0,true,false,false);assert_eq!(m.frame(100.0),f);}
    #[test]fn typing_pauses_for_six_seconds(){let mut m=live();m.act(Action::Turn,0.0);drive(&mut m,0.0,1);m.typed(1.0);let f=m.frame(1.0);assert_eq!(m.frame(6.0),f);assert_eq!(m.next_wake(6.0),Some(7.0));assert_eq!(m.frame(7.0),f);assert!(m.frame(7.1).blend>f.blend);}
    #[test]fn composition_holds_until_ended_and_quiet(){let mut m=live();m.environment(1.0,true,true,false);assert_eq!(m.next_wake(1.0),None);m.environment(8.0,true,false,false);assert_eq!(m.next_wake(8.0),Some(14.0));}
    #[test]fn manual_selection_while_held_moves_blend_not_clouds(){let mut m=live();m.act(Action::Hold,0.0);m.act(Action::Turn,0.0);drive(&mut m,0.0,10);let f=m.frame(10.0);assert_eq!(f.blend,1.0);assert_eq!(f.time,0.0);assert!(m.held());}
    #[test]fn reduced_motion_snaps_only_explicit_view_changes(){let mut m=live();m.environment(0.0,true,false,true);m.frame(500.0);assert_eq!(m.frame(500.0).blend,0.0);m.act(Action::Turn,500.0);assert_eq!(m.frame(500.0).blend,1.0);assert_eq!(m.next_wake(500.0),None);}
    #[test]fn reverse_mid_turn_is_continuous(){let mut m=live();m.act(Action::Turn,0.0);drive(&mut m,0.0,3);let b=m.frame(3.0).blend;m.act(Action::Turn,3.0);assert_eq!(m.frame(3.0).blend,b);drive(&mut m,3.0,10);assert_eq!(m.frame(13.0).blend,0.0);}
    #[test]fn long_host_stall_is_not_a_camera_jump(){let mut m=live();m.act(Action::Turn,0.0);m.frame(100.0);assert!(m.frame(100.0).blend<0.001);}
}

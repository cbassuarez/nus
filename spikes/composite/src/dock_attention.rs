//! Bounded attention, shared by native Dock/taskbar implementations.
use nus_render::dock_icon::{self, Face};
use std::time::Instant;

const DURATION: f32 = dock_icon::STEP_SECONDS * 6.0 * 3.0;
#[derive(Default)]
pub(super) struct Cycle {
    started: Option<Instant>,
    last: Option<Instant>,
}
impl Cycle {
    pub fn begin(&mut self, now: Instant, reduced: bool, focused: bool) -> bool {
        if reduced
            || focused
            || self
                .last
                .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() < 2.0)
        {
            return false;
        }
        self.started = Some(now);
        self.last = Some(now);
        true
    }
    pub fn tick(&mut self, now: Instant, reduced: bool, focused: bool) -> Face {
        if reduced
            || focused
            || self
                .started
                .is_some_and(|at| now.saturating_duration_since(at).as_secs_f32() >= DURATION)
        {
            self.stop();
        }
        self.started
            .map(|at| dock_icon::launch_face_at(now.saturating_duration_since(at).as_secs_f32()))
            .unwrap_or(Face::Newsreader)
    }
    pub fn active(&self) -> bool {
        self.started.is_some()
    }
    pub fn stop(&mut self) {
        self.started = None;
    }
}

// Wayland owns launcher artwork. Its urgency request still works where the
// compositor supports xdg-activation; never rewrite desktop files per frame.
#[cfg(any(target_os = "linux", target_os = "windows"))]
mod desktop {
    use super::*;
    use nus_render::Color;
    use std::sync::{mpsc, Arc};
    use winit::{
        raw_window_handle::{HasWindowHandle, RawWindowHandle},
        window::{Icon, UserAttentionType, Window},
    };

    pub struct Desktop {
        cycle: Cycle,
        window: Option<Arc<Window>>,
        icons: Vec<Icon>,
        shown: Option<Face>,
        signal: Option<Color>,
        send: mpsc::Sender<Color>,
        receive: mpsc::Receiver<(Color, Vec<Icon>)>,
    }
    impl Default for Desktop {
        fn default() -> Self {
            let (send, jobs) = mpsc::channel();
            let (results, receive) = mpsc::channel();
            std::thread::Builder::new()
                .name("attention artwork".into())
                .spawn(move || {
                    let fields: Vec<_> = Face::ALL
                        .into_iter()
                        .map(|face| dock_icon::Field::new(64, face))
                        .collect();
                    while let Ok(mut signal) = jobs.recv() {
                        for next in jobs.try_iter() {
                            signal = next;
                        }
                        let icons = fields
                            .iter()
                            .map(|field| {
                                Icon::from_rgba(field.frame(signal), 64, 64)
                                    .expect("generated icon")
                            })
                            .collect();
                        if results.send((signal, icons)).is_err() {
                            break;
                        }
                    }
                })
                .expect("attention artwork thread");
            Self {
                cycle: Cycle::default(),
                window: None,
                icons: Vec::new(),
                shown: None,
                signal: None,
                send,
                receive,
            }
        }
    }
    impl Desktop {
        pub fn begin(&mut self, window: &Arc<Window>, reduced: bool, focused: bool) {
            if self.cycle.begin(crate::clock::now(), reduced, focused) {
                self.restore();
                self.window = Some(window.clone());
                self.shown = None;
                window.request_user_attention(Some(UserAttentionType::Informational));
            }
        }
        pub fn tick(&mut self, signal: Color, reduced: bool, focused: bool) {
            if self.signal != Some(signal) {
                self.signal = Some(signal);
                let _ = self.send.send(signal);
            }
            for (color, icons) in self.receive.try_iter() {
                if color == signal {
                    self.icons = icons;
                    self.shown = None;
                }
            }
            let face = self.cycle.tick(crate::clock::now(), reduced, focused);
            if let Some(window) = &self.window {
                let wayland = window
                    .window_handle()
                    .is_ok_and(|h| matches!(h.as_raw(), RawWindowHandle::Wayland(_)));
                if !wayland && self.shown != Some(face) {
                    if let Some(icon) = self.icons.get(face as usize) {
                        window.set_window_icon(if crate::mercury::earned(){Icon::from_rgba(crate::mercury::icon(64),64,64).ok()}else{Some(icon.clone())});
                        self.shown = Some(face);
                    }
                }
            }
            if !self.cycle.active() {
                self.restore();
            }
        }
        fn restore(&mut self) {
            if let Some(window) = self.window.take() {
                window.request_user_attention(None);
                if let Some(icon) = self.icons.get(Face::Newsreader as usize) {
                    window.set_window_icon(if crate::mercury::earned(){Icon::from_rgba(crate::mercury::icon(64),64,64).ok()}else{Some(icon.clone())});
                }
            }
            self.shown = None;
        }
        pub fn stop(&mut self) {
            self.cycle.stop();
            self.restore();
        }
    }
    impl Drop for Desktop {
        fn drop(&mut self) {
            self.stop();
        }
    }
}
#[cfg(any(target_os = "linux", target_os = "windows"))]
pub(super) use desktop::Desktop;

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn attention_loops_then_settles_and_coalesces_bursts() {
        let now = Instant::now();
        let mut c = Cycle::default();
        assert!(c.begin(now, false, false));
        for i in 0..18 {
            assert_eq!(
                c.tick(
                    now + Duration::from_secs_f32((i as f32 + 0.25) * dock_icon::STEP_SECONDS),
                    false,
                    false
                ),
                Face::ALL[i % 6]
            );
        }
        assert!(!c.begin(now + Duration::from_millis(900), false, false));
        assert_eq!(
            c.tick(now + Duration::from_secs(2), false, false),
            Face::Newsreader
        );
        assert!(!c.active());
        assert!(c.begin(now + Duration::from_secs(3), false, false));
    }
    #[test]
    fn focus_and_reduced_motion_stop_and_prevent_attention() {
        let now = Instant::now();
        for (reduced, focused) in [(true, false), (false, true)] {
            let mut c = Cycle::default();
            assert!(!c.begin(now, reduced, focused));
            assert!(c.begin(now, false, false));
            assert_eq!(c.tick(now, reduced, focused), Face::Newsreader);
            assert!(!c.active());
        }
    }
}

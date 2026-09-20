//! Touch and pen: long-press is hover. A finger that lands moves the
//! pointer there, so everything that follows the pointer (lifted rows,
//! the tree's head, a toast going signal) follows the finger; held for a
//! beat without moving, it is a hover — the tooltip comes, nothing acts;
//! lifted quickly, it is a tap — a click at that point. A finger that
//! moves drags, as the mouse would with the button down. Once a touch
//! has been seen, hit targets grow toward 44px: the same rects, padded.
//! The mouse is untouched by any of this.

use std::time::{Duration, Instant};

use winit::event::{ElementState, MouseButton, TouchPhase};

use crate::app::App;

/// How long a press must hold to be a hover rather than a tap.
pub const HOLD: Duration = Duration::from_millis(350);
/// How far a finger may wander and still be a tap, in logical px.
const SLOP: f32 = 8.0;

#[derive(Default)]
pub struct Touch {
    /// The finger down now: where it landed, when, and whether it has
    /// moved past the slop (a drag) or held past the beat (a hover).
    pub down: Option<Press>,
    /// A touch has been seen: targets stay grown for the session.
    pub seen: bool,
}

pub struct Press {
    pub id: u64,
    pub at: Instant,
    pub start: (f32, f32),
    pub last: (f32, f32),
    pub dragging: bool,
    pub hovering: bool,
}

impl App {
    /// A touch event from the window.
    pub fn touch(&mut self, id: u64, phase: TouchPhase, x: f32, y: f32) {
        self.touch.seen = true;
        match phase {
            TouchPhase::Started => {
                // One finger at a time: a second finger while one is down is ignored.
                if self.touch.down.is_some() {
                    return;
                }
                self.mouse_moved(x, y);
                self.touch.down = Some(Press { id, at: crate::clock::now(), start: (x, y), last: (x, y), dragging: false, hovering: false });
            }
            TouchPhase::Moved => {
                let Some(p) = self.touch.down.as_mut() else { return };
                if p.id != id {
                    return;
                }
                p.last = (x, y);
                let far = ((x - p.start.0).powi(2) + (y - p.start.1).powi(2)).sqrt() > SLOP * self.scale;
                if far && !p.dragging && !p.hovering {
                    // A drag: the button goes down where the finger landed.
                    p.dragging = true;
                    let (sx, sy) = p.start;
                    self.mouse_moved(sx, sy);
                    self.mouse_button(MouseButton::Left, ElementState::Pressed);
                }
                if self.touch.down.as_ref().is_some_and(|p| p.dragging) {
                    self.mouse_moved(x, y);
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(p) = self.touch.down.take() else { return };
                if p.id != id {
                    self.touch.down = Some(p);
                    return;
                }
                self.mouse_moved(x, y);
                if p.dragging {
                    self.mouse_button(MouseButton::Left, ElementState::Released);
                } else if !p.hovering && phase == TouchPhase::Ended {
                    // A tap: a click where the finger lifted.
                    self.mouse_button(MouseButton::Left, ElementState::Pressed);
                    self.mouse_button(MouseButton::Left, ElementState::Released);
                }
                // The pointer does not stay under a lifted finger.
                if !p.hovering {
                    self.cursor_left();
                }
            }
        }
        self.dirty = true;
    }

    /// Once a loop: a press held past the beat becomes a hover — the
    /// pointer stays, the tooltip comes, nothing acts on lift.
    pub(crate) fn tend_touch(&mut self) {
        let Some(p) = self.touch.down.as_mut() else { return };
        if !p.dragging && !p.hovering && crate::clock::since(p.at) >= HOLD {
            p.hovering = true;
            let (x, y) = p.last;
            self.mouse_moved(x, y);
            self.dirty = true;
        }
    }

    /// How much a hit target grows once a touch has been seen: from the
    /// ~32px the mouse has to the 44px a finger needs, split both sides.
    pub(crate) fn touch_pad(&self) -> f32 {
        if self.touch.seen {
            self.px(6.0)
        } else {
            0.0
        }
    }
}

/// A rect grown by the touch pad, for hit-testing.
pub fn grown(r: nus_render::Rect, pad: f32) -> nus_render::Rect {
    nus_render::Rect::new(r.x - pad, r.y - pad, r.w + 2.0 * pad, r.h + 2.0 * pad)
}

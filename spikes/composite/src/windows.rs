//! nus windows are OS windows, all in one process: the host keeps one
//! `App` per window and hands every app the list of the others so the
//! sidebar's rail and window list are real. Fronting and opening go
//! through requests the host answers.

/// One window, as the others see it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// The winit window id, as a number.
    pub id: u64,
    pub name: String,
    pub tabs: usize,
    /// Creation order: the first window is 0.
    pub ordinal: usize,
    /// The window's container colour, for its square.
    pub colour: nus_render::Color,
}

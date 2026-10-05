//! A page's ready sound follows its load, including loads between app ticks.
//! Session restoration is quiet until its first load ends; later navigation
//! uses the usual cue, even while other restored pages are still loading.

use std::time::{Duration, Instant};

#[derive(Default)]
pub(crate) struct PageLoadCue {
    quiet_restore: bool,
    since: Option<Instant>,
    pending: bool,
}

impl PageLoadCue {
    pub(crate) fn restored() -> Self {
        Self {
            quiet_restore: true,
            ..Self::default()
        }
    }

    /// An explicit navigation or reload takes over from the restored load.
    pub(crate) fn user_navigation(&mut self) {
        self.quiet_restore = false;
        self.since = None;
    }

    pub(crate) fn observe(&mut self, loading: bool, now: Instant) {
        if loading {
            self.since.get_or_insert(now);
        } else if let Some(since) = self.since.take() {
            let quiet = std::mem::take(&mut self.quiet_restore);
            self.pending |= !quiet && now.saturating_duration_since(since) > Duration::from_secs(1);
        }
    }

    pub(crate) fn take(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staggered_restored_tabs_and_split_panes_stay_quiet() {
        let now = Instant::now();
        for seconds in [2, 3, 6, 12, 60] {
            let mut cue = PageLoadCue::restored();
            cue.observe(true, now);
            cue.observe(false, now + Duration::from_secs(seconds));
            assert!(
                !cue.take(),
                "a restored page taking {seconds}s made a sound"
            );
        }
    }

    #[test]
    fn a_new_page_is_audible_while_other_pages_restore() {
        let now = Instant::now();
        let mut restored = PageLoadCue::restored();
        let mut opened = PageLoadCue::default();
        restored.observe(true, now);
        opened.observe(true, now);
        opened.observe(false, now + Duration::from_secs(2));
        assert!(opened.take());
        restored.observe(false, now + Duration::from_secs(30));
        assert!(!restored.take());
    }

    #[test]
    fn redirects_belong_to_the_same_quiet_restore() {
        let now = Instant::now();
        let mut cue = PageLoadCue::restored();
        cue.observe(true, now);
        cue.observe(true, now + Duration::from_secs(2));
        cue.observe(true, now + Duration::from_secs(4));
        cue.observe(false, now + Duration::from_secs(6));
        assert!(!cue.take());
        cue.observe(true, now + Duration::from_secs(7));
        cue.observe(false, now + Duration::from_secs(9));
        assert!(
            cue.take(),
            "later navigation must still have its ready sound"
        );
    }

    #[test]
    fn a_fast_restore_between_ticks_does_not_mute_the_next_load() {
        let now = Instant::now();
        let mut cue = PageLoadCue::restored();
        cue.observe(true, now);
        cue.observe(false, now + Duration::from_millis(10));
        cue.observe(false, now + Duration::from_millis(20));
        cue.observe(true, now + Duration::from_secs(1));
        cue.observe(false, now + Duration::from_secs(3));
        assert!(cue.take());
        assert!(!cue.take(), "a completed load sounds only once");
    }

    #[test]
    fn an_explicit_reload_or_navigation_can_interrupt_restoration() {
        let now = Instant::now();
        let mut cue = PageLoadCue::restored();
        cue.observe(false, now); // Browser creation has not started the load yet.
        cue.observe(true, now);
        cue.user_navigation();
        cue.observe(true, now + Duration::from_secs(2));
        cue.observe(false, now + Duration::from_secs(4));
        assert!(cue.take());
    }

    #[test]
    fn short_loads_and_duplicate_completion_callbacks_stay_quiet() {
        let now = Instant::now();
        let mut cue = PageLoadCue::default();
        cue.observe(true, now);
        cue.observe(false, now + Duration::from_secs(1));
        assert!(!cue.take());
        cue.observe(false, now + Duration::from_secs(3));
        assert!(!cue.take());
        cue.observe(true, now + Duration::from_secs(4));
        cue.observe(false, now + Duration::from_secs(6));
        assert!(cue.take());
        cue.observe(false, now + Duration::from_secs(7));
        assert!(!cue.take());
    }
}

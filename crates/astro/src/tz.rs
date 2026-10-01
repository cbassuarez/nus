//! The machine's offset from UTC, so an almanac speaks in the clock on the wall.

/// Minutes east of UTC right now (daylight saving included), or 0 if the
/// machine will not say. One reading stands for the whole sky: a sunset a
/// week away across a clock change is rare enough to ignore.
pub fn local_offset_minutes() -> i32 {
    imp::offset()
}

#[cfg(windows)]
mod imp {
    pub fn offset() -> i32 {
        use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
        // SAFETY: a plain out-parameter struct the call fills in.
        unsafe {
            let mut info: TIME_ZONE_INFORMATION = std::mem::zeroed();
            let id = GetTimeZoneInformation(&mut info);
            // Bias is minutes WEST of UTC; daylight time adds its own bias.
            let extra = match id {
                1 => info.StandardBias,
                2 => info.DaylightBias,
                _ => 0,
            };
            -(info.Bias + extra)
        }
    }
}

#[cfg(unix)]
mod imp {
    pub fn offset() -> i32 {
        // SAFETY: localtime_r writes only into the tm we hand it.
        unsafe {
            let now = libc::time(std::ptr::null_mut());
            let mut tm: libc::tm = std::mem::zeroed();
            if libc::localtime_r(&now, &mut tm).is_null() {
                return 0;
            }
            (tm.tm_gmtoff / 60) as i32
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod imp {
    pub fn offset() -> i32 {
        0
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn offset_is_a_real_zone() {
        let m = super::local_offset_minutes();
        assert!((-14 * 60..=14 * 60).contains(&m), "{m}");
    }
}

//! Wall-clock formatting, via libc rather than a date crate.

/// A broken-down local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// 0 = Sunday.
    pub weekday: u32,
}

impl DateTime {
    /// The current local time.
    pub fn now() -> Self {
        // SAFETY: time(NULL) only reads the clock.
        let epoch = unsafe { libc::time(std::ptr::null_mut()) };
        Self::from_epoch(epoch)
    }

    /// Converts a Unix timestamp to local time.
    pub fn from_epoch(epoch: libc::time_t) -> Self {
        let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
        // SAFETY: localtime_r writes into the buffer we own and returns null
        // on failure, which is the only case we have to guard.
        let ok = unsafe { !libc::localtime_r(&epoch, tm.as_mut_ptr()).is_null() };
        if !ok {
            return Self {
                year: 1970,
                month: 1,
                day: 1,
                hour: 0,
                minute: 0,
                second: 0,
                weekday: 4,
            };
        }
        let tm = unsafe { tm.assume_init() };

        Self {
            year: tm.tm_year + 1900,
            month: (tm.tm_mon + 1) as u32,
            day: tm.tm_mday as u32,
            hour: tm.tm_hour as u32,
            minute: tm.tm_min as u32,
            second: tm.tm_sec as u32,
            weekday: tm.tm_wday as u32,
        }
    }

    /// `2026-08-17 14:05:31`.
    pub fn iso(&self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
            self.year, self.month, self.day, self.hour, self.minute, self.second
        )
    }

    /// `14:05:31`.
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }

    /// `Mon 17 Aug 2026, 14:05`, the form the info row uses.
    pub fn pretty(&self) -> String {
        const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];

        let day = DAYS.get(self.weekday as usize).copied().unwrap_or("???");
        let month = MONTHS
            .get(self.month.saturating_sub(1) as usize)
            .copied()
            .unwrap_or("???");
        format!(
            "{day} {:02} {month} {}, {:02}:{:02}",
            self.day, self.year, self.hour, self.minute
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_a_known_timestamp() {
        // 2000-01-01 00:00:00 UTC. The local rendering depends on the machine's
        // zone, so only the shape is asserted here.
        let dt = DateTime::from_epoch(946_684_800);
        assert!(dt.year == 1999 || dt.year == 2000);
        assert_eq!(dt.iso().len(), 19);
        assert_eq!(dt.clock().len(), 8);
    }

    #[test]
    fn fields_stay_in_range() {
        let now = DateTime::now();
        assert!(now.year >= 2020, "the clock should be roughly correct");
        assert!((1..=12).contains(&now.month));
        assert!((1..=31).contains(&now.day));
        assert!(now.hour < 24 && now.minute < 60 && now.second <= 60);
        assert!(now.weekday < 7);
    }

    #[test]
    fn pretty_names_the_day_and_month() {
        let dt = DateTime {
            year: 2026,
            month: 8,
            day: 17,
            hour: 14,
            minute: 5,
            second: 31,
            weekday: 1,
        };
        assert_eq!(dt.pretty(), "Mon 17 Aug 2026, 14:05");
        assert_eq!(dt.iso(), "2026-08-17 14:05:31");
        assert_eq!(dt.clock(), "14:05:31");
    }

    #[test]
    fn out_of_range_fields_do_not_panic() {
        let dt = DateTime {
            year: 2026,
            month: 99,
            day: 1,
            hour: 0,
            minute: 0,
            second: 0,
            weekday: 99,
        };
        assert!(dt.pretty().contains("???"));
    }
}

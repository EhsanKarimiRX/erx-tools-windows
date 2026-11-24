//! A clock for Secure Notes: the Unix time, the local calendar date and a
//! monotonic `Instant` for autosave and idle timing.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::sync::atomic::{AtomicI64, Ordering};
#[cfg(test)]
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::time::Duration;

use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};

/// Seconds between 1601-01-01 (the FILETIME epoch) and 1970-01-01.
const FILETIME_UNIX_EPOCH_SECONDS: i64 = 11_644_473_600;
/// 100-nanosecond units in one second.
const FILETIME_TICKS_PER_SECOND: i64 = 10_000_000;

/// A calendar date and a time of day, without a time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Days since 1970-01-01 of a civil date (standard civil algorithm).
#[allow(dead_code)] // used by tests only
pub(crate) fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year);
    let month = i64::from(month);
    let day = i64::from(day);
    let shifted_year = if month <= 2 { year - 1 } else { year };
    let era = if shifted_year >= 0 {
        shifted_year
    } else {
        shifted_year - 399
    } / 400;
    let year_of_era = shifted_year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date of a number of days since 1970-01-01.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year as i32, month as u32, day as u32)
}

/// UTC calendar date and time of a Unix timestamp. Works for negative values.
pub fn utc_parts(unix: i64) -> DateParts {
    let days = unix.div_euclid(86_400);
    let second_of_day = unix.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    DateParts {
        year,
        month,
        day,
        hour: (second_of_day / 3_600) as u32,
        minute: ((second_of_day % 3_600) / 60) as u32,
    }
}

/// Windows local calendar date and time of a Unix timestamp.
pub fn local_parts(unix: i64) -> Option<DateParts> {
    let ticks = unix
        .checked_add(FILETIME_UNIX_EPOCH_SECONDS)?
        .checked_mul(FILETIME_TICKS_PER_SECOND)?;
    if ticks < 0 {
        return None;
    }
    let ticks = ticks as u64;
    let file_time = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut utc = SYSTEMTIME::default();
    // SAFETY: both pointers refer to live, correctly sized local values and
    // `file_time` is a valid FILETIME for the call.
    let converted = unsafe { FileTimeToSystemTime(&file_time, &mut utc) };
    if converted == 0 {
        return None;
    }
    let mut local = SYSTEMTIME::default();
    // SAFETY: null asks for the current time zone (documented); the two
    // SYSTEMTIME pointers are live, correctly sized and distinct.
    let converted = unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) };
    if converted == 0 {
        return None;
    }
    Some(DateParts {
        year: i32::from(local.wYear),
        month: u32::from(local.wMonth),
        day: u32::from(local.wDay),
        hour: u32::from(local.wHour),
        minute: u32::from(local.wMinute),
    })
}

/// Time sources used by the notes session. Tests replace them.
pub trait Clock: Send + Sync {
    /// Monotonic time for autosave and idle timing.
    fn now(&self) -> Instant;
    /// Seconds since the Unix epoch.
    fn unix(&self) -> i64;
    /// Local calendar date and time of a Unix timestamp.
    fn local_parts(&self, unix: i64) -> Option<DateParts>;
}

/// The real clock.
pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn unix(&self) -> i64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(since_epoch) => i64::try_from(since_epoch.as_secs()).unwrap_or(i64::MAX),
            Err(_) => 0,
        }
    }

    fn local_parts(&self, unix: i64) -> Option<DateParts> {
        local_parts(unix)
    }
}

/// A clock that tests move by hand.
#[cfg(test)]
pub struct TestClock {
    base: Instant,
    offset: Mutex<Duration>,
    unix: AtomicI64,
    parts: Mutex<Option<DateParts>>,
}

#[cfg(test)]
impl TestClock {
    pub fn new(unix: i64, parts: Option<DateParts>) -> Arc<TestClock> {
        Arc::new(TestClock {
            base: Instant::now(),
            offset: Mutex::new(Duration::ZERO),
            unix: AtomicI64::new(unix),
            parts: Mutex::new(parts),
        })
    }

    /// Moves the monotonic time and the Unix time forward by `by`.
    pub fn advance(&self, by: Duration) {
        {
            let mut offset = self.offset.lock().unwrap();
            *offset += by;
        }
        self.unix.fetch_add(by.as_secs() as i64, Ordering::SeqCst);
    }

    /// Sets the Unix time without moving the monotonic time.
    pub fn set_unix(&self, unix: i64) {
        self.unix.store(unix, Ordering::SeqCst);
    }
}

#[cfg(test)]
impl Clock for TestClock {
    fn now(&self) -> Instant {
        self.base + *self.offset.lock().unwrap()
    }

    fn unix(&self) -> i64 {
        self.unix.load(Ordering::SeqCst)
    }

    fn local_parts(&self, _unix: i64) -> Option<DateParts> {
        *self.parts.lock().unwrap()
    }
}

#[cfg(test)]
impl Clock for Arc<TestClock> {
    fn now(&self) -> Instant {
        (**self).now()
    }

    fn unix(&self) -> i64 {
        (**self).unix()
    }

    fn local_parts(&self, unix: i64) -> Option<DateParts> {
        (**self).local_parts(unix)
    }
}

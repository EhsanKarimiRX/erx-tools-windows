//! Clock tests: civil dates, UTC, Windows local time and the real clock.

use super::clock::{
    Clock, DateParts, RealClock, TestClock, days_from_civil, local_parts, utc_parts,
};

fn parts(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> DateParts {
    DateParts {
        year,
        month,
        day,
        hour,
        minute,
    }
}

#[test]
fn k1_utc_parts() {
    assert_eq!(utc_parts(0), parts(1970, 1, 1, 0, 0));
    assert_eq!(utc_parts(86_400), parts(1970, 1, 2, 0, 0));
    assert_eq!(utc_parts(1_700_000_000), parts(2023, 11, 14, 22, 13));
    assert_eq!(utc_parts(1_709_164_800), parts(2024, 2, 29, 0, 0));
    assert_eq!(utc_parts(-1), parts(1969, 12, 31, 23, 59));
}

#[test]
fn k2_days_from_civil() {
    assert_eq!(days_from_civil(1970, 1, 1), 0);
    assert_eq!(days_from_civil(1970, 1, 2), 1);
    assert_eq!(
        days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28),
        2
    );
    assert_eq!(
        days_from_civil(2023, 11, 14),
        1_700_000_000_i64.div_euclid(86_400)
    );
}

#[test]
fn k3_local_parts_sanity() {
    let now = RealClock.unix();
    for unix in [1_700_000_000_i64, now] {
        let local = local_parts(unix).expect("Windows local time");
        let utc = utc_parts(unix);
        let day_minutes = (days_from_civil(local.year, local.month, local.day)
            - days_from_civil(utc.year, utc.month, utc.day))
            * 24
            * 60;
        let minute_of_day =
            |value: &DateParts| i64::from(value.hour) * 60 + i64::from(value.minute);
        let difference = day_minutes + minute_of_day(&local) - minute_of_day(&utc);
        assert!(difference.abs() <= 14 * 60, "time zone offset out of range");
        assert_eq!(difference.rem_euclid(15), 0);
    }

    // A test clock hands back exactly what it was given, or nothing.
    let stored = parts(2023, 11, 14, 22, 13);
    let clock = TestClock::new(1_700_000_000, Some(stored));
    assert_eq!(clock.local_parts(0), Some(stored));
    assert_eq!(clock.local_parts(123_456_789), Some(stored));
    assert_eq!(clock.unix(), 1_700_000_000);
    clock.advance(std::time::Duration::from_secs(7));
    assert_eq!(clock.unix(), 1_700_000_007);
    let empty = TestClock::new(5, None);
    assert_eq!(empty.local_parts(0), None);
}

#[test]
fn k4_real_clock() {
    let clock = RealClock;
    assert!(clock.unix() > 1_700_000_000);
    let first = clock.now();
    let second = clock.now();
    assert!(second >= first);
}

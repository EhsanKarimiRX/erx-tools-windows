//! Pure Awake logic: the 6-key power model, the restore plan (spec rule R3),
//! the reconcile decision (R4) and the timer check (R6). No Windows API and
//! no file I/O here — those live in `power.rs` and `state_file.rs`, so every
//! decision in this module is unit-testable with the in-memory fake.
//!
//! WHY the power-plan approach (owner decision, keep this reasoning with the
//! code): the owner runs Awake during long, expensive computer runs. With the
//! "execution state" API, a crash of ERX Tools returns Windows to normal
//! rules immediately, the PC may sleep, and the run is lost — unrecoverable.
//! With the power-plan approach a crash simply leaves the PC awake; at worst
//! the screen stays on until one setting is changed back — cheap and safe.
//! Therefore: never replace this with SetThreadExecutionState, and no helper
//! processes (see docs/PROMPT_5_DECISIONS.md).

use serde::{Deserialize, Serialize};

/// The six power values Awake may change: three settings × two power
/// sources (AC = plugged in, DC = on battery).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PowerKey {
    SleepAc,
    SleepDc,
    HibernateAc,
    HibernateDc,
    DisplayAc,
    DisplayDc,
}

impl PowerKey {
    /// Fixed iteration order used for reads, writes and plans.
    pub const ALL: [PowerKey; 6] = [
        PowerKey::SleepAc,
        PowerKey::SleepDc,
        PowerKey::HibernateAc,
        PowerKey::HibernateDc,
        PowerKey::DisplayAc,
        PowerKey::DisplayDc,
    ];

    /// Only the display keys; the other four are sleep/hibernate.
    pub const DISPLAY: [PowerKey; 2] = [PowerKey::DisplayAc, PowerKey::DisplayDc];

    pub fn is_display(self) -> bool {
        matches!(self, PowerKey::DisplayAc | PowerKey::DisplayDc)
    }

    /// Key name as stored in the state file (also the state-file JSON field).
    pub fn as_str(self) -> &'static str {
        match self {
            PowerKey::SleepAc => "sleep_ac",
            PowerKey::SleepDc => "sleep_dc",
            PowerKey::HibernateAc => "hibernate_ac",
            PowerKey::HibernateDc => "hibernate_dc",
            PowerKey::DisplayAc => "display_ac",
            PowerKey::DisplayDc => "display_dc",
        }
    }

    /// Plain-English name for reports (which setting, which power source).
    pub fn label(self) -> &'static str {
        match self {
            PowerKey::SleepAc => "sleep after (plugged in)",
            PowerKey::SleepDc => "sleep after (on battery)",
            PowerKey::HibernateAc => "hibernate after (plugged in)",
            PowerKey::HibernateDc => "hibernate after (on battery)",
            PowerKey::DisplayAc => "turn off display after (plugged in)",
            PowerKey::DisplayDc => "turn off display after (on battery)",
        }
    }
}

/// The six values as a small typed map; every field is `None` when the value
/// is unknown, unreadable, or intentionally untouched.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerValues {
    pub sleep_ac: Option<u32>,
    pub sleep_dc: Option<u32>,
    pub hibernate_ac: Option<u32>,
    pub hibernate_dc: Option<u32>,
    pub display_ac: Option<u32>,
    pub display_dc: Option<u32>,
}

impl PowerValues {
    pub fn get(&self, key: PowerKey) -> Option<u32> {
        match key {
            PowerKey::SleepAc => self.sleep_ac,
            PowerKey::SleepDc => self.sleep_dc,
            PowerKey::HibernateAc => self.hibernate_ac,
            PowerKey::HibernateDc => self.hibernate_dc,
            PowerKey::DisplayAc => self.display_ac,
            PowerKey::DisplayDc => self.display_dc,
        }
    }

    pub fn set(&mut self, key: PowerKey, value: Option<u32>) {
        match key {
            PowerKey::SleepAc => self.sleep_ac = value,
            PowerKey::SleepDc => self.sleep_dc = value,
            PowerKey::HibernateAc => self.hibernate_ac = value,
            PowerKey::HibernateDc => self.hibernate_dc = value,
            PowerKey::DisplayAc => self.display_ac = value,
            PowerKey::DisplayDc => self.display_dc = value,
        }
    }
}

/// What one key should do during a restore (R3): restore only if Windows
/// still holds the value we set (the user did not change it by hand).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreAction {
    /// `current == applied`: write the original value back.
    Restore { key: PowerKey, original: u32 },
    /// `current != applied`: the user changed it — keep their change.
    KeepManual { key: PowerKey },
    /// `applied` was null: Awake never touched this key.
    Untouched { key: PowerKey },
}

/// R3 as a pure function: per-key restore plan for the stored scheme.
pub fn restore_plan(
    originals: &PowerValues,
    applied: &PowerValues,
    current: &PowerValues,
) -> Vec<RestoreAction> {
    PowerKey::ALL
        .iter()
        .copied()
        .map(|key| match applied.get(key) {
            None => RestoreAction::Untouched { key },
            Some(applied_value) => {
                if current.get(key) == Some(applied_value) {
                    match originals.get(key) {
                        Some(original) => RestoreAction::Restore { key, original },
                        // By construction originals is Some whenever applied is
                        // Some; if a hand-edited state file breaks that, the
                        // safe behavior is to leave the key alone.
                        None => RestoreAction::Untouched { key },
                    }
                } else {
                    RestoreAction::KeepManual { key }
                }
            }
        })
        .collect()
}

/// What `reconcile` found when comparing the state file with reality (R4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconcileDecision {
    /// The state file matches reality; Awake is genuinely still on.
    StillOn,
    /// Something differs: a value was changed by hand, or a different power
    /// scheme is active (while the stored one still exists).
    Changed,
    /// The stored power scheme no longer exists.
    SchemeGone,
}

/// R4 as a pure function. `active_scheme` is the currently active scheme GUID
/// (lowercase, hyphenated), `stored_scheme_exists` says whether the scheme the
/// state file refers to still exists.
pub fn reconcile_decision(
    state_scheme_guid: &str,
    applied: &PowerValues,
    current_values: &PowerValues,
    active_scheme: &str,
    stored_scheme_exists: bool,
) -> ReconcileDecision {
    if !stored_scheme_exists {
        return ReconcileDecision::SchemeGone;
    }
    if !active_scheme.eq_ignore_ascii_case(state_scheme_guid) {
        return ReconcileDecision::Changed;
    }
    for key in PowerKey::ALL {
        if let Some(applied_value) = applied.get(key)
            && current_values.get(key) != Some(applied_value)
        {
            return ReconcileDecision::Changed;
        }
    }
    ReconcileDecision::StillOn
}

/// R6: true when the optional auto-off deadline has passed. The deadline is
/// inclusive — at exactly `ends_at_unix` the timer has finished.
pub fn timer_expired(now_unix: i64, ends_at_unix: Option<i64>) -> bool {
    match ends_at_unix {
        Some(ends_at) => now_unix >= ends_at,
        None => false,
    }
}

/// Why Awake switched itself off (shown to the user with an honest message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoOffReason {
    /// The user changed a timeout (or the active scheme) in Windows.
    ManualChange,
    /// The stored power scheme no longer exists.
    SchemeGone,
    /// The auto-off timer ran out while ERX Tools was open.
    TimerExpired,
    /// The auto-off timer ran out while ERX Tools was closed.
    TimerExpiredWhileClosed,
}

/// The persisted Awake state (state file `awake_state.json`, version 1).
/// `None` always means "unknown / never touch this key".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AwakeStateFile {
    pub version: u32,
    /// GUID (lowercase, hyphenated) of the scheme Awake changed.
    pub scheme_guid: String,
    pub started_at_unix: i64,
    pub ends_at_unix: Option<i64>,
    /// true = the display was also set to Never (the default).
    pub keep_screen_on: bool,
    /// Original values read before any change.
    pub originals: PowerValues,
    /// The values Awake actually set (null = untouched).
    pub applied: PowerValues,
}

/// The options a user can set in the Awake drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwakeOptions {
    /// true = the display is also kept on (default).
    pub keep_screen_on: bool,
    /// Auto-off deadline; None = stay on until turned off by hand.
    pub ends_at_unix: Option<i64>,
}

impl Default for AwakeOptions {
    fn default() -> Self {
        AwakeOptions {
            keep_screen_on: true,
            ends_at_unix: None,
        }
    }
}

/// Status as reported to the frontend. The UI toggle shows ON only when
/// `state` is `"on"` — it is always derived from the backend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AwakeStatus {
    /// "off" | "on"
    pub state: &'static str,
    pub started_at_unix: Option<i64>,
    pub ends_at_unix: Option<i64>,
    /// true = the display is also set to Never.
    pub keep_screen_on: bool,
    /// true = the app found a still-valid Awake from an earlier session.
    pub resumed_from_previous_session: bool,
}

/// Honest result of a restore: how many keys were written back, how many of
/// the user's manual changes were kept, what failed (plain English), and
/// whether the stored scheme no longer exists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: u32,
    pub kept: u32,
    /// One plain-English sentence per key that could not be restored.
    pub failed: Vec<String>,
    /// true = the stored power scheme is gone; nothing could be restored.
    pub scheme_gone: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(PowerKey, u32)]) -> PowerValues {
        let mut values = PowerValues::default();
        for &(key, value) in pairs {
            values.set(key, Some(value));
        }
        values
    }

    #[test]
    fn power_key_names_are_unique_state_file_fields() {
        let mut names: Vec<&str> = PowerKey::ALL.iter().map(|key| key.as_str()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), count, "six distinct state-file key names");
        assert!(
            names
                .iter()
                .all(|name| name.ends_with("_ac") || name.ends_with("_dc"))
        );
    }

    #[test]
    fn restore_restores_untouched_keys() {
        let originals = values(&[(PowerKey::SleepAc, 1800), (PowerKey::SleepDc, 900)]);
        let applied = values(&[(PowerKey::SleepAc, 0), (PowerKey::SleepDc, 0)]);
        let current = values(&[(PowerKey::SleepAc, 0), (PowerKey::SleepDc, 0)]);
        let plan = restore_plan(&originals, &applied, &current);
        assert_eq!(
            plan,
            vec![
                RestoreAction::Restore {
                    key: PowerKey::SleepAc,
                    original: 1800
                },
                RestoreAction::Restore {
                    key: PowerKey::SleepDc,
                    original: 900
                },
                RestoreAction::Untouched {
                    key: PowerKey::HibernateAc
                },
                RestoreAction::Untouched {
                    key: PowerKey::HibernateDc
                },
                RestoreAction::Untouched {
                    key: PowerKey::DisplayAc
                },
                RestoreAction::Untouched {
                    key: PowerKey::DisplayDc
                },
            ]
        );
    }

    #[test]
    fn restore_keeps_manually_changed_keys() {
        let originals = values(&[(PowerKey::SleepAc, 1800)]);
        let applied = values(&[(PowerKey::SleepAc, 0)]);
        // The user changed sleep to 20 minutes (1200 s) by hand.
        let current = values(&[(PowerKey::SleepAc, 1200)]);
        let plan = restore_plan(&originals, &applied, &current);
        assert_eq!(
            plan,
            PowerKey::ALL
                .iter()
                .map(|&key| if key == PowerKey::SleepAc {
                    RestoreAction::KeepManual { key }
                } else {
                    RestoreAction::Untouched { key }
                })
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn restore_skips_null_keys() {
        // applied has no entries at all: nothing is ever touched.
        let plan = restore_plan(
            &PowerValues::default(),
            &PowerValues::default(),
            &PowerValues::default(),
        );
        assert!(
            plan.iter()
                .all(|action| matches!(action, RestoreAction::Untouched { .. }))
        );

        // A null original with a null applied stays untouched too (the safe
        // fallback for a hand-edited state file).
        let applied = values(&[(PowerKey::DisplayAc, 0)]);
        let current = values(&[(PowerKey::DisplayAc, 0)]);
        let plan = restore_plan(&PowerValues::default(), &applied, &current);
        assert!(
            plan.iter()
                .all(|action| matches!(action, RestoreAction::Untouched { .. }))
        );
    }

    #[test]
    fn reconcile_still_on() {
        let applied = values(&[(PowerKey::SleepAc, 0), (PowerKey::SleepDc, 0)]);
        let current = values(&[(PowerKey::SleepAc, 0), (PowerKey::SleepDc, 0)]);
        let decision = reconcile_decision(
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            &applied,
            &current,
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            true,
        );
        assert_eq!(decision, ReconcileDecision::StillOn);
    }

    #[test]
    fn reconcile_value_changed_is_changed() {
        let applied = values(&[(PowerKey::SleepAc, 0)]);
        // The user set sleep back to 20 minutes.
        let current = values(&[(PowerKey::SleepAc, 1200)]);
        let decision = reconcile_decision(
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            &applied,
            &current,
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            true,
        );
        assert_eq!(decision, ReconcileDecision::Changed);
    }

    #[test]
    fn reconcile_different_active_scheme_is_changed() {
        let decision = reconcile_decision(
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            &PowerValues::default(),
            &PowerValues::default(),
            "99999999-1111-2222-3333-444444444444",
            true,
        );
        assert_eq!(decision, ReconcileDecision::Changed);
    }

    #[test]
    fn reconcile_scheme_gone_takes_priority() {
        let decision = reconcile_decision(
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            &PowerValues::default(),
            &PowerValues::default(),
            "99999999-1111-2222-3333-444444444444",
            false,
        );
        assert_eq!(decision, ReconcileDecision::SchemeGone);
    }

    #[test]
    fn reconcile_ignores_null_applied_keys() {
        // applied is all null: whatever the current values are, still on.
        let current = values(&[(PowerKey::SleepAc, 12345)]);
        let decision = reconcile_decision(
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            &PowerValues::default(),
            &current,
            "381b4222-f694-41f0-9685-ff5bb260df2e",
            true,
        );
        assert_eq!(decision, ReconcileDecision::StillOn);
    }

    #[test]
    fn timer_expiry_boundary() {
        assert!(!timer_expired(999, Some(1000)));
        assert!(timer_expired(1000, Some(1000)));
        assert!(timer_expired(1001, Some(1000)));
        assert!(!timer_expired(100_000, None));
    }
}

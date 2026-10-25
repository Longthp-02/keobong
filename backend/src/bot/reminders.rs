//! Reminders the bot posts to a chat on a schedule ("@Bot DaGhep nhắc mỗi thứ 5
//! 20h: cuối tuần ai đá không?"). Pure rules: no Axum, sqlx, HTTP or SDK types.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};

use crate::auth::UserId;
use crate::matches::domain::{local_date, local_instant};

/// Reminders one chat may have at a time (approved by Long 2026-10-25).
pub const MAX_REMINDERS_PER_CHAT: i64 = 3;
/// Longest reminder text, in characters.
pub const MAX_REMINDER_CHARS: usize = 200;
/// Days of the month a monthly reminder may use; every month has them.
pub const MONTH_DAYS: std::ops::RangeInclusive<i16> = 1..=28;
/// Failed sends in a row after which a reminder is dropped (the bot was
/// probably removed from the group).
pub const MAX_SEND_FAILURES: i16 = 3;
/// Reminders posted per scheduler run; later ones wait for the next run. Small
/// enough that a run (10 s Zalo timeout each) ends within the scheduler's deadline.
pub const REMINDERS_PER_RUN: i64 = 20;
/// A reminder more than this late (the scheduler or the API was down) is
/// skipped rather than posted at an odd hour.
pub const MAX_LATE_MINUTES: i64 = 60;

/// When a reminder repeats, in Ho Chi Minh City time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    /// Bit `n` is set for weekday `n`, counted from Sunday = 0. All seven bits
    /// set means every day.
    Weekdays(u8),
    /// The same day every month, 1–28.
    MonthDay(i16),
}

pub const EVERY_DAY: u8 = 0b111_1111;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Schedule {
    pub repeat: Repeat,
    pub time: NaiveTime,
}

impl Schedule {
    fn matches(&self, day: NaiveDate) -> bool {
        match self.repeat {
            Repeat::Weekdays(days) => days & (1 << day.weekday().num_days_from_sunday()) != 0,
            Repeat::MonthDay(month_day) => i64::from(day.day()) == i64::from(month_day),
        }
    }

    /// The first time strictly after `after` when the reminder is due.
    pub fn next_run(&self, after: DateTime<Utc>) -> DateTime<Utc> {
        let mut day = local_date(after);
        // A monthly day comes within 31 days; the loop bound is only a guard.
        for _ in 0..=62 {
            if self.matches(day) {
                let at = local_instant(day, self.time);
                if at > after {
                    return at;
                }
            }
            day = day.succ_opt().unwrap_or(day);
        }
        after + Duration::days(1)
    }
}

/// What the reader made of a reminder request, before any checks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawReminder {
    /// "daily", "weekly" or "monthly".
    pub repeat: Option<String>,
    /// Weekdays for "weekly", Sunday = 0 … Saturday = 6.
    pub weekdays: Vec<i64>,
    pub day_of_month: Option<i64>,
    /// HH:MM, 24-hour.
    pub time: Option<String>,
    /// The message to post, in Vietnamese.
    pub message: Option<String>,
}

/// A reminder that passed the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReminderPlan {
    pub schedule: Schedule,
    pub text: String,
}

/// What a request left out or got wrong, in the order the reply lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReminderGap {
    Repeat,
    Time,
    Message,
}

impl ReminderGap {
    pub fn key(self) -> &'static str {
        match self {
            Self::Repeat => "repeat",
            Self::Time => "time",
            Self::Message => "message",
        }
    }
}

impl RawReminder {
    /// Checks every field; lists what is missing or unusable.
    pub fn checked(self) -> Result<ReminderPlan, Vec<ReminderGap>> {
        let repeat = match self.repeat.as_deref() {
            Some("daily") => Some(Repeat::Weekdays(EVERY_DAY)),
            Some("weekly") => {
                let days = self
                    .weekdays
                    .iter()
                    .filter(|day| (0..7).contains(*day))
                    .fold(0u8, |days, day| days | (1 << day));
                (days != 0 && self.weekdays.iter().all(|day| (0..7).contains(day)))
                    .then_some(Repeat::Weekdays(days))
            }
            Some("monthly") => self
                .day_of_month
                .and_then(|day| i16::try_from(day).ok())
                .filter(|day| MONTH_DAYS.contains(day))
                .map(Repeat::MonthDay),
            _ => None,
        };
        let time = self
            .time
            .as_deref()
            .and_then(|time| NaiveTime::parse_from_str(time, "%H:%M").ok());
        let text = self
            .message
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty() && text.chars().count() <= MAX_REMINDER_CHARS);
        match (repeat, time, text) {
            (Some(repeat), Some(time), Some(text)) => Ok(ReminderPlan {
                schedule: Schedule { repeat, time },
                text,
            }),
            (repeat, time, text) => Err([
                (repeat.is_none(), ReminderGap::Repeat),
                (time.is_none(), ReminderGap::Time),
                (text.is_none(), ReminderGap::Message),
            ]
            .into_iter()
            .filter_map(|(gap, kind)| gap.then_some(kind))
            .collect()),
        }
    }
}

/// What the reminder reader is asked to read.
pub struct ReminderRequest<'a> {
    pub text: &'a str,
    pub today: NaiveDate,
}

#[derive(Debug, thiserror::Error)]
#[error("reminder reader failed: {0}")]
pub struct ReminderReaderError(pub String);

pub type ReminderFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RawReminder, ReminderReaderError>> + Send + 'a>>;

/// Port: turns a reminder request into a schedule and a message (an AI model in
/// production).
pub trait ReminderReader: Send + Sync {
    fn read<'a>(&'a self, request: ReminderRequest<'a>) -> ReminderFuture<'a>;
}

/// A reminder waiting for its author's "ok" (30 minutes, like match drafts).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReminder {
    pub zalo_user_id: String,
    pub chat_id: String,
    pub user: UserId,
    pub plan: ReminderPlan,
    pub expires_at: DateTime<Utc>,
}

/// A saved reminder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reminder {
    pub id: i64,
    pub chat_id: String,
    pub plan: ReminderPlan,
    pub next_run_at: DateTime<Utc>,
}

/// Stored form of a repeat: the weekday bits (0 for monthly) and the day of month.
pub fn repeat_columns(repeat: Repeat) -> (i16, Option<i16>) {
    match repeat {
        Repeat::Weekdays(days) => (i16::from(days), None),
        Repeat::MonthDay(day) => (0, Some(day)),
    }
}

pub fn repeat_from_columns(weekdays: i16, day_of_month: Option<i16>) -> Option<Repeat> {
    match (weekdays, day_of_month) {
        (0, Some(day)) if MONTH_DAYS.contains(&day) => Some(Repeat::MonthDay(day)),
        (days, None) if (1..=i16::from(EVERY_DAY)).contains(&days) => {
            u8::try_from(days).ok().map(Repeat::Weekdays)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().unwrap()
    }

    fn time(hm: &str) -> NaiveTime {
        NaiveTime::parse_from_str(hm, "%H:%M").unwrap()
    }

    // 2099-10-01 is a Thursday. Ho Chi Minh City is UTC+7.
    const THURSDAY_0700_LOCAL: &str = "2099-10-01T00:00:00Z";

    #[test]
    fn a_weekly_reminder_runs_on_the_next_chosen_weekday() {
        let saturday_sunday = Schedule {
            repeat: Repeat::Weekdays(0b100_0001),
            time: time("08:00"),
        };
        // Thursday 07:00 → Saturday 08:00 local = 01:00 UTC.
        assert_eq!(
            saturday_sunday.next_run(at(THURSDAY_0700_LOCAL)),
            at("2099-10-03T01:00:00Z")
        );
        // Right at Saturday 08:00 → Sunday 08:00.
        assert_eq!(
            saturday_sunday.next_run(at("2099-10-03T01:00:00Z")),
            at("2099-10-04T01:00:00Z")
        );
    }

    #[test]
    fn a_daily_reminder_runs_later_today_or_tomorrow() {
        let daily = |hm| Schedule {
            repeat: Repeat::Weekdays(EVERY_DAY),
            time: time(hm),
        };
        assert_eq!(
            daily("09:00").next_run(at(THURSDAY_0700_LOCAL)),
            at("2099-10-01T02:00:00Z")
        );
        assert_eq!(
            daily("06:00").next_run(at(THURSDAY_0700_LOCAL)),
            at("2099-10-01T23:00:00Z")
        );
    }

    #[test]
    fn a_monthly_reminder_runs_on_its_day() {
        let first = Schedule {
            repeat: Repeat::MonthDay(1),
            time: time("09:00"),
        };
        assert_eq!(
            first.next_run(at(THURSDAY_0700_LOCAL)),
            at("2099-10-01T02:00:00Z")
        );
        assert_eq!(
            first.next_run(at("2099-10-01T02:00:00Z")),
            at("2099-11-01T02:00:00Z")
        );
    }

    #[test]
    fn local_midnight_belongs_to_the_local_day() {
        // Friday 23:30 local is 16:30 UTC; the next Saturday 00:15 is 17:15 UTC.
        let saturday = Schedule {
            repeat: Repeat::Weekdays(1 << 6),
            time: time("00:15"),
        };
        assert_eq!(
            saturday.next_run(at("2099-10-02T16:30:00Z")),
            at("2099-10-02T17:15:00Z")
        );
    }

    fn raw() -> RawReminder {
        RawReminder {
            repeat: Some("weekly".to_owned()),
            weekdays: vec![6, 0],
            day_of_month: None,
            time: Some("08:00".to_owned()),
            message: Some("Cuối tuần này ai chơi bóng rổ không?".to_owned()),
        }
    }

    #[test]
    fn checks_a_complete_request() {
        let plan = raw().checked().unwrap();
        assert_eq!(plan.schedule.repeat, Repeat::Weekdays(0b100_0001));
        assert_eq!(plan.schedule.time, time("08:00"));
        assert_eq!(plan.text, "Cuối tuần này ai chơi bóng rổ không?");
    }

    #[test]
    fn lists_what_is_missing_or_unusable() {
        let no_time = RawReminder {
            time: None,
            ..raw()
        };
        assert_eq!(no_time.checked().unwrap_err(), vec![ReminderGap::Time]);

        let broken = RawReminder {
            repeat: Some("monthly".to_owned()),
            day_of_month: Some(31),
            time: Some("8pm".to_owned()),
            message: Some("x".repeat(MAX_REMINDER_CHARS + 1)),
            ..raw()
        };
        assert_eq!(
            broken.checked().unwrap_err(),
            vec![ReminderGap::Repeat, ReminderGap::Time, ReminderGap::Message]
        );

        let bad_weekday = RawReminder {
            weekdays: vec![6, 7],
            ..raw()
        };
        assert_eq!(
            bad_weekday.checked().unwrap_err(),
            vec![ReminderGap::Repeat]
        );
        let no_weekday = RawReminder {
            weekdays: vec![],
            ..raw()
        };
        assert_eq!(no_weekday.checked().unwrap_err(), vec![ReminderGap::Repeat]);
    }

    #[test]
    fn daily_and_monthly_requests() {
        let daily = RawReminder {
            repeat: Some("daily".to_owned()),
            weekdays: vec![],
            ..raw()
        };
        assert_eq!(
            daily.checked().unwrap().schedule.repeat,
            Repeat::Weekdays(EVERY_DAY)
        );
        let monthly = RawReminder {
            repeat: Some("monthly".to_owned()),
            day_of_month: Some(28),
            ..raw()
        };
        assert_eq!(
            monthly.checked().unwrap().schedule.repeat,
            Repeat::MonthDay(28)
        );
    }

    #[test]
    fn stored_columns_round_trip_and_reject_nonsense() {
        for repeat in [
            Repeat::Weekdays(EVERY_DAY),
            Repeat::Weekdays(1),
            Repeat::MonthDay(15),
        ] {
            let (days, month_day) = repeat_columns(repeat);
            assert_eq!(repeat_from_columns(days, month_day), Some(repeat));
        }
        assert_eq!(repeat_from_columns(0, None), None);
        assert_eq!(repeat_from_columns(1, Some(3)), None);
        assert_eq!(repeat_from_columns(0, Some(29)), None);
        assert_eq!(repeat_from_columns(128, None), None);
    }
}

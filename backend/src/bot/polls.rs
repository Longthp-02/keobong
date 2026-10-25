//! Attendance polls the bot posts to a chat ("@Bot DaGhep tạo bảng điểm danh T7,
//! CN, nhắc lúc 20h"). Players answer yes or no for each day on the web, signed
//! in; once a day the bot posts the counts and who has not answered yet.
//! Pure rules: no Axum, sqlx, HTTP or SDK types.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};

use crate::auth::UserId;
use crate::matches::domain::{ShareId, local_date, local_instant};

/// Open polls one chat may have at a time.
pub const MAX_OPEN_POLLS_PER_CHAT: i64 = 3;
/// Days one poll may ask about.
pub const MAX_POLL_DAYS: usize = 7;
/// How far ahead a poll's days may be.
pub const MAX_POLL_DAYS_AHEAD: i64 = 14;
/// Longest poll title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;
/// A member counts for nudges while seen in the chat (tagging the bot, or
/// answering one of its polls) within this many days; people who left stop
/// being named.
pub const MEMBER_DAYS: i64 = 60;
/// Names listed in a nudge; the rest are counted ("và 4 người khác").
pub const MAX_NAMES_IN_NUDGE: usize = 15;

/// What the reader made of a poll request, before any checks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawPoll {
    pub title: Option<String>,
    /// YYYY-MM-DD.
    pub days: Vec<String>,
    /// HH:MM, 24-hour: when the daily nudge is posted.
    pub nudge_time: Option<String>,
}

/// A poll that passed the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollPlan {
    pub title: String,
    /// Sorted, without repeats.
    pub days: Vec<NaiveDate>,
    pub nudge_time: NaiveTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollGap {
    Title,
    Days,
    NudgeTime,
}

impl PollGap {
    pub fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Days => "days",
            Self::NudgeTime => "nudgeTime",
        }
    }
}

impl RawPoll {
    /// Checks every field against `today` (Ho Chi Minh City); lists what is
    /// missing or unusable.
    pub fn checked(self, today: NaiveDate) -> Result<PollPlan, Vec<PollGap>> {
        let title = self
            .title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty() && title.chars().count() <= MAX_TITLE_CHARS);
        let parsed: Option<Vec<NaiveDate>> = self
            .days
            .iter()
            .map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .collect();
        let days = parsed
            .map(|mut days| {
                days.sort();
                days.dedup();
                days
            })
            .filter(|days| {
                !days.is_empty()
                    && days.len() <= MAX_POLL_DAYS
                    && days.iter().all(|day| {
                        *day >= today && *day <= today + Duration::days(MAX_POLL_DAYS_AHEAD)
                    })
            });
        let nudge_time = self
            .nudge_time
            .as_deref()
            .and_then(|time| NaiveTime::parse_from_str(time, "%H:%M").ok());
        match (title, days, nudge_time) {
            (Some(title), Some(days), Some(nudge_time)) => Ok(PollPlan {
                title,
                days,
                nudge_time,
            }),
            (title, days, nudge_time) => Err([
                (title.is_none(), PollGap::Title),
                (days.is_none(), PollGap::Days),
                (nudge_time.is_none(), PollGap::NudgeTime),
            ]
            .into_iter()
            .filter_map(|(gap, kind)| gap.then_some(kind))
            .collect()),
        }
    }
}

impl PollPlan {
    /// Answers close at the end of the last day, Ho Chi Minh City time.
    pub fn closes_at(&self) -> DateTime<Utc> {
        let last = self.days.last().copied().unwrap_or_default();
        local_instant(last + Duration::days(1), NaiveTime::MIN)
    }

    /// The first daily nudge strictly after `after`, or `None` once the poll
    /// closes before it.
    pub fn next_nudge(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let today = local_date(after);
        [today, today + Duration::days(1)]
            .into_iter()
            .map(|day| local_instant(day, self.nudge_time))
            .find(|at| *at > after)
            .filter(|at| *at < self.closes_at())
    }
}

/// What the poll reader is asked to read.
pub struct PollRequest<'a> {
    pub text: &'a str,
    pub today: NaiveDate,
}

#[derive(Debug, thiserror::Error)]
#[error("poll reader failed: {0}")]
pub struct PollReaderError(pub String);

pub type PollFuture<'a> =
    Pin<Box<dyn Future<Output = Result<RawPoll, PollReaderError>> + Send + 'a>>;

/// Port: turns a poll request into a title, days and a nudge time (an AI model
/// in production).
pub trait PollReader: Send + Sync {
    fn read<'a>(&'a self, request: PollRequest<'a>) -> PollFuture<'a>;
}

/// A poll waiting for its author's "ok" (30 minutes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPoll {
    pub zalo_user_id: String,
    pub chat_id: String,
    pub user: UserId,
    pub plan: PollPlan,
    pub expires_at: DateTime<Utc>,
}

/// A saved poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Poll {
    pub id: i64,
    pub share_id: ShareId,
    pub chat_id: String,
    pub plan: PollPlan,
    pub closes_at: DateTime<Utc>,
}

/// One player's answer for one day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollAnswer {
    pub user: UserId,
    pub name: Option<String>,
    pub day: NaiveDate,
    pub attending: bool,
}

/// A chat member the bot knows: someone who answered one of its polls or, while
/// linked, tagged the bot there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub user: UserId,
    pub name: Option<String>,
}

/// What a nudge says: yes-counts per day, and who has not answered every day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tally {
    pub yes_per_day: Vec<(NaiveDate, usize)>,
    pub missing: Vec<Member>,
}

/// Counts the answers and finds the members who have not answered every day.
pub fn tally(plan: &PollPlan, answers: &[PollAnswer], members: &[Member]) -> Tally {
    let yes_per_day = plan
        .days
        .iter()
        .map(|day| {
            let yes = answers
                .iter()
                .filter(|answer| answer.day == *day && answer.attending)
                .count();
            (*day, yes)
        })
        .collect();
    let missing = members
        .iter()
        .filter(|member| {
            plan.days.iter().any(|day| {
                !answers
                    .iter()
                    .any(|answer| answer.user == member.user && answer.day == *day)
            })
        })
        .cloned()
        .collect();
    Tally {
        yes_per_day,
        missing,
    }
}

#[derive(Debug, thiserror::Error)]
#[error("poll repository error: {0}")]
pub struct PollRepoError(pub String);

/// What answering a poll found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerOutcome {
    Saved,
    UnknownPoll,
    Closed,
    /// A day that is not one of the poll's days.
    UnknownDay,
}

/// Port: polls, their answers and chat members in Postgres.
pub trait PollRepository: Send + Sync {
    /// Replaces whatever the sender has pending in that chat with this poll.
    fn save_pending_poll(
        &self,
        pending: &PendingPoll,
    ) -> impl Future<Output = Result<(), PollRepoError>> + Send;

    fn take_pending_poll(
        &self,
        zalo_user_id: &str,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<PendingPoll>, PollRepoError>> + Send;

    /// Saves a poll unless its chat already has `MAX_OPEN_POLLS_PER_CHAT` open
    /// ones; returns it when saved.
    fn add_poll(
        &self,
        chat_id: &str,
        user: UserId,
        plan: &PollPlan,
        share_id: &ShareId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<Poll>, PollRepoError>> + Send;

    fn poll(
        &self,
        share_id: &ShareId,
    ) -> impl Future<Output = Result<Option<Poll>, PollRepoError>> + Send;

    fn answers(
        &self,
        poll_id: i64,
    ) -> impl Future<Output = Result<Vec<PollAnswer>, PollRepoError>> + Send;

    /// Saves a player's answers, all or none.
    fn answer(
        &self,
        share_id: &ShareId,
        user: UserId,
        answers: &[(NaiveDate, bool)],
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<AnswerOutcome, PollRepoError>> + Send;

    /// Remembers that a linked user is in this chat.
    fn note_member(
        &self,
        chat_id: &str,
        user: UserId,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(), PollRepoError>> + Send;

    /// Everyone seen in the chat since `since`, oldest first.
    fn members(
        &self,
        chat_id: &str,
        since: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Member>, PollRepoError>> + Send;

    /// Takes the polls whose nudge is due and moves each to its next nudge (or
    /// none) before returning them, so a nudge is posted at most once.
    fn claim_due_nudges(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<(Poll, DateTime<Utc>)>, PollRepoError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(ymd: &str) -> NaiveDate {
        NaiveDate::parse_from_str(ymd, "%Y-%m-%d").unwrap()
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().unwrap()
    }

    // 2099-10-01 is a Thursday.
    fn today() -> NaiveDate {
        day("2099-10-01")
    }

    fn raw() -> RawPoll {
        RawPoll {
            title: Some("Ai chơi bóng rổ cuối tuần này?".to_owned()),
            days: vec!["2099-10-04".to_owned(), "2099-10-03".to_owned()],
            nudge_time: Some("20:00".to_owned()),
        }
    }

    #[test]
    fn checks_a_complete_request_and_sorts_the_days() {
        let plan = raw().checked(today()).unwrap();
        assert_eq!(plan.days, vec![day("2099-10-03"), day("2099-10-04")]);
        assert_eq!(plan.nudge_time, NaiveTime::from_hms_opt(20, 0, 0).unwrap());
    }

    #[test]
    fn lists_what_is_missing_or_out_of_range() {
        let no_time = RawPoll {
            nudge_time: None,
            ..raw()
        };
        assert_eq!(
            no_time.checked(today()).unwrap_err(),
            vec![PollGap::NudgeTime]
        );
        for days in [
            vec![],
            vec!["2099-09-30".to_owned()],
            vec!["2099-10-16".to_owned()],
            vec!["soon".to_owned()],
            (1..=8).map(|d| format!("2099-10-0{d}")).collect(),
        ] {
            let raw = RawPoll { days, ..raw() };
            assert_eq!(raw.checked(today()).unwrap_err(), vec![PollGap::Days]);
        }
        let empty = RawPoll {
            title: Some("  ".to_owned()),
            days: vec![],
            nudge_time: Some("8pm".to_owned()),
        };
        assert_eq!(
            empty.checked(today()).unwrap_err(),
            vec![PollGap::Title, PollGap::Days, PollGap::NudgeTime]
        );
    }

    #[test]
    fn nudges_daily_until_the_poll_closes() {
        let plan = raw().checked(today()).unwrap();
        // Closes at Monday 00:00 local = Sunday 17:00 UTC.
        assert_eq!(plan.closes_at(), at("2099-10-04T17:00:00Z"));
        // Thursday 07:00 local → Thursday 20:00 local = 13:00 UTC.
        assert_eq!(
            plan.next_nudge(at("2099-10-01T00:00:00Z")),
            Some(at("2099-10-01T13:00:00Z"))
        );
        // Right at a nudge → the next day's.
        assert_eq!(
            plan.next_nudge(at("2099-10-01T13:00:00Z")),
            Some(at("2099-10-02T13:00:00Z"))
        );
        // Sunday's nudge is the last one.
        assert_eq!(
            plan.next_nudge(at("2099-10-03T13:00:00Z")),
            Some(at("2099-10-04T13:00:00Z"))
        );
        assert_eq!(plan.next_nudge(at("2099-10-04T13:00:00Z")), None);
    }

    #[test]
    fn tallies_yes_per_day_and_who_has_not_answered_every_day() {
        let plan = raw().checked(today()).unwrap();
        let (sat, sun) = (day("2099-10-03"), day("2099-10-04"));
        let member = |id, name: &str| Member {
            user: UserId(id),
            name: Some(name.to_owned()),
        };
        let answer = |id, day, attending| PollAnswer {
            user: UserId(id),
            name: None,
            day,
            attending,
        };
        let answers = vec![
            answer(1, sat, true),
            answer(1, sun, false),
            answer(2, sat, true),
            answer(3, sun, true),
        ];
        let members = vec![
            member(1, "An"),
            member(2, "Bình"),
            member(3, "Chi"),
            member(4, "Dũng"),
        ];
        let tally = tally(&plan, &answers, &members);
        assert_eq!(tally.yes_per_day, vec![(sat, 2), (sun, 1)]);
        assert_eq!(
            tally.missing,
            vec![member(2, "Bình"), member(3, "Chi"), member(4, "Dũng")]
        );
    }
}

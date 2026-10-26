//! Attendance polls the bot posts to a chat ("@Bot DaGhep tạo bảng điểm danh T7,
//! CN, nhắc lúc 20h"). Players answer yes or no for each day on the web, signed
//! in; once a day the bot posts the counts and who has not answered yet.
//! Pure rules: no Axum, sqlx, HTTP or SDK types.

use std::future::Future;
use std::pin::Pin;

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};

use crate::auth::UserId;
use crate::matches::domain::{ShareId, local_date, local_instant};
use crate::matches::venues::{GameFormat, GroupVenue, Sport};

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
/// When the daily nudge is posted unless the request says otherwise.
pub const DEFAULT_NUDGE_TIME: NaiveTime = match NaiveTime::from_hms_opt(20, 0, 0) {
    Some(time) => time,
    None => panic!("20:00 is a valid time"),
};
/// Names listed in a nudge; the rest are counted ("và 4 người khác").
pub const MAX_NAMES_IN_NUDGE: usize = 15;

/// What the reader made of a poll request, before any checks.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RawPoll {
    pub title: Option<String>,
    /// YYYY-MM-DD.
    pub days: Vec<String>,
    /// HH:MM, 24-hour: when the players play.
    pub play_time: Option<String>,
    /// HH:MM, 24-hour: when the daily nudge is posted; `DEFAULT_NUDGE_TIME`
    /// when the request does not say.
    pub nudge_time: Option<String>,
    /// "hằng tuần": a new poll for the same weekdays every week until stopped.
    pub weekly: bool,
    /// "football" or "basketball".
    pub sport: Option<String>,
    /// "3x3", "5x5", "five_a_side", "seven_a_side" or "eleven_a_side".
    pub format: Option<String>,
    /// A venue slug from the list the reader was given.
    pub venue_id: Option<String>,
}

/// A poll that passed the rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollPlan {
    pub title: String,
    /// Sorted, without repeats.
    pub days: Vec<NaiveDate>,
    /// Always set for new polls; `None` for polls made before it was asked.
    pub play_time: Option<NaiveTime>,
    pub nudge_time: NaiveTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollGap {
    Title,
    Days,
    PlayTime,
    NudgeTime,
}

impl PollGap {
    pub fn key(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Days => "days",
            Self::PlayTime => "playTime",
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
        let time = |value: &str| NaiveTime::parse_from_str(value, "%H:%M").ok();
        let play_time = self.play_time.as_deref().and_then(time);
        // The nudge defaults to the evening (Long, 2026-10-26); a stated time
        // that cannot be read is asked again.
        let nudge_time = match self.nudge_time.as_deref() {
            None => Some(DEFAULT_NUDGE_TIME),
            Some(value) => time(value),
        };
        match (title, days, play_time, nudge_time) {
            (Some(title), Some(days), Some(play_time), Some(nudge_time)) => Ok(PollPlan {
                title,
                days,
                play_time: Some(play_time),
                nudge_time,
            }),
            (title, days, play_time, nudge_time) => Err([
                (title.is_none(), PollGap::Title),
                (days.is_none(), PollGap::Days),
                (play_time.is_none(), PollGap::PlayTime),
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
    /// Approved venues the reader may pick from.
    pub venues: &'a [GroupVenue],
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
    /// Repeat every week until stopped.
    pub weekly: bool,
    pub group: PollGroup,
    pub expires_at: DateTime<Utc>,
}

/// A poll that repeats every week: each Monday at its nudge time the bot posts a
/// new poll for that week's chosen weekdays (Long, 2026-10-26).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollSeries {
    pub id: i64,
    pub chat_id: String,
    pub user: UserId,
    pub title: String,
    /// Bit `n` is set for weekday `n`, counted from Sunday = 0.
    pub weekdays: u8,
    pub play_time: Option<NaiveTime>,
    pub nudge_time: NaiveTime,
    pub next_post_at: DateTime<Utc>,
    pub group: PollGroup,
}

/// What a group plays and where, and whether the home page shows it
/// (`docs/public-groups-plan.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PollGroup {
    pub sport: Option<Sport>,
    pub format: Option<GameFormat>,
    /// An approved venue's id.
    pub venue_id: Option<i64>,
    pub public: bool,
}

/// A public group as the home page lists it. No member names.
#[derive(Debug, Clone, PartialEq)]
pub struct PublicGroup {
    pub title: String,
    pub group: PollGroup,
    pub venue_name: Option<String>,
    pub venue_address: Option<String>,
    pub weekdays: u8,
    pub play_time: Option<NaiveTime>,
    /// This week's poll, when one is open: yes-counts per day and how many
    /// players answered.
    pub week: Option<(Vec<(NaiveDate, i64)>, i64)>,
}

/// Weekly polls one chat may have at a time.
pub const MAX_SERIES_PER_CHAT: i64 = 3;
/// A weekly post more than this late (an outage) is skipped until next week.
pub const MAX_SERIES_LATE_HOURS: i64 = 12;
/// Failed weekly posts in a row after which a series is dropped (the bot was
/// probably removed from the group).
pub const MAX_SERIES_FAILURES: i16 = 3;

/// The weekdays of a poll's days, as series bits.
pub fn weekday_bits(days: &[NaiveDate]) -> u8 {
    days.iter().fold(0, |bits, day| {
        bits | (1 << day.weekday().num_days_from_sunday())
    })
}

const MONDAY_BIT: u8 = 1 << 1;

fn has_day(weekdays: u8, day: NaiveDate) -> bool {
    weekdays & (1 << day.weekday().num_days_from_sunday()) != 0
}

/// The Monday of the Monday–Sunday week holding `day`.
fn monday_of(day: NaiveDate) -> NaiveDate {
    day - Duration::days(i64::from(day.weekday().num_days_from_monday()))
}

/// The days of the week starting `monday` that the series plays on.
fn week_days(weekdays: u8, monday: NaiveDate) -> Vec<NaiveDate> {
    (0..7)
        .map(|offset| monday + Duration::days(offset))
        .filter(|day| has_day(weekdays, *day))
        .collect()
}

/// When the poll for the week starting `monday` is posted: that Monday at the
/// nudge time, or the Sunday before when the series plays on Mondays, so a
/// Monday game is never posted after it is played.
pub fn post_at(weekdays: u8, nudge_time: NaiveTime, monday: NaiveDate) -> DateTime<Utc> {
    let day = if weekdays & MONDAY_BIT != 0 {
        monday - Duration::days(1)
    } else {
        monday
    };
    local_instant(day, nudge_time)
}

/// The week a post at `posted_at` is for (see `post_at`).
pub fn week_posted_at(posted_at: DateTime<Utc>) -> NaiveDate {
    let day = local_date(posted_at);
    match day.weekday() {
        chrono::Weekday::Sun => day + Duration::days(1),
        _ => monday_of(day),
    }
}

/// The first poll of a new weekly series made at `now`: the series' days still
/// ahead this week (or all of next week's when none are left), and when the
/// next week's poll is posted. Days run without gaps from one poll to the next.
pub fn first_week(
    weekdays: u8,
    nudge_time: NaiveTime,
    now: DateTime<Utc>,
) -> (Vec<NaiveDate>, DateTime<Utc>) {
    let today = local_date(now);
    let mut monday = monday_of(today);
    let mut days: Vec<NaiveDate> = week_days(weekdays, monday)
        .into_iter()
        .filter(|day| *day >= today)
        .collect();
    if days.is_empty() {
        monday += Duration::days(7);
        days = week_days(weekdays, monday);
    }
    let mut next = monday + Duration::days(7);
    // Too late for next week's post (a Sunday-evening post that has passed):
    // this first poll covers next week too.
    if post_at(weekdays, nudge_time, next) <= now {
        days.extend(week_days(weekdays, next));
        // A poll holds at most 7 days: keep next week's whole, drop the earliest.
        let extra = days.len().saturating_sub(MAX_POLL_DAYS);
        days.drain(..extra);
        next += Duration::days(7);
    }
    (days, post_at(weekdays, nudge_time, next))
}

impl PollSeries {
    /// The poll for the week the post at `posted_at` is for, without days that
    /// have already passed by `now` (a late post).
    pub fn plan_for_week(&self, posted_at: DateTime<Utc>, now: DateTime<Utc>) -> PollPlan {
        let today = local_date(now);
        PollPlan {
            title: self.title.clone(),
            days: week_days(self.weekdays, week_posted_at(posted_at))
                .into_iter()
                .filter(|day| *day >= today)
                .collect(),
            play_time: self.play_time,
            nudge_time: self.nudge_time,
        }
    }
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
        series_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<Poll>, PollRepoError>> + Send;

    fn poll(
        &self,
        share_id: &ShareId,
    ) -> impl Future<Output = Result<Option<Poll>, PollRepoError>> + Send;

    /// The chat's polls still open at `now`, newest first.
    fn open_polls(
        &self,
        chat_id: &str,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<Poll>, PollRepoError>> + Send;

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

    /// Saves a weekly series unless its chat already has `MAX_SERIES_PER_CHAT`;
    /// returns its id when saved, and ties `first_poll` to it.
    fn add_series(
        &self,
        series: &PollSeries,
        first_poll: i64,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<i64>, PollRepoError>> + Send;

    /// Shows or hides a series on the home page; returns whether it exists.
    fn set_series_public(
        &self,
        chat_id: &str,
        id: i64,
        public: bool,
    ) -> impl Future<Output = Result<bool, PollRepoError>> + Send;

    /// Public series, newest first, with their venue and this week's counts.
    fn public_groups(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<PublicGroup>, PollRepoError>> + Send;

    /// The chat's weekly series, oldest first (their list numbers).
    fn series(
        &self,
        chat_id: &str,
    ) -> impl Future<Output = Result<Vec<PollSeries>, PollRepoError>> + Send;

    fn delete_series(
        &self,
        chat_id: &str,
        id: i64,
    ) -> impl Future<Output = Result<bool, PollRepoError>> + Send;

    /// Takes the series whose weekly post is due and moves each to its next
    /// post after `now` before returning them, so a week's poll is posted at
    /// most once.
    fn claim_due_series(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<PollSeries>, PollRepoError>> + Send;

    /// Records a weekly post's result; a series that failed
    /// `MAX_SERIES_FAILURES` times in a row is deleted.
    fn record_series_post(
        &self,
        id: i64,
        delivered: bool,
    ) -> impl Future<Output = Result<(), PollRepoError>> + Send;

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
            play_time: Some("18:00".to_owned()),
            nudge_time: None,
            weekly: false,
            ..RawPoll::default()
        }
    }

    #[test]
    fn checks_a_complete_request_and_sorts_the_days() {
        let plan = raw().checked(today()).unwrap();
        assert_eq!(plan.days, vec![day("2099-10-03"), day("2099-10-04")]);
        assert_eq!(plan.play_time, NaiveTime::from_hms_opt(18, 0, 0));
        // No nudge time stated: 20:00.
        assert_eq!(plan.nudge_time, NaiveTime::from_hms_opt(20, 0, 0).unwrap());
        let stated = RawPoll {
            nudge_time: Some("09:30".to_owned()),
            ..raw()
        };
        assert_eq!(
            stated.checked(today()).unwrap().nudge_time,
            NaiveTime::from_hms_opt(9, 30, 0).unwrap()
        );
    }

    #[test]
    fn lists_what_is_missing_or_out_of_range() {
        let no_play_time = RawPoll {
            play_time: None,
            ..raw()
        };
        assert_eq!(
            no_play_time.checked(today()).unwrap_err(),
            vec![PollGap::PlayTime]
        );
        let unreadable_nudge = RawPoll {
            nudge_time: Some("tối".to_owned()),
            ..raw()
        };
        assert_eq!(
            unreadable_nudge.checked(today()).unwrap_err(),
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
            play_time: None,
            nudge_time: Some("8pm".to_owned()),
            weekly: false,
            ..RawPoll::default()
        };
        assert_eq!(
            empty.checked(today()).unwrap_err(),
            vec![
                PollGap::Title,
                PollGap::Days,
                PollGap::PlayTime,
                PollGap::NudgeTime
            ]
        );
    }

    const WEEKEND: u8 = 0b100_0001;
    const MON_WED: u8 = 0b000_1010;

    fn eight_pm() -> NaiveTime {
        NaiveTime::from_hms_opt(20, 0, 0).unwrap()
    }

    #[test]
    fn a_weekly_series_starts_with_the_days_left_this_week() {
        assert_eq!(
            weekday_bits(&[day("2099-10-03"), day("2099-10-04")]),
            WEEKEND
        );
        // Thursday 07:00: Sat + Sun this week; next post Monday 05 20:00.
        assert_eq!(
            first_week(WEEKEND, eight_pm(), at("2099-10-01T00:00:00Z")),
            (
                vec![day("2099-10-03"), day("2099-10-04")],
                at("2099-10-05T13:00:00Z")
            )
        );
        // Sunday 04 morning: only Sunday is left, and next week starts on time
        // (no Sunday 11 gap).
        assert_eq!(
            first_week(WEEKEND, eight_pm(), at("2099-10-04T01:00:00Z")),
            (vec![day("2099-10-04")], at("2099-10-05T13:00:00Z"))
        );
        // Tuesday 06: Wednesday 07 this week; Monday 12 belongs to next week's
        // poll, posted Sunday 11 at 20:00 (no Wednesday 14 gap).
        assert_eq!(
            first_week(MON_WED, eight_pm(), at("2099-10-06T01:00:00Z")),
            (vec![day("2099-10-07")], at("2099-10-11T13:00:00Z"))
        );
        // Sunday 11 at 21:00 for Mon + Wed: next week's post has passed, so the
        // first poll covers next week.
        // Every day, made Sunday 11 at 21:00: next week's 7 days, nothing lost.
        let every_day = first_week(0b111_1111, eight_pm(), at("2099-10-11T14:00:00Z"));
        assert_eq!(every_day.0.first(), Some(&day("2099-10-12")));
        assert_eq!(every_day.0.last(), Some(&day("2099-10-18")));
        assert_eq!(every_day.1, at("2099-10-18T13:00:00Z"));
        assert_eq!(
            first_week(MON_WED, eight_pm(), at("2099-10-11T14:00:00Z")),
            (
                vec![day("2099-10-12"), day("2099-10-14")],
                at("2099-10-18T13:00:00Z")
            )
        );
    }

    #[test]
    fn a_weekly_poll_covers_the_week_it_is_posted_for_without_past_days() {
        let series = PollSeries {
            id: 1,
            chat_id: "zgr".to_owned(),
            user: UserId(1),
            title: "Ai chơi?".to_owned(),
            weekdays: MON_WED,
            play_time: None,
            nudge_time: eight_pm(),
            next_post_at: at("2099-10-11T13:00:00Z"),
            group: PollGroup::default(),
        };
        // Posted Sunday 11 for the week of Monday 12.
        let posted = at("2099-10-11T13:00:00Z");
        assert_eq!(
            series.plan_for_week(posted, posted).days,
            vec![day("2099-10-12"), day("2099-10-14")]
        );
        // Posted late, on Tuesday 13: Monday 12 is gone.
        assert_eq!(
            series
                .plan_for_week(posted, at("2099-10-13T01:00:00Z"))
                .days,
            vec![day("2099-10-14")]
        );
        assert_eq!(
            post_at(WEEKEND, eight_pm(), day("2099-10-12")),
            at("2099-10-12T13:00:00Z")
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

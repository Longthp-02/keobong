//! Poll answers typed in the chat ("@Bot DaGhep T7 có, CN không"): which days a
//! player names and whether they come. Words live in `answerWords` in the copy
//! file. Pure rules: no Axum, sqlx, HTTP or SDK types.

use chrono::{Datelike, Duration, NaiveDate, Weekday};
use serde::Deserialize;

use super::domain::COPY;

/// Words of a chat answer (`answerWords` in the copy file).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerWords {
    /// Names of each weekday, Monday first.
    pub weekdays: Vec<Vec<String>>,
    /// Every day of the poll ("cả 2 ngày").
    pub all_days: Vec<String>,
    pub today: Vec<String>,
    pub tomorrow: Vec<String>,
    pub yes: Vec<String>,
    /// Checked before `yes`: "không đi được" is a no.
    pub no: Vec<String>,
    /// Words clear enough to answer a one-day poll without naming the day.
    pub strong: Vec<String>,
    /// Words that start a new part of the answer ("T7 có còn CN không").
    pub breaks: Vec<String>,
    /// Phrases of a question or a match post, not an answer ("ai", "kèo").
    pub not_answer: Vec<String>,
}

/// A day a player named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DayRef {
    Weekday(Weekday),
    /// Day and month ("31/10").
    Date(u32, u32),
    Today,
    Tomorrow,
    All,
}

/// What a chat answer says, before it is matched to a poll's days.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerIntent {
    /// Yes or no for the days named.
    Days(Vec<(DayRef, bool)>),
    /// Yes or no without a day: only clear for a one-day poll.
    NoDay(bool),
}

/// Why an answer does not fit a poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveError {
    /// A named day is not one of the poll's days.
    NotInPoll,
    /// No day named and the poll has several.
    WhichDay,
}

/// Reads a chat message as a poll answer; `None` when it is not one.
///
/// The message is cut into parts at commas and break words; each part may name
/// days and say yes or no ("không" wins over "có"). A part naming days without
/// a yes or no takes the one before it ("đăng ký T7, CN"), or else the one after
/// it ("T7, CN mình nghỉ").
pub fn read_answer(text: &str) -> Option<AnswerIntent> {
    let words = &COPY.answer_words;
    let text = text.to_lowercase();
    if text.contains('?') {
        return None;
    }
    let all = tokens(&text);
    // "19h", "60k", "19:00": a match post or a question about times.
    let timeish = |token: &&str| {
        token.starts_with(|c: char| c.is_ascii_digit())
            && token.chars().any(|c| c.is_alphabetic() || c == ':')
    };
    if all.iter().any(timeish) || has_phrase(&all, &words.not_answer) {
        return None;
    }
    let mut parts: Vec<(Vec<DayRef>, Option<bool>)> = Vec::new();
    for piece in text.split([',', ';', '\n', '.', '!']) {
        let piece = tokens(piece);
        for part in piece.split(|token| words.breaks.iter().any(|word| word == token)) {
            if part.is_empty() {
                continue;
            }
            let answer = if has_phrase(part, &words.no) {
                Some(false)
            } else if has_phrase(part, &words.yes) {
                Some(true)
            } else {
                None
            };
            parts.push((days_in(part, words), answer));
        }
    }
    if parts.iter().all(|(days, _)| days.is_empty()) {
        let mut answers = parts.iter().filter_map(|(_, answer)| *answer);
        let first = answers.next()?;
        let agreed = answers.all(|answer| answer == first);
        return (agreed && has_phrase(&all, &words.strong)).then_some(AnswerIntent::NoDay(first));
    }
    let mut result = Vec::new();
    for (index, (days, answer)) in parts.iter().enumerate() {
        if days.is_empty() {
            continue;
        }
        let answer = answer
            .or_else(|| parts[..index].iter().rev().find_map(|(_, answer)| *answer))
            .or_else(|| parts[index + 1..].iter().find_map(|(_, answer)| *answer))?;
        result.extend(days.iter().map(|day| (*day, answer)));
    }
    Some(AnswerIntent::Days(result))
}

/// Lower-case words, without punctuation around them; "+1" and "31/10" stay whole.
fn tokens(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|word| word.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '+' | '-'))))
        .filter(|word| !word.is_empty())
        .collect()
}

/// Whether one of `phrases` appears as whole words.
fn has_phrase(tokens: &[&str], phrases: &[String]) -> bool {
    phrases.iter().any(|phrase| {
        let phrase: Vec<&str> = phrase.split_whitespace().collect();
        !phrase.is_empty()
            && tokens
                .windows(phrase.len())
                .any(|window| window == phrase.as_slice())
    })
}

/// The days a part names, longest phrase first at each word.
fn days_in(tokens: &[&str], words: &AnswerWords) -> Vec<DayRef> {
    const WEEKDAYS: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let mut names: Vec<(Vec<&str>, DayRef)> = Vec::new();
    for (names_of_day, weekday) in words.weekdays.iter().zip(WEEKDAYS) {
        names.extend(
            names_of_day
                .iter()
                .map(|name| (name.split_whitespace().collect(), DayRef::Weekday(weekday))),
        );
    }
    for (list, day) in [
        (&words.all_days, DayRef::All),
        (&words.today, DayRef::Today),
        (&words.tomorrow, DayRef::Tomorrow),
    ] {
        names.extend(
            list.iter()
                .map(|name| (name.split_whitespace().collect(), day)),
        );
    }
    names.retain(|(name, _)| !name.is_empty());
    names.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut days = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        if let Some((name, day)) = names
            .iter()
            .find(|(name, _)| tokens[index..].starts_with(name))
        {
            days.push(*day);
            index += name.len();
            continue;
        }
        if let Some(day) = date_of(tokens[index]) {
            days.push(day);
        }
        index += 1;
    }
    days
}

/// "31/10" as a day and month.
fn date_of(token: &str) -> Option<DayRef> {
    let (day, month) = token.split_once('/')?;
    let day: u32 = day.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    ((1..=31).contains(&day) && (1..=12).contains(&month)).then_some(DayRef::Date(day, month))
}

impl AnswerIntent {
    /// The poll days this answer is for, with yes or no; later parts win.
    pub fn resolve(
        &self,
        days: &[NaiveDate],
        today: NaiveDate,
    ) -> Result<Vec<(NaiveDate, bool)>, ResolveError> {
        let parts = match self {
            Self::NoDay(answer) => match days {
                [day] => return Ok(vec![(*day, *answer)]),
                _ => return Err(ResolveError::WhichDay),
            },
            Self::Days(parts) => parts,
        };
        let mut result: Vec<(NaiveDate, bool)> = Vec::new();
        for (day, answer) in parts {
            let matched: Vec<NaiveDate> = match day {
                DayRef::All => days.to_vec(),
                DayRef::Today => days.iter().copied().filter(|day| *day == today).collect(),
                DayRef::Tomorrow => {
                    let tomorrow = today + Duration::days(1);
                    days.iter()
                        .copied()
                        .filter(|day| *day == tomorrow)
                        .collect()
                }
                DayRef::Date(date, month) => days
                    .iter()
                    .copied()
                    .filter(|day| day.day() == *date && day.month() == *month)
                    .collect(),
                // The first coming one, when a poll spans two weeks.
                DayRef::Weekday(weekday) => days
                    .iter()
                    .copied()
                    .find(|day| day.weekday() == *weekday && *day >= today)
                    .into_iter()
                    .collect(),
            };
            if matched.is_empty() {
                return Err(ResolveError::NotInPoll);
            }
            for day in matched {
                result.retain(|(seen, _)| *seen != day);
                result.push((day, *answer));
            }
        }
        result.sort_by_key(|(day, _)| *day);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, month, day).unwrap()
    }

    /// Saturday 31/10 and Sunday 01/11, read on Monday 26/10.
    fn weekend(text: &str) -> Result<Vec<(NaiveDate, bool)>, ResolveError> {
        read_answer(text)
            .unwrap_or_else(|| panic!("not read as an answer: {text}"))
            .resolve(&[date(10, 31), date(11, 1)], date(10, 26))
    }

    #[test]
    fn reads_days_and_yes_or_no() {
        assert_eq!(
            weekend("tui muốn đăng ký chơi bóng rổ chủ nhật"),
            Ok(vec![(date(11, 1), true)])
        );
        assert_eq!(
            weekend("T7 có, CN không"),
            Ok(vec![(date(10, 31), true), (date(11, 1), false)])
        );
        assert_eq!(
            weekend("thứ 7 có còn chủ nhật bận"),
            Ok(vec![(date(10, 31), true), (date(11, 1), false)])
        );
        assert_eq!(weekend("t7 k đi được"), Ok(vec![(date(10, 31), false)]));
        assert_eq!(read_answer("cả 2 ngày luôn"), None, "no yes or no word");
    }

    #[test]
    fn a_day_list_shares_one_answer() {
        assert_eq!(
            weekend("đăng ký T7, CN"),
            Ok(vec![(date(10, 31), true), (date(11, 1), true)])
        );
        assert_eq!(
            weekend("T7, CN mình nghỉ"),
            Ok(vec![(date(10, 31), false), (date(11, 1), false)])
        );
        assert_eq!(
            weekend("có cả 2 ngày"),
            Ok(vec![(date(10, 31), true), (date(11, 1), true)])
        );
        assert_eq!(weekend("+1 31/10"), Ok(vec![(date(10, 31), true)]));
    }

    #[test]
    fn a_day_without_a_word_takes_the_next_parts_answer() {
        assert_eq!(weekend("CN, có nha"), Ok(vec![(date(11, 1), true)]));
    }

    #[test]
    fn days_outside_the_poll_are_refused() {
        assert_eq!(weekend("T6 có"), Err(ResolveError::NotInPoll));
        assert_eq!(weekend("nay có"), Err(ResolveError::NotInPoll));
    }

    #[test]
    fn no_day_is_clear_only_for_a_one_day_poll() {
        let intent = read_answer("đăng ký").unwrap();
        assert_eq!(intent, AnswerIntent::NoDay(true));
        assert_eq!(
            intent.resolve(&[date(10, 31)], date(10, 26)),
            Ok(vec![(date(10, 31), true)])
        );
        assert_eq!(
            intent.resolve(&[date(10, 31), date(11, 1)], date(10, 26)),
            Err(ResolveError::WhichDay)
        );
        assert_eq!(read_answer("nghỉ nha"), Some(AnswerIntent::NoDay(false)));
        // "có" alone is too weak without a day.
        assert_eq!(read_answer("có"), None);
    }

    #[test]
    fn today_and_tomorrow_are_days() {
        let intent = read_answer("tối nay mình chơi").unwrap();
        assert_eq!(
            intent.resolve(&[date(10, 31)], date(10, 31)),
            Ok(vec![(date(10, 31), true)])
        );
        let intent = read_answer("ngày mai không đi").unwrap();
        assert_eq!(
            intent.resolve(&[date(10, 31), date(11, 1)], date(10, 31)),
            Ok(vec![(date(11, 1), false)])
        );
    }

    #[test]
    fn questions_match_posts_and_chatter_are_not_answers() {
        for text in [
            "tối nay 19h SSA sân 7 thiếu 3, 60k/người",
            "ai chơi T7 không",
            "T7 có kèo không",
            "CN chơi được không?",
            "T7 có không",
            "haizzz",
            "",
            "tìm kèo",
            "T7",
        ] {
            assert_eq!(read_answer(text), None, "{text}");
        }
    }
}

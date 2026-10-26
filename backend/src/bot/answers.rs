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
    /// A part ending in one of these after a yes word is a question
    /// ("T7 chơi không").
    pub question_end: Vec<String>,
    /// Playing words ("đá", "đánh"): a yes, except in a question ("T7 đá không").
    pub play: Vec<String>,
    /// Words an answer may also hold ("tui", "nha"). Any other word means the
    /// message is not an answer: a match post, a question or chatter.
    pub filler: Vec<String>,
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

/// What one known phrase in an answer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Day(DayRef),
    Yes,
    No,
    Break,
    /// "đá", "đánh": a yes ("CN đá"), but "T7 đá không" is a question.
    Play,
    Filler,
}

/// One part of an answer: the days it names and what it says about them.
struct Part {
    days: Vec<DayRef>,
    answer: Option<bool>,
    /// Only days (and filler): a list like "T7, CN" that shares a neighbour's answer.
    only_days: bool,
    /// Parts after a break word ("còn") never share an answer with earlier ones.
    segment: usize,
}

/// Reads a chat message as a poll answer; `None` when it is not one.
///
/// Every word must be a day, a yes or no word, a break word or filler, so match
/// posts, questions and chatter are not taken for answers. The message is cut
/// into parts at commas and break words; "không" wins over "có" in a part. A
/// part that only names days takes the answer of the part before it ("đăng ký
/// T7, CN"), or else after it ("T7, CN mình nghỉ"), but not across a break word.
pub fn read_answer(text: &str) -> Option<AnswerIntent> {
    use unicode_normalization::UnicodeNormalization;
    let words = &COPY.answer_words;
    let text: String = text.nfc().collect::<String>().to_lowercase();
    if text.contains('?') {
        return None;
    }
    let table = phrase_table(words);
    let mut parts: Vec<Part> = Vec::new();
    let mut segment = 0;
    for piece in text.split([',', ';', '\n', '.', '!']) {
        let tokens = tokens(piece);
        let mut items: Vec<(Item, &[String])> = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let found = table.iter().find(|(phrase, _)| {
                tokens[index..].len() >= phrase.len()
                    && phrase.iter().zip(&tokens[index..]).all(|(a, b)| a == b)
            });
            if let Some((phrase, item)) = found {
                items.push((*item, phrase.as_slice()));
                index += phrase.len();
            } else {
                // Any other word: not an answer.
                let day = date_of(tokens[index])?;
                items.push((Item::Day(day), &[]));
                index += 1;
            }
        }
        for (position, chunk) in items.split(|(item, _)| *item == Item::Break).enumerate() {
            if position > 0 {
                segment += 1;
            }
            let meaningful: Vec<&(Item, &[String])> = chunk
                .iter()
                .filter(|(item, _)| *item != Item::Filler)
                .collect();
            // "T7 có CN không": a new day after a yes, no or play word starts a new part.
            let mut pieces: Vec<Vec<&(Item, &[String])>> = vec![Vec::new()];
            for entry in meaningful {
                let Some(current) = pieces.last_mut() else {
                    continue;
                };
                let answered = current
                    .iter()
                    .any(|(item, _)| matches!(item, Item::Yes | Item::No | Item::Play));
                if matches!(entry.0, Item::Day(_)) && answered {
                    pieces.push(vec![entry]);
                } else {
                    current.push(entry);
                }
            }
            for piece in pieces.into_iter().filter(|piece| !piece.is_empty()) {
                parts.push(read_part(&piece, words, segment)?);
            }
        }
    }
    if parts.iter().all(|part| part.days.is_empty()) {
        let mut answers = parts.iter().filter_map(|part| part.answer);
        let first = answers.next()?;
        let agreed = answers.all(|answer| answer == first);
        return (agreed && has_phrase(&tokens(&text), &words.strong))
            .then_some(AnswerIntent::NoDay(first));
    }
    let mut result = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        if part.days.is_empty() {
            continue;
        }
        let shared = |other: &Part| (other.segment == part.segment).then_some(other.answer)?;
        let answer = part.answer.or_else(|| {
            part.only_days
                .then(|| {
                    parts[..index]
                        .iter()
                        .rev()
                        .find_map(shared)
                        .or_else(|| parts[index + 1..].iter().find_map(shared))
                })
                .flatten()
        })?;
        result.extend(part.days.iter().map(|day| (*day, answer)));
    }
    Some(AnswerIntent::Days(result))
}

/// One part of an answer; `None` when it is a question ("T7 chơi không",
/// "T7 đá không", "T7 bận không"): a question word at the end after a yes, play
/// or no word.
fn read_part(items: &[&(Item, &[String])], words: &AnswerWords, segment: usize) -> Option<Part> {
    if let Some(((Item::No, phrase), before)) =
        items.split_last().map(|(last, before)| (**last, before))
        && words
            .question_end
            .iter()
            .any(|end| phrase.len() == 1 && phrase[0] == *end)
        && before
            .iter()
            .any(|(item, _)| matches!(item, Item::Yes | Item::Play | Item::No))
    {
        return None;
    }
    let days: Vec<DayRef> = items
        .iter()
        .filter_map(|(item, _)| match item {
            Item::Day(day) => Some(*day),
            _ => None,
        })
        .collect();
    let answer = if items.iter().any(|(item, _)| *item == Item::No) {
        Some(false)
    } else if items
        .iter()
        .any(|(item, _)| matches!(item, Item::Yes | Item::Play))
    {
        // "CN đá": playing is a yes.
        Some(true)
    } else {
        None
    };
    Some(Part {
        only_days: items.iter().all(|(item, _)| matches!(item, Item::Day(_))),
        days,
        answer,
        segment,
    })
}

/// Every known phrase as words, longest first; at equal length a no word wins
/// over a yes word, which wins over a day.
fn phrase_table(words: &AnswerWords) -> Vec<(Vec<String>, Item)> {
    const WEEKDAYS: [Weekday; 7] = [
        Weekday::Mon,
        Weekday::Tue,
        Weekday::Wed,
        Weekday::Thu,
        Weekday::Fri,
        Weekday::Sat,
        Weekday::Sun,
    ];
    let mut groups: Vec<(&Vec<String>, Item)> = vec![
        (&words.no, Item::No),
        (&words.yes, Item::Yes),
        (&words.all_days, Item::Day(DayRef::All)),
        (&words.today, Item::Day(DayRef::Today)),
        (&words.tomorrow, Item::Day(DayRef::Tomorrow)),
    ];
    for (names, weekday) in words.weekdays.iter().zip(WEEKDAYS) {
        groups.push((names, Item::Day(DayRef::Weekday(weekday))));
    }
    groups.push((&words.breaks, Item::Break));
    groups.push((&words.play, Item::Play));
    groups.push((&words.filler, Item::Filler));
    let mut table: Vec<(Vec<String>, Item)> = groups
        .into_iter()
        .flat_map(|(phrases, item)| {
            phrases.iter().map(move |phrase| {
                let words: Vec<String> = phrase.split_whitespace().map(str::to_owned).collect();
                (words, item)
            })
        })
        .filter(|(words, _)| !words.is_empty())
        .collect();
    // Stable: equal lengths keep the order above.
    table.sort_by_key(|(words, _)| std::cmp::Reverse(words.len()));
    table
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
            // Clear only when one day is left to answer.
            Self::NoDay(answer) => {
                let left: Vec<NaiveDate> =
                    days.iter().copied().filter(|day| *day >= today).collect();
                return match left.as_slice() {
                    [day] => Ok(vec![(*day, *answer)]),
                    [] => Err(ResolveError::NotInPoll),
                    _ => Err(ResolveError::WhichDay),
                };
            }
            Self::Days(parts) => parts,
        };
        let mut result: Vec<(NaiveDate, bool)> = Vec::new();
        for (day, answer) in parts {
            let matched: Vec<NaiveDate> = match day {
                DayRef::All => days.iter().copied().filter(|day| *day >= today).collect(),
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
                    .filter(|day| day.day() == *date && day.month() == *month && *day >= today)
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
    fn cancelling_is_a_no_and_this_week_is_not_today() {
        assert_eq!(weekend("huỷ đăng ký CN"), Ok(vec![(date(11, 1), false)]));
        assert_eq!(weekend("hủy đăng ký CN"), Ok(vec![(date(11, 1), false)]));
        assert_eq!(weekend("tuan nay T7 co"), Ok(vec![(date(10, 31), true)]));
        assert_eq!(weekend("tuần này T7 có"), Ok(vec![(date(10, 31), true)]));
    }

    #[test]
    fn answers_without_commas_and_names_that_look_like_words() {
        let both = Ok(vec![(date(10, 31), true), (date(11, 1), false)]);
        assert_eq!(weekend("T7 có CN không"), both);
        assert_eq!(weekend("thứ 7 có chủ nhật không"), both);
        assert_eq!(weekend("t7 co cn ko"), both);
        assert_eq!(weekend("T7 không"), Ok(vec![(date(10, 31), false)]));
        assert_eq!(weekend("CN ko đi dc"), Ok(vec![(date(11, 1), false)]));
        assert_eq!(weekend("dạ ae T7 có"), Ok(vec![(date(10, 31), true)]));
        assert_eq!(read_answer("anh Huy T7 có"), None, "a name, not huỷ");
    }

    #[test]
    fn a_day_with_a_play_word_is_a_yes() {
        let sat_no_sun_yes = Ok(vec![(date(10, 31), false), (date(11, 1), true)]);
        for text in [
            "T7 nghỉ CN đá",
            "T7 bận, CN đá",
            "nghỉ T7, CN đá",
            "T7 không đá, CN đá",
            "T7 k, CN đá nhé",
        ] {
            assert_eq!(weekend(text), sat_no_sun_yes, "{text}");
        }
        assert_eq!(
            weekend("T7 đá CN nghỉ"),
            Ok(vec![(date(10, 31), true), (date(11, 1), false)])
        );
        assert_eq!(weekend("T7 đá"), Ok(vec![(date(10, 31), true)]));
    }

    #[test]
    fn a_word_for_one_day_left_answers_it() {
        let intent = read_answer("đăng ký").unwrap();
        assert_eq!(
            intent.resolve(&[date(10, 31), date(11, 1)], date(11, 1)),
            Ok(vec![(date(11, 1), true)])
        );
        assert_eq!(
            intent.resolve(&[date(10, 31)], date(11, 1)),
            Err(ResolveError::NotInPoll)
        );
    }

    #[test]
    fn decomposed_accents_read_like_composed_ones() {
        // "không" typed with separate tone marks, as some keyboards send it.
        let decomposed = "T7 kho\u{302}ng";
        assert_eq!(weekend(decomposed), Ok(vec![(date(10, 31), false)]));
    }

    #[test]
    fn days_already_past_are_not_answered() {
        let intent = read_answer("cả 2 ngày có").unwrap();
        assert_eq!(
            intent.resolve(&[date(10, 31), date(11, 1)], date(11, 1)),
            Ok(vec![(date(11, 1), true)])
        );
        let intent = read_answer("31/10 có").unwrap();
        assert_eq!(
            intent.resolve(&[date(10, 31), date(11, 1)], date(11, 1)),
            Err(ResolveError::NotInPoll)
        );
    }

    #[test]
    fn days_outside_the_poll_are_refused() {
        assert_eq!(weekend("T6 có"), Err(ResolveError::NotInPoll));
        assert_eq!(weekend("hôm nay có"), Err(ResolveError::NotInPoll));
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
            // Review 2026-10-26: posts, questions and unsure replies.
            "CN này chơi sân Lakeview 7 giờ tối, 5 người, 50 nghìn",
            "T7 chơi không",
            "T7 có chơi không",
            "CN đi không",
            "T7 có, CN chưa biết",
            "T7 có còn CN để xem",
            "T7 chưa được",
            "T7 có thể",
            "chủ nhật đi đá ở đâu",
            "T7 mấy giờ vào",
            "CN cho mình +1 bạn nữa",
            "T7 60 k",
            // Second review: questions with a play word, a name.
            "T7 đá không",
            "Thứ 7 đá không anh em",
            "CN đánh k",
            "T7 bận không",
        ] {
            assert_eq!(read_answer(text), None, "{text}");
        }
    }
}

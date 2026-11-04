//! DD/MM/YYYY date parsing and validation (design D6, spec "Editing invoice
//! number and dates"). Pure functions only: no UI, no I/O, no external crates.
//!
//! Dates are stored as free text at the model layer; this module is the
//! reusable check consumed by the Invoice details panel and the export gate.
//!
//! [`Date::parse`] runs these checks in order and reports the first failure:
//! 1. Shape: exactly 10 ASCII characters, `DD/MM/YYYY`, with `/` at positions
//!    2 and 5 and digits everywhere else. Otherwise [`DateError::Malformed`].
//! 2. Month lies in 1..=12. Otherwise [`DateError::MonthOutOfRange`].
//! 3. Day lies in 1..=days-in-that-month (leap-year aware for February).
//!    Otherwise [`DateError::DayOutOfRange`].

use std::fmt;

/// A validated calendar date. Only obtainable through [`Date::parse`], so any
/// `Date` value is known to be a real day in the Gregorian calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Date {
    day: u8,
    month: u8,
    year: u16,
}

/// Why a DD/MM/YYYY string was rejected. Variants separate a wrong shape from
/// out-of-range components so the UI can show a specific message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateError {
    /// Not exactly `DD/MM/YYYY` (wrong separators, wrong digit counts, or
    /// non-numeric characters).
    Malformed,
    /// The month is not in 1..=12.
    MonthOutOfRange { month: u8 },
    /// The day is not valid for the given month and year (e.g. 00, 31/04, or
    /// 29/02 in a non-leap year).
    DayOutOfRange { day: u8, month: u8, year: u16 },
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DateError::Malformed => write!(f, "Date must be in DD/MM/YYYY format"),
            DateError::MonthOutOfRange { month } => {
                write!(f, "Month must be between 01 and 12, found {month:02}")
            }
            DateError::DayOutOfRange { day, month, year } => write!(
                f,
                "Day {day:02} is not valid for {month:02}/{year:04}"
            ),
        }
    }
}

impl std::error::Error for DateError {}

impl Date {
    /// Parses and validates a `DD/MM/YYYY` string.
    pub fn parse(input: &str) -> Result<Date, DateError> {
        let bytes = input.as_bytes();
        let shape_ok = bytes.len() == 10
            && bytes[2] == b'/'
            && bytes[5] == b'/'
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| i == 2 || i == 5 || b.is_ascii_digit());
        if !shape_ok {
            return Err(DateError::Malformed);
        }

        let day = two_digits(&bytes[0..2]);
        let month = two_digits(&bytes[3..5]);
        let year = bytes[6..10]
            .iter()
            .fold(0u16, |acc, b| acc * 10 + u16::from(b - b'0'));

        if !(1..=12).contains(&month) {
            return Err(DateError::MonthOutOfRange { month });
        }
        if day < 1 || day > days_in_month(month, year) {
            return Err(DateError::DayOutOfRange { day, month, year });
        }

        Ok(Date {
            day,
            month,
            year,
        })
    }

    /// Day of the month, 1..=31.
    pub const fn day(self) -> u8 {
        self.day
    }

    /// Month of the year, 1..=12.
    pub const fn month(self) -> u8 {
        self.month
    }

    /// Four-digit year.
    pub const fn year(self) -> u16 {
        self.year
    }
}

impl fmt::Display for Date {
    /// Formats as `DD/MM/YYYY`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}/{:02}/{:04}", self.day, self.month, self.year)
    }
}

/// Validates a `DD/MM/YYYY` string without returning the parsed value.
pub fn validate(input: &str) -> Result<(), DateError> {
    Date::parse(input).map(|_| ())
}

/// Reads two ASCII digit bytes as a number. Caller guarantees the bytes are digits.
fn two_digits(bytes: &[u8]) -> u8 {
    (bytes[0] - b'0') * 10 + (bytes[1] - b'0')
}

/// Gregorian leap-year rule: divisible by 4, except centuries not divisible by 400.
const fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Number of days in `month` (1..=12) of `year`.
fn days_in_month(month: u8, year: u16) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        _ => 28,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(input: &str) -> (u8, u8, u16) {
        let d = Date::parse(input).expect("expected a valid date");
        (d.day(), d.month(), d.year())
    }

    // --- Acceptance: reference dates from the source invoice ---

    #[test]
    fn accepts_reference_dates() {
        assert_eq!(parts("03/10/2026"), (3, 10, 2026));
        assert_eq!(parts("11/09/2026"), (11, 9, 2026));
        assert_eq!(parts("15/10/2026"), (15, 10, 2026));
    }

    // --- Acceptance: other well-formed dates ---

    #[test]
    fn accepts_leap_day_in_leap_year() {
        assert_eq!(parts("29/02/2024"), (29, 2, 2024));
    }

    #[test]
    fn accepts_first_and_last_day_of_each_month_lengths() {
        assert_eq!(parts("31/01/2026"), (31, 1, 2026));
        assert_eq!(parts("30/04/2026"), (30, 4, 2026));
        assert_eq!(parts("31/12/2026"), (31, 12, 2026));
        assert_eq!(parts("01/01/2026"), (1, 1, 2026));
    }

    #[test]
    fn accepts_century_leap_year_divisible_by_400() {
        assert_eq!(parts("29/02/2000"), (29, 2, 2000));
    }

    #[test]
    fn validate_wrapper_accepts_well_formed_dates() {
        assert_eq!(validate("03/10/2026"), Ok(()));
        assert_eq!(validate("29/02/2024"), Ok(()));
    }

    #[test]
    fn display_round_trips_to_ddmmyyyy() {
        let d = Date::parse("03/10/2026").unwrap();
        assert_eq!(d.to_string(), "03/10/2026");
        assert_eq!(Date::parse(&d.to_string()), Ok(d));
    }

    // --- Rejection: wrong format ---

    #[test]
    fn rejects_iso_format() {
        assert_eq!(Date::parse("2026-10-03"), Err(DateError::Malformed));
    }

    #[test]
    fn rejects_single_digit_day_and_month() {
        assert_eq!(Date::parse("3/10/2026"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03/1/2026"), Err(DateError::Malformed));
    }

    #[test]
    fn rejects_dash_separators() {
        assert_eq!(Date::parse("03-10-2026"), Err(DateError::Malformed));
    }

    #[test]
    fn rejects_missing_parts() {
        assert_eq!(Date::parse("03/10"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03/10/26"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03102026"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03/"), Err(DateError::Malformed));
        assert_eq!(Date::parse(""), Err(DateError::Malformed));
    }

    #[test]
    fn rejects_non_numeric_characters() {
        assert_eq!(Date::parse("ab/10/2026"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03/1x/2026"), Err(DateError::Malformed));
        assert_eq!(Date::parse("03/10/20 6"), Err(DateError::Malformed));
        assert_eq!(Date::parse(" 3/10/2026"), Err(DateError::Malformed));
    }

    #[test]
    fn rejects_trailing_or_leading_extra_characters() {
        assert_eq!(Date::parse("03/10/2026 "), Err(DateError::Malformed));
        assert_eq!(Date::parse("003/10/2026"), Err(DateError::Malformed));
    }

    // --- Rejection: month out of range ---

    #[test]
    fn rejects_month_zero() {
        assert_eq!(
            Date::parse("03/00/2026"),
            Err(DateError::MonthOutOfRange { month: 0 })
        );
    }

    #[test]
    fn rejects_month_thirteen() {
        assert_eq!(
            Date::parse("03/13/2026"),
            Err(DateError::MonthOutOfRange { month: 13 })
        );
    }

    // --- Rejection: day out of range ---

    #[test]
    fn rejects_day_zero() {
        assert_eq!(
            Date::parse("00/10/2026"),
            Err(DateError::DayOutOfRange {
                day: 0,
                month: 10,
                year: 2026
            })
        );
    }

    #[test]
    fn rejects_day_31_in_30_day_month() {
        assert_eq!(
            Date::parse("31/04/2026"),
            Err(DateError::DayOutOfRange {
                day: 31,
                month: 4,
                year: 2026
            })
        );
        assert_eq!(
            Date::parse("31/06/2026"),
            Err(DateError::DayOutOfRange {
                day: 31,
                month: 6,
                year: 2026
            })
        );
    }

    #[test]
    fn rejects_feb_29_in_non_leap_year() {
        assert_eq!(
            Date::parse("29/02/2026"),
            Err(DateError::DayOutOfRange {
                day: 29,
                month: 2,
                year: 2026
            })
        );
    }

    #[test]
    fn rejects_feb_29_in_century_year_not_divisible_by_400() {
        assert_eq!(
            Date::parse("29/02/1900"),
            Err(DateError::DayOutOfRange {
                day: 29,
                month: 2,
                year: 1900
            })
        );
    }

    #[test]
    fn rejects_day_32() {
        assert_eq!(
            Date::parse("32/01/2026"),
            Err(DateError::DayOutOfRange {
                day: 32,
                month: 1,
                year: 2026
            })
        );
    }

    #[test]
    fn error_messages_are_usable_for_inline_display() {
        assert_eq!(
            DateError::Malformed.to_string(),
            "Date must be in DD/MM/YYYY format"
        );
        assert_eq!(
            DateError::MonthOutOfRange { month: 13 }.to_string(),
            "Month must be between 01 and 12, found 13"
        );
        assert_eq!(
            DateError::DayOutOfRange {
                day: 29,
                month: 2,
                year: 2026
            }
            .to_string(),
            "Day 29 is not valid for 02/2026"
        );
    }
}

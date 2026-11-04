//! IBAN validation and display grouping (design D7, spec "Editing bank details
//! with IBAN validation"). Pure functions only: no UI, no I/O, no external
//! crates.
//!
//! [`validate`] runs these checks in order and reports the first failure:
//! 1. Normalize: remove whitespace and uppercase (see [`normalize`]).
//! 2. Every remaining character must be an ASCII letter or digit.
//! 3. Length must lie within [`MIN_IBAN_LENGTH`]..=[`MAX_IBAN_LENGTH`].
//! 4. Characters 1-2 must be letters (country code); 3-4 must be digits (check digits).
//! 5. If the country code has a known length (a small SEPA table), the total
//!    length must match it.
//! 6. ISO 7064 mod-97-10 checksum: move the first four characters to the end,
//!    map letters A-Z to 10-35, and the resulting number must leave remainder 1
//!    when divided by 97.
//!
//! The mod-97 remainder is computed incrementally over the digit string, so the
//! arbitrarily large integer is never materialised and no bignum crate is needed.

use std::fmt;

/// Shortest IBAN allowed by ISO 13616 (the length of Norway's IBAN).
pub const MIN_IBAN_LENGTH: usize = 15;
/// Longest IBAN allowed by ISO 13616.
pub const MAX_IBAN_LENGTH: usize = 34;

/// Why an IBAN failed validation. Variants are ordered as checked by
/// [`validate`]; the first failing check is the one reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IbanError {
    /// Contains a character that is not an ASCII letter or digit.
    InvalidCharacter,
    /// Shorter than [`MIN_IBAN_LENGTH`] (also covers an in-progress entry).
    TooShort,
    /// Longer than [`MAX_IBAN_LENGTH`].
    TooLong,
    /// The first two characters are not both letters.
    InvalidCountryCode,
    /// Characters 3-4 are not both digits.
    InvalidCheckDigits,
    /// The country code has a known length and the IBAN differs from it.
    WrongLengthForCountry { expected: usize, actual: usize },
    /// Structure is valid but the mod-97 checksum does not equal 1.
    ChecksumFailed,
}

impl fmt::Display for IbanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IbanError::InvalidCharacter => {
                write!(f, "IBAN may contain only letters and digits")
            }
            IbanError::TooShort => write!(f, "IBAN is too short"),
            IbanError::TooLong => write!(f, "IBAN is too long"),
            IbanError::InvalidCountryCode => {
                write!(f, "IBAN must start with a two-letter country code")
            }
            IbanError::InvalidCheckDigits => {
                write!(f, "IBAN must have two check digits after the country code")
            }
            IbanError::WrongLengthForCountry { expected, actual } => write!(
                f,
                "IBAN for this country must be {expected} characters, found {actual}"
            ),
            IbanError::ChecksumFailed => write!(f, "IBAN checksum is invalid"),
        }
    }
}

impl std::error::Error for IbanError {}

/// Removes all whitespace and uppercases the input. Use this before storing
/// or comparing an IBAN; [`validate`] and [`group_in_blocks`] apply it themselves.
pub fn normalize(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

/// Validates an IBAN (spaces are ignored, case is ignored).
///
/// Returns `Ok(())` for a structurally valid IBAN with a correct mod-97
/// checksum, otherwise the first [`IbanError`] encountered.
pub fn validate(input: &str) -> Result<(), IbanError> {
    let iban = normalize(input);

    if !iban.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(IbanError::InvalidCharacter);
    }

    let bytes = iban.as_bytes();
    let len = bytes.len();
    if len < MIN_IBAN_LENGTH {
        return Err(IbanError::TooShort);
    }
    if len > MAX_IBAN_LENGTH {
        return Err(IbanError::TooLong);
    }

    if !bytes[0].is_ascii_alphabetic() || !bytes[1].is_ascii_alphabetic() {
        return Err(IbanError::InvalidCountryCode);
    }
    if !bytes[2].is_ascii_digit() || !bytes[3].is_ascii_digit() {
        return Err(IbanError::InvalidCheckDigits);
    }

    if let Some(expected) = expected_length(&iban[0..2]) {
        if expected != len {
            return Err(IbanError::WrongLengthForCountry {
                expected,
                actual: len,
            });
        }
    }

    if mod97(&iban) != 1 {
        return Err(IbanError::ChecksumFailed);
    }

    Ok(())
}

/// Formats an IBAN (valid or still being typed) into space-separated blocks of
/// four characters, e.g. `DE89 3704 0044 0532 0130 00`. Whitespace is removed
/// and the result is uppercased first, so the output does not depend on how
/// the input was typed. This performs no validation.
pub fn group_in_blocks(input: &str) -> String {
    let normalized = normalize(input);
    let mut out = String::with_capacity(normalized.len() + normalized.len() / 4);
    for (i, c) in normalized.chars().enumerate() {
        if i > 0 && i % 4 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

/// Known total IBAN lengths for a selection of SEPA countries (ISO 13616
/// registry). Countries not listed here are checked only against the generic
/// length bounds and the checksum.
fn expected_length(country: &str) -> Option<usize> {
    Some(match country {
        "AT" => 20,
        "BE" => 16,
        "CH" => 21,
        "CZ" => 24,
        "DE" => 22,
        "DK" => 18,
        "ES" => 24,
        "FI" => 18,
        "FR" => 27,
        "GB" => 22,
        "IE" => 22,
        "IT" => 27,
        "LI" => 21,
        "LU" => 20,
        "NL" => 18,
        "NO" => 15,
        "PL" => 28,
        "PT" => 25,
        "SE" => 24,
        _ => return None,
    })
}

/// ISO 7064 mod-97-10 remainder of the rearranged IBAN. Expects an already
/// normalized, ASCII-alphanumeric string of at least four characters.
fn mod97(iban: &str) -> u32 {
    let rearranged = iban[4..].chars().chain(iban[..4].chars());
    let mut remainder: u32 = 0;
    for c in rearranged {
        remainder = match c.to_digit(10) {
            // Digit: append one decimal digit.
            Some(d) => (remainder * 10 + d) % 97,
            // Letter A-Z: append its two-digit value 10-35.
            None => {
                let value = (c as u32) - ('A' as u32) + 10;
                (remainder * 100 + value) % 97
            }
        };
    }
    remainder
}

#[cfg(test)]
mod tests {
    use super::*;

    const REFERENCE_SPACED: &str = "DE89 3704 0044 0532 0130 00";
    const REFERENCE_COMPACT: &str = "DE89370400440532013000";

    // --- Acceptance ---

    #[test]
    fn accepts_reference_iban_with_spaces() {
        assert_eq!(validate(REFERENCE_SPACED), Ok(()));
    }

    #[test]
    fn accepts_reference_iban_without_spaces() {
        assert_eq!(validate(REFERENCE_COMPACT), Ok(()));
    }

    #[test]
    fn accepts_lowercase_reference_iban() {
        assert_eq!(validate("de89 3704 0044 0532 0130 00"), Ok(()));
    }

    #[test]
    fn accepts_other_published_examples() {
        assert_eq!(validate("GB82 WEST 1234 5698 7654 32"), Ok(()));
        assert_eq!(validate("FR14 2004 1010 0505 0001 3M02 606"), Ok(()));
    }

    // --- Checksum rejection ---

    #[test]
    fn rejects_bad_checksum_from_flipped_last_digit() {
        assert_eq!(
            validate("DE59 3704 0044 0532 0130 00"),
            Err(IbanError::ChecksumFailed)
        );
    }

    #[test]
    fn rejects_bad_checksum_from_flipped_middle_digit() {
        assert_eq!(
            validate("DE59 3704 0044 0532 0130 00"),
            Err(IbanError::ChecksumFailed)
        );
    }

    #[test]
    fn rejects_bad_checksum_from_wrong_check_digits() {
        assert_eq!(
            validate("DE60 3704 0044 0532 0130 00"),
            Err(IbanError::ChecksumFailed)
        );
    }

    // --- Structural rejection ---

    #[test]
    fn rejects_too_short_input() {
        assert_eq!(validate("DE59 1001"), Err(IbanError::TooShort));
        assert_eq!(validate(""), Err(IbanError::TooShort));
    }

    #[test]
    fn rejects_too_long_input() {
        // 35 characters: one more than the ISO maximum of 34.
        assert_eq!(
            validate("DE89370400440532013000000000000000X"),
            Err(IbanError::TooLong)
        );
    }

    #[test]
    fn rejects_non_alphanumeric_input() {
        assert_eq!(
            validate("DE89-3704-0044-0532-0130-00"),
            Err(IbanError::InvalidCharacter)
        );
        assert_eq!(
            validate("DE59 3704 0044 0532 0130 0!"),
            Err(IbanError::InvalidCharacter)
        );
    }

    #[test]
    fn rejects_country_code_that_is_not_letters() {
        assert_eq!(
            validate("1E59 3704 0044 0532 0130 00"),
            Err(IbanError::InvalidCountryCode)
        );
    }

    #[test]
    fn rejects_check_digits_that_are_not_digits() {
        assert_eq!(
            validate("DEX9 3704 0044 0532 0130 00"),
            Err(IbanError::InvalidCheckDigits)
        );
    }

    #[test]
    fn rejects_german_iban_with_wrong_length() {
        // Reference IBAN minus its last character: 21 characters, Germany needs 22.
        assert_eq!(
            validate("DE59 3704 0044 0532 0130 0"),
            Err(IbanError::WrongLengthForCountry {
                expected: 22,
                actual: 21
            })
        );
    }

    #[test]
    fn error_messages_are_usable_for_inline_display() {
        assert_eq!(
            IbanError::ChecksumFailed.to_string(),
            "IBAN checksum is invalid"
        );
        assert_eq!(
            IbanError::WrongLengthForCountry {
                expected: 22,
                actual: 21
            }
            .to_string(),
            "IBAN for this country must be 22 characters, found 21"
        );
    }

    // --- Normalization ---

    #[test]
    fn normalize_strips_whitespace_and_uppercases() {
        assert_eq!(normalize(" de59 3704\t0044 "), "DE5937040044");
    }

    // --- 4-character-block grouping ---

    #[test]
    fn groups_reference_iban_into_four_character_blocks() {
        assert_eq!(group_in_blocks(REFERENCE_COMPACT), REFERENCE_SPACED);
    }

    #[test]
    fn grouping_ignores_existing_spacing_and_case() {
        assert_eq!(group_in_blocks("de89 3704 0044 0532 0130 00"), REFERENCE_SPACED);
        assert_eq!(group_in_blocks(REFERENCE_SPACED), REFERENCE_SPACED);
    }

    #[test]
    fn groups_partial_input_in_progress() {
        assert_eq!(group_in_blocks("DE5910010"), "DE59 1001 0");
        assert_eq!(group_in_blocks("DE59"), "DE59");
        assert_eq!(group_in_blocks(""), "");
    }

    #[test]
    fn groups_non_german_iban_in_four_character_blocks() {
        assert_eq!(
            group_in_blocks("GB82WEST12345698765432"),
            "GB82 WEST 1234 5698 7654 32"
        );
    }

    #[test]
    fn grouped_output_still_validates() {
        assert_eq!(validate(&group_in_blocks(REFERENCE_COMPACT)), Ok(()));
    }
}

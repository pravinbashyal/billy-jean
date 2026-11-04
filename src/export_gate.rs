//! Export validation gate (task 9.1, spec "Export blocked by validation").
//!
//! [`check`] is a pure function over [`Invoice`]: no UI, no I/O. It returns every
//! reason export must be blocked, in a fixed order, and an empty list means the
//! invoice may be exported. The editor marks each offending field with its
//! message and shows [`summary`] beside the Export PDF control.
//!
//! Rules (all required before export):
//! - a non-blank sender Tax number (checked first, as the Sender panel comes
//!   first; VAT ID and Email are optional and never checked);
//! - at least one line item;
//! - a non-blank invoice number;
//! - Invoice date, Service date, and Due date, each present and a valid
//!   DD/MM/YYYY date (checked by [`crate::date::validate`]);
//! - a non-empty IBAN that passes [`crate::iban::validate`]. An empty IBAN is
//!   blocked here even though the panel shows no error for it while editing.
//!
//! Text is trimmed before checking, matching what the PDF projection prints.

use std::fmt;

use crate::app_state::Field;
use crate::date::{self, DateError};
use crate::iban::{self, IbanError};
use crate::model::Invoice;

/// One reason export is blocked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportIssue {
    /// The sender Tax number (Steuernummer) is blank.
    MissingTaxNumber,
    /// The Line items table is empty.
    NoLineItems,
    /// The invoice number is blank.
    MissingInvoiceNumber,
    /// A date field is blank. `field` is one of the three date fields.
    MissingDate { field: Field },
    /// A date field is not a valid DD/MM/YYYY date. `field` is one of the three date fields.
    MalformedDate { field: Field, error: DateError },
    /// The IBAN is blank.
    MissingIban,
    /// The IBAN is not empty but fails format or checksum validation.
    InvalidIban(IbanError),
}

impl ExportIssue {
    /// The form field this issue belongs to, for the inline error. `None` for
    /// [`ExportIssue::NoLineItems`], which belongs to the Line items panel.
    pub fn field(&self) -> Option<Field> {
        match self {
            ExportIssue::MissingTaxNumber => Some(Field::SenderTaxNumber),
            ExportIssue::NoLineItems => None,
            ExportIssue::MissingInvoiceNumber => Some(Field::InvoiceNumber),
            ExportIssue::MissingDate { field } | ExportIssue::MalformedDate { field, .. } => {
                Some(*field)
            }
            ExportIssue::MissingIban | ExportIssue::InvalidIban(_) => Some(Field::Iban),
        }
    }

    /// The name of the thing to fix, as used in the summary message.
    pub fn item_name(&self) -> &'static str {
        match self {
            ExportIssue::MissingTaxNumber => "Tax number",
            ExportIssue::NoLineItems => "Line items",
            ExportIssue::MissingInvoiceNumber => "Invoice number",
            ExportIssue::MissingDate { field } | ExportIssue::MalformedDate { field, .. } => {
                date_field_name(*field)
            }
            ExportIssue::MissingIban | ExportIssue::InvalidIban(_) => "IBAN",
        }
    }
}

/// The inline message for one issue.
impl fmt::Display for ExportIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportIssue::MissingTaxNumber => {
                write!(f, "Enter your tax number (Steuernummer).")
            }
            ExportIssue::NoLineItems => write!(f, "Add at least one line item."),
            ExportIssue::MissingInvoiceNumber => write!(f, "Enter an invoice number."),
            ExportIssue::MissingDate { .. } => write!(f, "Enter a date in DD/MM/YYYY format."),
            ExportIssue::MalformedDate { error, .. } => write!(f, "{error}"),
            ExportIssue::MissingIban => write!(f, "Enter an IBAN."),
            ExportIssue::InvalidIban(error) => write!(f, "{error}"),
        }
    }
}

fn date_field_name(field: Field) -> &'static str {
    match field {
        Field::InvoiceDate => "Invoice date",
        Field::ServiceDate => "Service date",
        Field::DueDate => "Due date",
        _ => "Date",
    }
}

/// Why one date field is rejected, before it is attached to a field.
enum DateProblem {
    Missing,
    Malformed(DateError),
}

fn check_date(text: &str) -> Result<(), DateProblem> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(DateProblem::Missing);
    }
    date::validate(trimmed).map_err(DateProblem::Malformed)
}

/// Every reason the invoice must not be exported, in display order. An empty
/// vector means export may proceed.
pub fn check(invoice: &Invoice) -> Vec<ExportIssue> {
    let mut issues = Vec::new();

    if invoice.sender.tax_number.trim().is_empty() {
        issues.push(ExportIssue::MissingTaxNumber);
    }

    if invoice.line_items.is_empty() {
        issues.push(ExportIssue::NoLineItems);
    }

    if invoice.meta.number.trim().is_empty() {
        issues.push(ExportIssue::MissingInvoiceNumber);
    }

    let dates = [
        (Field::InvoiceDate, &invoice.meta.invoice_date),
        (Field::ServiceDate, &invoice.meta.service_date),
        (Field::DueDate, &invoice.meta.due_date),
    ];
    for (field, text) in dates {
        match check_date(text) {
            Ok(()) => {}
            Err(DateProblem::Missing) => issues.push(ExportIssue::MissingDate { field }),
            Err(DateProblem::Malformed(error)) => {
                issues.push(ExportIssue::MalformedDate { field, error });
            }
        }
    }

    let iban_text = &invoice.bank.iban;
    if iban::normalize(iban_text).is_empty() {
        issues.push(ExportIssue::MissingIban);
    } else if let Err(error) = iban::validate(iban_text) {
        issues.push(ExportIssue::InvalidIban(error));
    }

    issues
}

/// The summary message shown beside the Export control, naming each item to fix
/// once. `None` when there are no issues.
pub fn summary(issues: &[ExportIssue]) -> Option<String> {
    if issues.is_empty() {
        return None;
    }
    let mut names: Vec<&'static str> = Vec::new();
    for issue in issues {
        let name = issue.item_name();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Some(format!("Export blocked. Fix: {}.", names.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bank, InvoiceMeta, LineItem, Money, Quantity, Sender};

    const REFERENCE_IBAN: &str = "DE89 3704 0044 0532 0130 00";
    const BAD_CHECKSUM_IBAN: &str = "DE59 3704 0044 0532 0130 00";

    /// An invoice that passes every rule.
    fn valid_invoice() -> Invoice {
        Invoice {
            sender: Sender {
                tax_number: "21/815/08150".to_string(),
                ..Sender::default()
            },
            meta: InvoiceMeta {
                number: "4".to_string(),
                invoice_date: "01/11/2026".to_string(),
                service_date: "31/10/2026".to_string(),
                due_date: "15/11/2026".to_string(),
            },
            line_items: vec![LineItem {
                description: "Consulting".to_string(),
                quantity: Quantity::whole(1),
                rate: Money::from_euros(200),
            }],
            bank: Bank {
                bank_name: "Musterbank".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: REFERENCE_IBAN.to_string(),
            },
            ..Invoice::default()
        }
    }

    #[test]
    fn all_valid_invoice_has_no_issues_and_no_summary() {
        let issues = check(&valid_invoice());
        assert!(issues.is_empty(), "unexpected issues: {issues:?}");
        assert_eq!(summary(&issues), None);
    }

    #[test]
    fn surrounding_whitespace_in_required_text_is_ignored() {
        let mut inv = valid_invoice();
        inv.meta.number = "  4 ".to_string();
        inv.meta.invoice_date = " 01/11/2026 ".to_string();
        assert!(check(&inv).is_empty());
    }

    #[test]
    fn blank_tax_number_blocks_export_with_its_own_message() {
        for tax_number in ["", "   ", " \t "] {
            let mut inv = valid_invoice();
            inv.sender.tax_number = tax_number.to_string();
            let issues = check(&inv);
            assert_eq!(
                issues,
                vec![ExportIssue::MissingTaxNumber],
                "tax number {tax_number:?}"
            );
            assert_eq!(issues[0].field(), Some(Field::SenderTaxNumber));
            assert_eq!(
                issues[0].to_string(),
                "Enter your tax number (Steuernummer)."
            );
            assert_eq!(
                summary(&issues).as_deref(),
                Some("Export blocked. Fix: Tax number.")
            );
        }
    }

    #[test]
    fn filled_tax_number_is_not_reported() {
        for tax_number in ["21/815/08150", "DE 123/456/78901", " 1 "] {
            let mut inv = valid_invoice();
            inv.sender.tax_number = tax_number.to_string();
            assert!(check(&inv).is_empty(), "tax number {tax_number:?}");
        }
    }

    #[test]
    fn vat_id_and_email_are_never_checked() {
        let mut inv = valid_invoice();
        inv.sender.vat_id = String::new();
        inv.sender.email = String::new();
        assert!(check(&inv).is_empty(), "blank VAT ID and email");

        inv.sender.vat_id = "not a vat id".to_string();
        inv.sender.email = "not an email".to_string();
        assert!(check(&inv).is_empty(), "malformed VAT ID and email");
    }

    #[test]
    fn no_line_items_blocks_export_with_panel_error() {
        let mut inv = valid_invoice();
        inv.line_items.clear();
        let issues = check(&inv);
        assert_eq!(issues, vec![ExportIssue::NoLineItems]);
        assert_eq!(issues[0].field(), None, "belongs to the Line items panel");
        assert_eq!(issues[0].to_string(), "Add at least one line item.");
        assert_eq!(
            summary(&issues).as_deref(),
            Some("Export blocked. Fix: Line items.")
        );
    }

    #[test]
    fn missing_invoice_number_blocks_export() {
        for number in ["", "   "] {
            let mut inv = valid_invoice();
            inv.meta.number = number.to_string();
            let issues = check(&inv);
            assert_eq!(
                issues,
                vec![ExportIssue::MissingInvoiceNumber],
                "number {number:?}"
            );
            assert_eq!(issues[0].field(), Some(Field::InvoiceNumber));
            assert_eq!(issues[0].to_string(), "Enter an invoice number.");
            assert_eq!(
                summary(&issues).as_deref(),
                Some("Export blocked. Fix: Invoice number.")
            );
        }
    }

    #[test]
    fn each_missing_date_blocks_export_on_its_own_field() {
        let cases = [
            (Field::InvoiceDate, "invoice_date"),
            (Field::ServiceDate, "service_date"),
            (Field::DueDate, "due_date"),
        ];
        for (field, name) in cases {
            let mut inv = valid_invoice();
            match field {
                Field::InvoiceDate => inv.meta.invoice_date = String::new(),
                Field::ServiceDate => inv.meta.service_date = "  ".to_string(),
                _ => inv.meta.due_date = String::new(),
            }
            let issues = check(&inv);
            assert_eq!(issues, vec![ExportIssue::MissingDate { field }], "{name}");
            assert_eq!(issues[0].field(), Some(field));
            assert_eq!(issues[0].to_string(), "Enter a date in DD/MM/YYYY format.");
        }
    }

    #[test]
    fn malformed_date_blocks_export_with_the_date_error_message() {
        let cases = [
            "2026-11-01", // wrong separators and order
            "1/11/2026",  // single-digit day
            "01/11/26",   // two-digit year
            "ab/cd/efgh", // non-numeric
        ];
        for text in cases {
            let mut inv = valid_invoice();
            inv.meta.invoice_date = text.to_string();
            let issues = check(&inv);
            assert_eq!(
                issues,
                vec![ExportIssue::MalformedDate {
                    field: Field::InvoiceDate,
                    error: DateError::Malformed,
                }],
                "date {text:?}"
            );
            assert_eq!(issues[0].field(), Some(Field::InvoiceDate));
            assert_eq!(issues[0].to_string(), "Date must be in DD/MM/YYYY format");
        }
    }

    #[test]
    fn impossible_calendar_date_is_malformed_with_day_message() {
        let mut inv = valid_invoice();
        inv.meta.due_date = "31/04/2026".to_string();
        let issues = check(&inv);
        assert_eq!(
            issues,
            vec![ExportIssue::MalformedDate {
                field: Field::DueDate,
                error: DateError::DayOutOfRange {
                    day: 31,
                    month: 4,
                    year: 2026,
                },
            }]
        );
        assert_eq!(issues[0].to_string(), "Day 31 is not valid for 04/2026");
        assert_eq!(
            summary(&issues).as_deref(),
            Some("Export blocked. Fix: Due date.")
        );
    }

    #[test]
    fn invalid_iban_blocks_export_with_the_iban_error() {
        let mut inv = valid_invoice();
        inv.bank.iban = BAD_CHECKSUM_IBAN.to_string();
        let issues = check(&inv);
        assert_eq!(
            issues,
            vec![ExportIssue::InvalidIban(IbanError::ChecksumFailed)]
        );
        assert_eq!(issues[0].field(), Some(Field::Iban));
        assert_eq!(issues[0].to_string(), "IBAN checksum is invalid");
        assert_eq!(
            summary(&issues).as_deref(),
            Some("Export blocked. Fix: IBAN.")
        );
    }

    #[test]
    fn malformed_iban_blocks_export_with_its_format_error() {
        let mut inv = valid_invoice();
        inv.bank.iban = "DE59 1001".to_string();
        let issues = check(&inv);
        assert_eq!(issues, vec![ExportIssue::InvalidIban(IbanError::TooShort)]);
    }

    #[test]
    fn empty_iban_blocks_export_with_its_own_message() {
        for iban in ["", "    "] {
            let mut inv = valid_invoice();
            inv.bank.iban = iban.to_string();
            let issues = check(&inv);
            assert_eq!(issues, vec![ExportIssue::MissingIban], "iban {iban:?}");
            assert_eq!(issues[0].field(), Some(Field::Iban));
            assert_eq!(issues[0].to_string(), "Enter an IBAN.");
        }
    }

    #[test]
    fn every_problem_is_reported_in_order_and_summary_names_each_once() {
        let inv = Invoice {
            bank: Bank {
                iban: BAD_CHECKSUM_IBAN.to_string(),
                ..Bank::default()
            },
            ..Invoice::default()
        };
        let issues = check(&inv);
        assert_eq!(
            issues,
            vec![
                ExportIssue::MissingTaxNumber,
                ExportIssue::NoLineItems,
                ExportIssue::MissingInvoiceNumber,
                ExportIssue::MissingDate {
                    field: Field::InvoiceDate
                },
                ExportIssue::MissingDate {
                    field: Field::ServiceDate
                },
                ExportIssue::MissingDate {
                    field: Field::DueDate
                },
                ExportIssue::InvalidIban(IbanError::ChecksumFailed),
            ]
        );
        assert_eq!(
            summary(&issues).as_deref(),
            Some(
                "Export blocked. Fix: Tax number, Line items, Invoice number, Invoice date, Service date, Due date, IBAN."
            )
        );
    }

    #[test]
    fn summary_lists_a_date_name_once_even_when_two_dates_are_wrong() {
        let mut inv = valid_invoice();
        inv.meta.invoice_date = String::new();
        inv.meta.service_date = String::new();
        let issues = check(&inv);
        assert_eq!(issues.len(), 2);
        assert_eq!(
            summary(&issues).as_deref(),
            Some("Export blocked. Fix: Invoice date, Service date.")
        );
    }
}

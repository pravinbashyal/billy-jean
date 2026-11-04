//! Testable state layer for the single-window invoice editor (task 5.1).
//!
//! No GPUI here: the window shell in `main.rs`/`editor.rs` is a thin wrapper
//! over [`AppState`] and [`PANEL_ORDER`], so the acceptance-bearing logic
//! (panel order, identity pre-fill, what each panel displays) is unit-tested
//! without a display.

use std::path::Path;

use crate::iban::{self, IbanError};
use crate::identity_store::{self, Identity, IdentityError};
use crate::model::{Bank, Invoice, LineItem, Money, Quantity, SenderLayout, VatRate};
use crate::pdf::return_line;

/// What [`AppState::persist_identity_to`] did with the identity (task 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistOutcome {
    /// The whole identity (Sender and Bank) was written to the store.
    Saved,
    /// The IBAN fails validation. The Sender was written, but the Bank was not
    /// changed: the store keeps the last valid Bank (or an empty Bank on a
    /// first run). An invalid IBAN is never written.
    SavedSenderOnly,
}

/// Whether a stored IBAN may be kept as the default: empty (not entered yet) or
/// valid. Mirrors the rule `AppState::iban_error` applies to the editor field.
fn iban_is_storable(iban_text: &str) -> bool {
    iban::normalize(iban_text).is_empty() || iban::validate(iban_text).is_ok()
}

/// One titled panel of the left-hand form column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Sender,
    BillTo,
    InvoiceDetails,
    LineItems,
    Totals,
    Notes,
    BankDetails,
}

/// The seven form panels in the order the spec requires (Launching scenario).
pub const PANEL_ORDER: [Panel; 7] = [
    Panel::Sender,
    Panel::BillTo,
    Panel::InvoiceDetails,
    Panel::LineItems,
    Panel::Totals,
    Panel::Notes,
    Panel::BankDetails,
];

impl Panel {
    /// The panel heading shown in the form.
    pub const fn title(self) -> &'static str {
        match self {
            Panel::Sender => "Sender",
            Panel::BillTo => "Bill to",
            Panel::InvoiceDetails => "Invoice details",
            Panel::LineItems => "Line items",
            Panel::Totals => "Totals",
            Panel::Notes => "Notes",
            Panel::BankDetails => "Bank details",
        }
    }
}

/// Every text field the simple panels edit (task 5.2). Each variant maps to
/// exactly one `Invoice` field, so the GPUI inputs and the tests share one mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    SenderName,
    SenderStreet,
    SenderPostalCodeCity,
    SenderCountry,
    SenderTaxNumber,
    SenderVatId,
    SenderEmail,
    ClientName,
    ClientStreet,
    ClientPostalCodeCity,
    ClientCountry,
    InvoiceNumber,
    InvoiceDate,
    ServiceDate,
    DueDate,
    Notes,
    BankName,
    Beneficiary,
    Iban,
}

/// The editable fields of one line-item row. The row Total is deliberately
/// absent: it is derived from Quantity and Rate and has no write path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineItemField {
    Description,
    Quantity,
    Rate,
}

/// The app's in-memory state: the single invoice being edited.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppState {
    pub invoice: Invoice,
}

impl AppState {
    /// Seeds a fresh invoice from the persisted identity (design D5).
    ///
    /// The identity's Sender and Bank become the invoice's Sender and Bank.
    /// Everything else (client, meta, line items, VAT, notes) starts empty;
    /// invoice content is not persisted across runs.
    pub fn new(identity: Identity) -> Self {
        let Identity { sender, bank } = identity;
        AppState {
            invoice: Invoice {
                sender,
                bank,
                ..Invoice::default()
            },
        }
    }

    /// The current model value behind one editable field. Each input is
    /// initialised from this, so the widget starts from the model.
    pub fn field_value(&self, field: Field) -> &str {
        let inv = &self.invoice;
        match field {
            Field::SenderName => &inv.sender.name,
            Field::SenderStreet => &inv.sender.street,
            Field::SenderPostalCodeCity => &inv.sender.postal_code_city,
            Field::SenderCountry => &inv.sender.country,
            Field::SenderTaxNumber => &inv.sender.tax_number,
            Field::SenderVatId => &inv.sender.vat_id,
            Field::SenderEmail => &inv.sender.email,
            Field::ClientName => &inv.client.name,
            Field::ClientStreet => &inv.client.street,
            Field::ClientPostalCodeCity => &inv.client.postal_code_city,
            Field::ClientCountry => &inv.client.country,
            Field::InvoiceNumber => &inv.meta.number,
            Field::InvoiceDate => &inv.meta.invoice_date,
            Field::ServiceDate => &inv.meta.service_date,
            Field::DueDate => &inv.meta.due_date,
            Field::Notes => &inv.notes,
            Field::BankName => &inv.bank.bank_name,
            Field::Beneficiary => &inv.bank.beneficiary,
            Field::Iban => &inv.bank.iban,
        }
    }

    /// Writes one edited value into the model. This is the only write path
    /// from the editable panels; every other field is left untouched. Values
    /// are stored verbatim (trimming happens in the PDF projection).
    pub fn set_field(&mut self, field: Field, value: impl Into<String>) {
        let value = value.into();
        let inv = &mut self.invoice;
        match field {
            Field::SenderName => inv.sender.name = value,
            Field::SenderStreet => inv.sender.street = value,
            Field::SenderPostalCodeCity => inv.sender.postal_code_city = value,
            Field::SenderCountry => inv.sender.country = value,
            Field::SenderTaxNumber => inv.sender.tax_number = value,
            Field::SenderVatId => inv.sender.vat_id = value,
            Field::SenderEmail => inv.sender.email = value,
            Field::ClientName => inv.client.name = value,
            Field::ClientStreet => inv.client.street = value,
            Field::ClientPostalCodeCity => inv.client.postal_code_city = value,
            Field::ClientCountry => inv.client.country = value,
            Field::InvoiceNumber => inv.meta.number = value,
            Field::InvoiceDate => inv.meta.invoice_date = value,
            Field::ServiceDate => inv.meta.service_date = value,
            Field::DueDate => inv.meta.due_date = value,
            Field::Notes => inv.notes = value,
            Field::BankName => inv.bank.bank_name = value,
            Field::Beneficiary => inv.bank.beneficiary = value,
            Field::Iban => inv.bank.iban = value,
        }
    }

    /// Writes the sender layout into the model (design D5). The layout is a
    /// `Sender` field, so it reaches `identity()` and the store with no extra plumbing.
    pub fn set_sender_layout(&mut self, layout: SenderLayout) {
        self.invoice.sender.layout = layout;
    }

    /// The inline error shown beneath the IBAN field (task 7.1, using the task
    /// 2.3 validation), or `None` when the IBAN is valid. An empty field means
    /// "not entered yet", so it shows no error.
    pub fn iban_error(&self) -> Option<IbanError> {
        let iban = &self.invoice.bank.iban;
        if iban::normalize(iban).is_empty() {
            return None;
        }
        iban::validate(iban).err()
    }

    /// The IBAN as shown under its field: four-character blocks (task 2.3
    /// grouping). Performs no validation.
    pub fn iban_grouped(&self) -> String {
        iban::group_in_blocks(&self.invoice.bank.iban)
    }

    /// The identity this state would retain as the default for the next launch:
    /// the Sender and Bank details (design D5).
    pub fn identity(&self) -> Identity {
        Identity {
            sender: self.invoice.sender.clone(),
            bank: self.invoice.bank.clone(),
        }
    }

    /// Writes the identity to the store in `dir` (task 7.2).
    ///
    /// The Sender is always written: a Sender edit is retained as the default
    /// with no condition. The Bank is written only when its IBAN is valid
    /// (`iban_error()` is `None`, which includes an empty IBAN). When the IBAN
    /// is invalid, the Bank currently in the store is kept, so an invalid IBAN
    /// is never written and the last valid Bank stays the default. If nothing
    /// valid is stored yet, the Bank is saved empty.
    ///
    /// The stored Bank is read back from `dir` rather than held in memory. If
    /// the store cannot be read (for example an unsupported schema version), an
    /// error is returned and nothing is written, so unknown data is not overwritten.
    pub fn persist_identity_to(&self, dir: &Path) -> Result<PersistOutcome, IdentityError> {
        let mut identity = self.identity();
        if self.iban_error().is_none() {
            identity_store::save_to(dir, &identity)?;
            return Ok(PersistOutcome::Saved);
        }

        let stored = identity_store::load_from(dir)?;
        identity.bank = if iban_is_storable(&stored.bank.iban) {
            stored.bank
        } else {
            Bank::default()
        };
        identity_store::save_to(dir, &identity)?;
        Ok(PersistOutcome::SavedSenderOnly)
    }

    /// Appends a new empty line item (empty description, quantity 0, rate 0)
    /// to the invoice's single `line_items` list and returns its index.
    pub fn add_line_item(&mut self) -> usize {
        self.invoice.line_items.push(LineItem::default());
        self.invoice.line_items.len() - 1
    }

    /// Removes exactly the row at `index` and returns it, or `None` when no
    /// row has that index (the list is left unchanged).
    pub fn remove_line_item(&mut self, index: usize) -> Option<LineItem> {
        if index < self.invoice.line_items.len() {
            Some(self.invoice.line_items.remove(index))
        } else {
            None
        }
    }

    /// The row's Total cell: Quantity x Rate, computed by the model. There is
    /// no setter for it, so it can only follow Quantity and Rate.
    pub fn line_item_total(&self, index: usize) -> Option<Money> {
        self.invoice.line_items.get(index).map(LineItem::total)
    }

    /// The text one row input shows for one field, rendered from the model.
    pub fn line_item_text(&self, index: usize, field: LineItemField) -> Option<String> {
        let item = self.invoice.line_items.get(index)?;
        Some(match field {
            LineItemField::Description => item.description.clone(),
            LineItemField::Quantity => format_quantity(item.quantity),
            LineItemField::Rate => format_rate(item.rate),
        })
    }

    /// Writes one edited row value into the model. The description is stored
    /// verbatim. Quantity and Rate are parsed; text that does not parse, or an
    /// unknown row index, leaves the model unchanged and returns `false`.
    pub fn set_line_item_text(&mut self, index: usize, field: LineItemField, text: &str) -> bool {
        let Some(item) = self.invoice.line_items.get_mut(index) else {
            return false;
        };
        match field {
            LineItemField::Description => item.description = text.to_string(),
            LineItemField::Quantity => match parse_quantity(text) {
                Some(quantity) => item.quantity = quantity,
                None => return false,
            },
            LineItemField::Rate => match parse_money(text) {
                Some(rate) => item.rate = rate,
                None => return false,
            },
        }
        true
    }

    /// The VAT-rate control's text, rendered from the model (`1900` is `19`).
    pub fn vat_rate_text(&self) -> String {
        format_percent(self.invoice.vat_rate)
    }

    /// Writes the VAT-rate control's text into the model. The text is a
    /// percentage with at most two decimals (`19`, `7.5`, `0.01`), stored as
    /// basis points. Text that does not parse, or a rate above 100%, leaves the
    /// model unchanged and returns `false`.
    pub fn set_vat_rate_text(&mut self, text: &str) -> bool {
        match parse_vat_rate(text) {
            Some(rate) => {
                self.invoice.vat_rate = rate;
                true
            }
            None => false,
        }
    }

    /// The standing §19 UStG note, read from the model's single rule
    /// ([`Invoice::section_19_note`]): `Some` at 0% VAT, `None` above.
    pub fn standing_note(&self) -> Option<&'static str> {
        self.invoice.section_19_note()
    }

    /// The sender return line `Name · Street · Postal City` (design D6), using
    /// the same formatter as the PDF projection so the line is single-sourced.
    pub fn return_line(&self) -> String {
        return_line(&self.invoice.sender)
    }

    /// The display lines for one panel, built from the current model values.
    ///
    /// The GPUI form renders these strings verbatim. Keeping them here makes
    /// what each panel shows testable without a window.
    pub fn panel_lines(&self, panel: Panel) -> Vec<String> {
        let inv = &self.invoice;
        match panel {
            Panel::Sender => address_lines(
                &inv.sender.name,
                &inv.sender.street,
                &inv.sender.postal_code_city,
                &inv.sender.country,
            ),
            Panel::BillTo => address_lines(
                &inv.client.name,
                &inv.client.street,
                &inv.client.postal_code_city,
                &inv.client.country,
            ),
            Panel::InvoiceDetails => vec![
                format!("Invoice number: {}", inv.meta.number),
                format!("Invoice date: {}", inv.meta.invoice_date),
                format!("Service date: {}", inv.meta.service_date),
                format!("Due date: {}", inv.meta.due_date),
            ],
            Panel::LineItems => {
                if inv.line_items.is_empty() {
                    vec!["No line items yet.".to_string()]
                } else {
                    inv.line_items
                        .iter()
                        .map(|item| {
                            let description = if item.description.is_empty() {
                                "(no description)"
                            } else {
                                item.description.as_str()
                            };
                            format!(
                                "{description}: {} x {} = {}",
                                format_quantity(item.quantity),
                                item.rate,
                                item.total()
                            )
                        })
                        .collect()
                }
            }
            Panel::Totals => {
                let totals = inv.totals();
                vec![
                    format!("Net: {}", totals.net),
                    format!("VAT ({}%): {}", format_percent(inv.vat_rate), totals.vat),
                    format!("Gross: {}", totals.gross),
                ]
            }
            Panel::Notes => {
                if inv.notes.is_empty() {
                    vec!["No notes.".to_string()]
                } else {
                    inv.notes.lines().map(str::to_string).collect()
                }
            }
            Panel::BankDetails => vec![
                format!("Bank: {}", inv.bank.bank_name),
                format!("Beneficiary: {}", inv.bank.beneficiary),
                format!("IBAN: {}", inv.bank.iban),
            ],
        }
    }
}

fn address_lines(name: &str, street: &str, postal_code_city: &str, country: &str) -> Vec<String> {
    vec![
        format!("Name: {name}"),
        format!("Street: {street}"),
        format!("Postal code/City: {postal_code_city}"),
        format!("Country: {country}"),
    ]
}

/// Renders a quantity stored in thousandths without trailing zeros (`2500` is `2.5`).
fn format_quantity(quantity: Quantity) -> String {
    let thousandths = quantity.thousandths();
    let sign = if thousandths < 0 { "-" } else { "" };
    let abs = thousandths.unsigned_abs();
    let whole = abs / 1000;
    let frac = abs % 1000;
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let frac = format!("{frac:03}");
        format!("{sign}{whole}.{}", frac.trim_end_matches('0'))
    }
}

/// Renders a unit rate for an input box: the single euro formatter's digits
/// without the currency sign (`200.00`).
fn format_rate(rate: Money) -> String {
    rate.to_string().trim_end_matches(" €").to_string()
}

/// Parses a typed quantity into thousandths (`2` -> 2000, `2.5` -> 2500).
/// Accepts `.` or `,` as the decimal separator and at most three decimals.
fn parse_quantity(text: &str) -> Option<Quantity> {
    parse_decimal(text, 3).map(Quantity::from_thousandths)
}

/// Parses a typed EUR amount into cents (`200` -> 20000, `12.5` -> 1250).
/// Accepts `.` or `,` as the decimal separator and at most two decimals.
fn parse_money(text: &str) -> Option<Money> {
    parse_decimal(text, 2).map(Money::from_cents)
}

/// Parses a typed VAT percentage into basis points (`19` -> 1900, `7.5` -> 750).
/// Accepts `.` or `,` as the decimal separator, at most two decimals, an
/// optional trailing `%`, and rates from 0% to 100% inclusive.
fn parse_vat_rate(text: &str) -> Option<VatRate> {
    let trimmed = text.trim();
    let number = trimmed.strip_suffix('%').unwrap_or(trimmed).trim_end();
    let bps = parse_decimal(number, 2)?;
    if bps > 10_000 {
        return None;
    }
    Some(VatRate::from_basis_points(bps as u32))
}

/// Parses a non-negative decimal into an integer with `places` implied decimal
/// digits, without floating point. Returns `None` for malformed input.
fn parse_decimal(text: &str, places: usize) -> Option<i64> {
    let normalized = text.trim().replace(',', ".");
    let (whole, frac) = normalized
        .split_once('.')
        .unwrap_or((normalized.as_str(), ""));
    if whole.is_empty() && frac.is_empty() {
        return None;
    }
    if !whole.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if frac.len() > places {
        return None;
    }
    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let frac: i64 = format!("{frac:0<places$}").parse().ok()?;
    whole
        .checked_mul(10_i64.checked_pow(places as u32)?)?
        .checked_add(frac)
}

/// Renders a VAT rate in basis points as a percentage (`1900` is `19`, `750` is `7.5`).
fn format_percent(rate: VatRate) -> String {
    let bps = rate.basis_points();
    let whole = bps / 100;
    let frac = bps % 100;
    if frac == 0 {
        whole.to_string()
    } else {
        let frac = format!("{frac:02}");
        format!("{whole}.{}", frac.trim_end_matches('0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bank, LineItem, Money, SECTION_19_NOTE, Sender, SenderLayout};

    fn reference_identity() -> Identity {
        Identity {
            sender: Sender {
                name: "Alexandra Morgan".to_string(),
                street: "Whitfield. 42".to_string(),
                postal_code_city: "10115 Musterstadt".to_string(),
                country: "Germany".to_string(),
                ..Sender::default()
            },
            bank: Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: "DE89 3704 0044 0532 0130 00".to_string(),
            },
        }
    }

    // --- (a) ordered panel list ---

    #[test]
    fn panel_order_is_the_seven_spec_panels_in_sequence() {
        let titles: Vec<&str> = PANEL_ORDER.iter().map(|p| p.title()).collect();
        assert_eq!(
            titles,
            vec![
                "Sender",
                "Bill to",
                "Invoice details",
                "Line items",
                "Totals",
                "Notes",
                "Bank details",
            ]
        );
    }

    // --- (b) first run: Launching, empty identity ---

    #[test]
    fn first_run_default_identity_yields_empty_sender_and_bank() {
        let state = AppState::new(Identity::default());
        assert_eq!(state.invoice.sender, Sender::default());
        assert_eq!(state.invoice.bank, Bank::default());
    }

    #[test]
    fn first_run_default_identity_leaves_content_empty() {
        let state = AppState::new(Identity::default());
        assert_eq!(state.invoice, Invoice::default());
    }

    #[test]
    fn first_run_sender_and_bank_panels_show_blank_values() {
        let state = AppState::new(Identity::default());
        assert_eq!(
            state.panel_lines(Panel::Sender),
            vec!["Name: ", "Street: ", "Postal code/City: ", "Country: ",]
        );
        assert_eq!(
            state.panel_lines(Panel::BankDetails),
            vec!["Bank: ", "Beneficiary: ", "IBAN: "]
        );
    }

    // --- (c) second run: Resuming, pre-filled identity ---

    #[test]
    fn populated_identity_prefills_sender_and_bank() {
        let identity = reference_identity();
        let state = AppState::new(identity.clone());
        assert_eq!(state.invoice.sender, identity.sender);
        assert_eq!(state.invoice.bank, identity.bank);
    }

    #[test]
    fn populated_identity_keeps_all_invoice_content_empty() {
        let state = AppState::new(reference_identity());
        assert_eq!(state.invoice.client, crate::model::Client::default());
        assert_eq!(state.invoice.meta, crate::model::InvoiceMeta::default());
        assert!(state.invoice.line_items.is_empty());
        assert_eq!(state.invoice.vat_rate, VatRate::ZERO);
        assert!(state.invoice.notes.is_empty());
    }

    #[test]
    fn prefilled_sender_panel_shows_saved_values() {
        let state = AppState::new(reference_identity());
        assert_eq!(
            state.panel_lines(Panel::Sender),
            vec![
                "Name: Alexandra Morgan",
                "Street: Whitfield. 42",
                "Postal code/City: 10115 Musterstadt",
                "Country: Germany",
            ]
        );
    }

    #[test]
    fn prefilled_bank_panel_shows_saved_values() {
        let state = AppState::new(reference_identity());
        assert_eq!(
            state.panel_lines(Panel::BankDetails),
            vec![
                "Bank: Ellery",
                "Beneficiary: Penhallow Morgan",
                "IBAN: DE89 3704 0044 0532 0130 00",
            ]
        );
    }

    // --- Display of the other panels from current model values ---

    #[test]
    fn empty_line_items_panel_shows_placeholder() {
        let state = AppState::new(Identity::default());
        assert_eq!(
            state.panel_lines(Panel::LineItems),
            vec!["No line items yet."]
        );
    }

    #[test]
    fn line_items_panel_shows_row_qty_rate_and_total() {
        let mut state = AppState::new(Identity::default());
        state.invoice.line_items.push(LineItem {
            description: "Consulting".to_string(),
            quantity: Quantity::from_thousandths(2_500),
            rate: Money::from_cents(10_000),
        });
        assert_eq!(
            state.panel_lines(Panel::LineItems),
            vec!["Consulting: 2.5 x 100.00 € = 250.00 €"]
        );
    }

    #[test]
    fn totals_panel_shows_net_vat_and_gross_at_zero_percent() {
        let mut state = AppState::new(Identity::default());
        state.invoice.line_items.push(LineItem {
            description: String::new(),
            quantity: Quantity::whole(1),
            rate: Money::from_euros(200),
        });
        assert_eq!(
            state.panel_lines(Panel::Totals),
            vec!["Net: 200.00 €", "VAT (0%): 0.00 €", "Gross: 200.00 €"]
        );
    }

    #[test]
    fn totals_panel_shows_fractional_vat_percentage() {
        let mut state = AppState::new(Identity::default());
        state.invoice.vat_rate = VatRate::from_basis_points(750);
        assert_eq!(state.panel_lines(Panel::Totals)[1], "VAT (7.5%): 0.00 €");
    }

    #[test]
    fn notes_panel_shows_placeholder_then_verbatim_text() {
        let mut state = AppState::new(Identity::default());
        assert_eq!(state.panel_lines(Panel::Notes), vec!["No notes."]);
        state.invoice.notes = "Payment within 14 days".to_string();
        assert_eq!(
            state.panel_lines(Panel::Notes),
            vec!["Payment within 14 days"]
        );
    }

    // --- (d) simple text panels two-way bound to the model (task 5.2) ---

    const ALL_FIELDS: [Field; 19] = [
        Field::SenderName,
        Field::SenderStreet,
        Field::SenderPostalCodeCity,
        Field::SenderCountry,
        Field::SenderTaxNumber,
        Field::SenderVatId,
        Field::SenderEmail,
        Field::ClientName,
        Field::ClientStreet,
        Field::ClientPostalCodeCity,
        Field::ClientCountry,
        Field::InvoiceNumber,
        Field::InvoiceDate,
        Field::ServiceDate,
        Field::DueDate,
        Field::Notes,
        Field::BankName,
        Field::Beneficiary,
        Field::Iban,
    ];

    // --- Bank details with IBAN validation (task 7.1) ---

    const REFERENCE_IBAN: &str = "DE89 3704 0044 0532 0130 00";

    #[test]
    fn editing_bank_fields_updates_the_bank_model() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::BankName, "Ellery");
        state.set_field(Field::Beneficiary, "Penhallow Morgan");
        state.set_field(Field::Iban, REFERENCE_IBAN);
        assert_eq!(
            state.invoice.bank,
            Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: REFERENCE_IBAN.to_string(),
            }
        );
        assert_eq!(state.field_value(Field::Iban), REFERENCE_IBAN);
    }

    #[test]
    fn valid_iban_shows_grouped_with_no_error_and_is_retained_as_default() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::Iban, "DE89370400440532013000");

        assert_eq!(state.iban_error(), None);
        assert_eq!(state.iban_grouped(), REFERENCE_IBAN);

        let identity = state.identity();
        assert_eq!(identity.bank.iban, "DE89370400440532013000");
        assert_eq!(identity.sender, state.invoice.sender);
    }

    #[test]
    fn invalid_iban_shows_inline_checksum_error() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::Iban, "DE59 3704 0044 0532 0130 00");

        let error = state.iban_error().expect("bad checksum is an error");
        assert_eq!(error, IbanError::ChecksumFailed);
        assert_eq!(error.to_string(), "IBAN checksum is invalid");
    }

    #[test]
    fn malformed_iban_shows_inline_format_error() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::Iban, "DE5");
        assert_eq!(state.iban_error(), Some(IbanError::TooShort));

        state.set_field(Field::Iban, "1E59 3704 0044 0532 0130 00");
        assert_eq!(state.iban_error(), Some(IbanError::InvalidCountryCode));
    }

    #[test]
    fn empty_iban_shows_no_error_on_first_run() {
        let state = AppState::new(Identity::default());
        assert_eq!(state.iban_error(), None);
        assert_eq!(state.iban_grouped(), "");
    }

    #[test]
    fn prefilled_valid_iban_is_grouped_and_error_free() {
        let state = AppState::new(reference_identity());
        assert_eq!(state.iban_error(), None);
        assert_eq!(state.iban_grouped(), REFERENCE_IBAN);
    }

    #[test]
    fn every_field_reads_back_what_was_set() {
        let mut state = AppState::new(Identity::default());
        for (i, field) in ALL_FIELDS.iter().enumerate() {
            let value = format!("value-{i}");
            state.set_field(*field, value.clone());
            assert_eq!(state.field_value(*field), value);
        }
    }

    #[test]
    fn setting_one_field_leaves_every_other_field_unchanged() {
        let mut state = AppState::new(reference_identity());
        let before = state.invoice.clone();
        state.set_field(Field::ClientName, "Ostrander Quennell");
        let mut expected = before;
        expected.client.name = "Ostrander Quennell".to_string();
        assert_eq!(state.invoice, expected);
    }

    #[test]
    fn editing_sender_fields_renders_the_return_line() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::SenderName, "Alexandra Morgan");
        state.set_field(Field::SenderStreet, "Whitfield. 42");
        state.set_field(Field::SenderPostalCodeCity, "10115 Musterstadt");
        state.set_field(Field::SenderCountry, "Germany");

        assert_eq!(
            state.return_line(),
            "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt"
        );
        assert_eq!(
            state.panel_lines(Panel::Sender),
            vec![
                "Name: Alexandra Morgan",
                "Street: Whitfield. 42",
                "Postal code/City: 10115 Musterstadt",
                "Country: Germany",
            ]
        );
    }

    #[test]
    fn editing_bill_to_fields_updates_the_client() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::ClientName, "Ostrander Quennell");
        state.set_field(Field::ClientStreet, "Rayburn Str. 42");
        state.set_field(Field::ClientPostalCodeCity, "80331 Musterstadt");
        state.set_field(Field::ClientCountry, "Germany");

        assert_eq!(
            state.invoice.client,
            crate::model::Client {
                name: "Ostrander Quennell".to_string(),
                street: "Rayburn Str. 42".to_string(),
                postal_code_city: "80331 Musterstadt".to_string(),
                country: "Germany".to_string(),
            }
        );
        assert_eq!(
            state.panel_lines(Panel::BillTo),
            vec![
                "Name: Ostrander Quennell",
                "Street: Rayburn Str. 42",
                "Postal code/City: 80331 Musterstadt",
                "Country: Germany",
            ]
        );
    }

    #[test]
    fn editing_invoice_number_and_dates_updates_the_meta() {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::InvoiceNumber, "4");
        state.set_field(Field::InvoiceDate, "03/10/2026");
        state.set_field(Field::ServiceDate, "11/09/2026");
        state.set_field(Field::DueDate, "15/10/2026");

        assert_eq!(
            state.invoice.meta,
            crate::model::InvoiceMeta {
                number: "4".to_string(),
                invoice_date: "03/10/2026".to_string(),
                service_date: "11/09/2026".to_string(),
                due_date: "15/10/2026".to_string(),
            }
        );
        assert_eq!(
            state.panel_lines(Panel::InvoiceDetails),
            vec![
                "Invoice number: 4",
                "Invoice date: 03/10/2026",
                "Service date: 11/09/2026",
                "Due date: 15/10/2026",
            ]
        );
    }

    #[test]
    fn editing_notes_sets_invoice_notes_and_never_seeds_section_19() {
        let mut state = AppState::new(Identity::default());
        assert!(state.invoice.notes.is_empty(), "notes start empty");

        state.set_field(Field::Notes, "Payment within 14 days");
        assert_eq!(state.invoice.notes, "Payment within 14 days");
        assert_eq!(
            state.panel_lines(Panel::Notes),
            vec!["Payment within 14 days"]
        );

        // Clearing the field returns to empty; the §19 text is never inserted
        // into the free-text notes (it is the separate standing note).
        state.set_field(Field::Notes, "");
        assert!(state.invoice.notes.is_empty());
        assert_eq!(state.panel_lines(Panel::Notes), vec!["No notes."]);
    }

    #[test]
    fn format_quantity_trims_trailing_zeros() {
        assert_eq!(format_quantity(Quantity::whole(3)), "3");
        assert_eq!(format_quantity(Quantity::from_thousandths(1_500)), "1.5");
        assert_eq!(format_quantity(Quantity::from_thousandths(5)), "0.005");
    }

    // --- Line items: Adding / Removing / Line-total (task 6.1) ---

    fn set_row(state: &mut AppState, index: usize, field: LineItemField, text: &str) {
        assert!(state.set_line_item_text(index, field, text), "set {text:?}");
    }

    #[test]
    fn add_line_item_appends_an_editable_empty_row() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        assert_eq!(index, 0);
        assert_eq!(state.invoice.line_items.len(), 1);
        assert_eq!(state.invoice.line_items[0], LineItem::default());
        assert_eq!(
            state
                .line_item_text(0, LineItemField::Description)
                .as_deref(),
            Some("")
        );
        assert_eq!(
            state.line_item_text(0, LineItemField::Quantity).as_deref(),
            Some("0")
        );
        assert_eq!(
            state.line_item_text(0, LineItemField::Rate).as_deref(),
            Some("0.00")
        );

        // The new row accepts edits like any other row.
        set_row(&mut state, 0, LineItemField::Description, "Consulting");
        set_row(&mut state, 0, LineItemField::Quantity, "2");
        set_row(&mut state, 0, LineItemField::Rate, "100");
        assert_eq!(
            state.panel_lines(Panel::LineItems),
            vec!["Consulting: 2 x 100.00 € = 200.00 €"]
        );
    }

    #[test]
    fn add_line_item_appends_after_existing_rows() {
        let mut state = AppState::new(Identity::default());
        state.add_line_item();
        set_row(&mut state, 0, LineItemField::Description, "First");
        let index = state.add_line_item();
        assert_eq!(index, 1);
        assert_eq!(
            state
                .line_item_text(0, LineItemField::Description)
                .as_deref(),
            Some("First")
        );
        assert_eq!(
            state
                .line_item_text(1, LineItemField::Description)
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn remove_line_item_drops_a_middle_row_and_keeps_the_others_in_order() {
        let mut state = AppState::new(Identity::default());
        for (i, name) in ["A", "B", "C"].iter().enumerate() {
            let index = state.add_line_item();
            assert_eq!(index, i);
            set_row(&mut state, index, LineItemField::Description, name);
        }

        let removed = state.remove_line_item(1).expect("row 1 exists");
        assert_eq!(removed.description, "B");

        assert_eq!(state.invoice.line_items.len(), 2);
        assert_eq!(
            state
                .line_item_text(0, LineItemField::Description)
                .as_deref(),
            Some("A")
        );
        assert_eq!(
            state
                .line_item_text(1, LineItemField::Description)
                .as_deref(),
            Some("C")
        );
        assert_eq!(state.line_item_text(2, LineItemField::Description), None);
    }

    #[test]
    fn remove_line_item_out_of_range_changes_nothing() {
        let mut state = AppState::new(Identity::default());
        state.add_line_item();
        let before = state.clone();
        assert_eq!(state.remove_line_item(5), None);
        assert_eq!(state.invoice, before.invoice);
    }

    #[test]
    fn removed_row_drops_from_the_table_and_the_pdf_projection() {
        let mut state = AppState::new(Identity::default());
        for name in ["A", "B"] {
            let index = state.add_line_item();
            set_row(&mut state, index, LineItemField::Description, name);
        }
        assert_eq!(crate::pdf::project(&state.invoice).line_items.len(), 2);

        state.remove_line_item(0);
        let projection = crate::pdf::project(&state.invoice);
        assert_eq!(projection.line_items.len(), 1);
        assert_eq!(projection.line_items[0].description, "B");
        assert_eq!(state.panel_lines(Panel::LineItems).len(), 1);
    }

    #[test]
    fn row_total_recomputes_after_a_quantity_change() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Rate, "100");
        set_row(&mut state, index, LineItemField::Quantity, "1");
        assert_eq!(state.line_item_total(index), Some(Money::from_euros(100)));

        set_row(&mut state, index, LineItemField::Quantity, "2.5");
        assert_eq!(state.line_item_total(index), Some(Money::from_euros(250)));
        assert_eq!(
            state
                .line_item_text(index, LineItemField::Quantity)
                .as_deref(),
            Some("2.5")
        );
    }

    #[test]
    fn row_total_recomputes_after_a_rate_change() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Quantity, "2");
        set_row(&mut state, index, LineItemField::Rate, "12.34");
        assert_eq!(state.line_item_total(index), Some(Money::from_cents(2_468)));
        assert_eq!(
            state.line_item_text(index, LineItemField::Rate).as_deref(),
            Some("12.34")
        );
    }

    #[test]
    fn row_total_is_quantity_times_rate_and_follows_the_invoice_net() {
        let mut state = AppState::new(Identity::default());
        for (qty, rate) in [("2", "50"), ("1.5", "100")] {
            let index = state.add_line_item();
            set_row(&mut state, index, LineItemField::Quantity, qty);
            set_row(&mut state, index, LineItemField::Rate, rate);
        }
        assert_eq!(state.line_item_total(0), Some(Money::from_euros(100)));
        assert_eq!(state.line_item_total(1), Some(Money::from_euros(150)));
        assert_eq!(state.invoice.totals().net, Money::from_euros(250));
        assert_eq!(state.line_item_total(2), None);
    }

    #[test]
    fn unparseable_quantity_or_rate_leaves_the_model_and_total_unchanged() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Quantity, "2");
        set_row(&mut state, index, LineItemField::Rate, "10");
        let before = state.invoice.line_items[index].clone();

        assert!(!state.set_line_item_text(index, LineItemField::Quantity, "2,5,1"));
        assert!(!state.set_line_item_text(index, LineItemField::Rate, "abc"));
        assert_eq!(state.invoice.line_items[index], before);
        assert_eq!(state.line_item_total(index), Some(Money::from_euros(20)));
    }

    #[test]
    fn setting_a_line_item_on_an_unknown_row_is_rejected() {
        let mut state = AppState::new(Identity::default());
        assert!(!state.set_line_item_text(0, LineItemField::Description, "x"));
        assert!(state.invoice.line_items.is_empty());
    }

    #[test]
    fn parse_decimal_accepts_point_or_comma_and_rejects_malformed_text() {
        assert_eq!(parse_quantity("2"), Some(Quantity::whole(2)));
        assert_eq!(
            parse_quantity("2,5"),
            Some(Quantity::from_thousandths(2_500))
        );
        assert_eq!(parse_quantity("0.005"), Some(Quantity::from_thousandths(5)));
        assert_eq!(parse_quantity(".5"), Some(Quantity::from_thousandths(500)));
        assert_eq!(parse_quantity("5."), Some(Quantity::whole(5)));
        assert_eq!(parse_quantity("1.2345"), None);
        assert_eq!(parse_quantity(""), None);
        assert_eq!(parse_quantity("."), None);
        assert_eq!(parse_quantity("-1"), None);
        assert_eq!(parse_quantity("1e3"), None);

        assert_eq!(parse_money("200"), Some(Money::from_euros(200)));
        assert_eq!(parse_money("12.5"), Some(Money::from_cents(1_250)));
        assert_eq!(parse_money("0.05"), Some(Money::from_cents(5)));
        assert_eq!(parse_money("0.005"), None);
        assert_eq!(parse_money("12..5"), None);
    }

    #[test]
    fn line_item_text_renders_rate_without_currency_sign() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        state.invoice.line_items[index].rate = Money::from_cents(10_000);
        assert_eq!(
            state.line_item_text(index, LineItemField::Rate).as_deref(),
            Some("100.00")
        );
    }

    // --- Totals and VAT rate (task 6.2) ---

    fn reference_row_state(vat_text: &str) -> AppState {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Quantity, "1");
        set_row(&mut state, index, LineItemField::Rate, "200");
        assert!(state.set_vat_rate_text(vat_text), "set VAT {vat_text:?}");
        state
    }

    #[test]
    fn vat_default_is_zero_percent_with_note_shown_and_gross_equal_to_net() {
        let mut state = AppState::new(Identity::default());
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Quantity, "1");
        set_row(&mut state, index, LineItemField::Rate, "200");

        assert_eq!(state.invoice.vat_rate, VatRate::ZERO);
        assert_eq!(state.vat_rate_text(), "0");
        assert_eq!(state.standing_note(), Some(SECTION_19_NOTE));

        let totals = state.invoice.totals();
        assert_eq!(totals.net, Money::from_euros(200));
        assert_eq!(totals.vat, Money::ZERO);
        assert_eq!(totals.gross, totals.net);
        assert_eq!(
            state.panel_lines(Panel::Totals),
            vec!["Net: 200.00 €", "VAT (0%): 0.00 €", "Gross: 200.00 €"]
        );
    }

    #[test]
    fn nonzero_vat_recomputes_vat_and_gross_and_hides_the_note() {
        let state = reference_row_state("19");

        assert_eq!(state.invoice.vat_rate, VatRate::from_percent(19));
        assert_eq!(state.vat_rate_text(), "19");
        assert_eq!(state.standing_note(), None);

        let totals = state.invoice.totals();
        assert_eq!(totals.net, Money::from_euros(200));
        assert_eq!(totals.vat, Money::from_euros(38));
        assert_eq!(totals.gross, Money::from_euros(238));
        assert_eq!(
            state.panel_lines(Panel::Totals),
            vec!["Net: 200.00 €", "VAT (19%): 38.00 €", "Gross: 238.00 €"]
        );
    }

    #[test]
    fn fractional_vat_rate_round_trips_through_its_text() {
        let mut state = AppState::new(Identity::default());
        assert!(state.set_vat_rate_text("7.5"));
        assert_eq!(state.invoice.vat_rate, VatRate::from_basis_points(750));
        assert_eq!(state.vat_rate_text(), "7.5");
        assert!(state.set_vat_rate_text("0,01"));
        assert_eq!(state.invoice.vat_rate, VatRate::from_basis_points(1));
        assert_eq!(state.vat_rate_text(), "0.01");
    }

    #[test]
    fn vat_rate_accepts_trailing_percent_sign_and_full_range() {
        assert_eq!(parse_vat_rate("19%"), Some(VatRate::from_percent(19)));
        assert_eq!(parse_vat_rate(" 19 % "), Some(VatRate::from_percent(19)));
        assert_eq!(parse_vat_rate("0"), Some(VatRate::ZERO));
        assert_eq!(parse_vat_rate("100"), Some(VatRate::from_percent(100)));
        assert_eq!(parse_vat_rate("100.01"), None);
        assert_eq!(parse_vat_rate("150"), None);
    }

    #[test]
    fn unparseable_vat_input_leaves_the_model_unchanged() {
        let mut state = reference_row_state("19");
        let before = state.invoice.clone();

        for text in ["", "%", "abc", "19,5,1", "1e3", "-5", "0.005", "100.01"] {
            assert!(!state.set_vat_rate_text(text), "rejects {text:?}");
            assert_eq!(state.invoice, before, "model untouched by {text:?}");
        }
        assert_eq!(state.invoice.totals().vat, Money::from_euros(38));
    }

    #[test]
    fn changing_a_line_item_recomputes_net_vat_and_gross_at_nonzero_rate() {
        let mut state = reference_row_state("19");

        // Rate change on the reference row.
        set_row(&mut state, 0, LineItemField::Rate, "100");
        assert_eq!(
            state.invoice.totals(),
            crate::model::Totals {
                net: Money::from_euros(100),
                vat: Money::from_euros(19),
                gross: Money::from_euros(119),
            }
        );

        // Quantity change on the reference row.
        set_row(&mut state, 0, LineItemField::Quantity, "2.5");
        assert_eq!(state.invoice.totals().net, Money::from_euros(250));
        assert_eq!(state.invoice.totals().vat, Money::from_cents(4_750));
        assert_eq!(state.invoice.totals().gross, Money::from_cents(29_750));

        // Adding a row.
        let index = state.add_line_item();
        set_row(&mut state, index, LineItemField::Quantity, "1");
        set_row(&mut state, index, LineItemField::Rate, "50");
        assert_eq!(state.invoice.totals().net, Money::from_euros(300));
        assert_eq!(state.invoice.totals().vat, Money::from_euros(57));
        assert_eq!(state.invoice.totals().gross, Money::from_euros(357));

        // Removing a row.
        state.remove_line_item(0);
        assert_eq!(state.invoice.totals().net, Money::from_euros(50));
        assert_eq!(state.invoice.totals().vat, Money::from_cents(950));
        assert_eq!(state.invoice.totals().gross, Money::from_cents(5_950));
        assert_eq!(
            state.panel_lines(Panel::Totals),
            vec!["Net: 50.00 €", "VAT (19%): 9.50 €", "Gross: 59.50 €"]
        );
    }

    #[test]
    fn vat_change_recomputes_totals_and_note_for_existing_rows() {
        let mut state = reference_row_state("0");
        assert_eq!(state.standing_note(), Some(SECTION_19_NOTE));

        assert!(state.set_vat_rate_text("19"));
        assert_eq!(state.invoice.totals().gross, Money::from_euros(238));
        assert_eq!(state.standing_note(), None);

        assert!(state.set_vat_rate_text("0"));
        assert_eq!(state.invoice.totals().vat, Money::ZERO);
        assert_eq!(state.invoice.totals().gross, state.invoice.totals().net);
        assert_eq!(state.standing_note(), Some(SECTION_19_NOTE));
    }

    #[test]
    fn section_19_note_is_single_sourced_from_the_model_rule() {
        // Shown at 0% by the model, hidden above 0%.
        let zero = reference_row_state("0");
        assert_eq!(zero.standing_note(), zero.invoice.section_19_note());
        assert_eq!(
            crate::pdf::project(&zero.invoice).standing_note,
            zero.standing_note()
        );

        let nineteen = reference_row_state("19");
        assert_eq!(nineteen.standing_note(), None);
        assert_eq!(crate::pdf::project(&nineteen.invoice).standing_note, None);

        // The standing note never leaks into the free-text Notes panel.
        assert!(
            zero.panel_lines(Panel::Notes)
                .iter()
                .all(|line| !line.contains(SECTION_19_NOTE))
        );
    }

    #[test]
    fn vat_rate_flows_into_the_pdf_projection_totals_and_note() {
        let state = reference_row_state("19");
        let projection = crate::pdf::project(&state.invoice);
        let labels: Vec<(&str, &str)> = projection
            .totals
            .iter()
            .map(|t| (t.label.as_str(), t.amount.as_str()))
            .collect();
        assert_eq!(
            labels,
            vec![
                ("Total amount (Net)", "200.00 €"),
                ("VAT (19%)", "38.00 €"),
                ("Total amount (Gross)", "238.00 €"),
            ]
        );
        assert_eq!(projection.standing_note, None);
    }

    // --- Identity save-on-edit (task 7.2) ---

    /// A unique scratch dir under the OS temp dir, removed on drop. These tests
    /// never touch the real per-user config directory.
    struct ScratchDir(std::path::PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            ScratchDir(std::env::temp_dir().join(format!(
                "billing-appstate-{label}-{}-{nanos}",
                std::process::id()
            )))
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn editing_sender_and_bank_writes_the_store_and_reload_restores_them() {
        let scratch = ScratchDir::new("save");
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::SenderName, "Alexandra Morgan");
        state.set_field(Field::SenderStreet, "Whitfield. 42");
        state.set_field(Field::SenderPostalCodeCity, "10115 Musterstadt");
        state.set_field(Field::SenderCountry, "Germany");
        state.set_field(Field::BankName, "Ellery");
        state.set_field(Field::Beneficiary, "Penhallow Morgan");
        state.set_field(Field::Iban, REFERENCE_IBAN);

        let outcome = state
            .persist_identity_to(&scratch.0)
            .expect("save should succeed");
        assert_eq!(outcome, PersistOutcome::Saved);

        // A later launch loads the store and pre-fills the editor from it.
        let reloaded =
            AppState::new(identity_store::load_from(&scratch.0).expect("load should succeed"));
        assert_eq!(reloaded.identity(), state.identity());
        assert_eq!(
            reloaded.field_value(Field::SenderName),
            "Alexandra Morgan"
        );
        assert_eq!(reloaded.field_value(Field::SenderCountry), "Germany");
        assert_eq!(reloaded.field_value(Field::BankName), "Ellery");
        assert_eq!(
            reloaded.field_value(Field::Beneficiary),
            "Penhallow Morgan"
        );
        assert_eq!(reloaded.field_value(Field::Iban), REFERENCE_IBAN);
        assert_eq!(reloaded.iban_error(), None);
    }

    #[test]
    fn each_new_sender_field_updates_invoice_sender_and_identity() {
        let cases = [
            (Field::SenderTaxNumber, "DE 123/456/78901"),
            (Field::SenderVatId, "DE123456789"),
            (Field::SenderEmail, "Alexandra@example.com"),
        ];
        for (field, value) in cases {
            let mut state = AppState::new(Identity::default());
            state.set_field(field, value);

            assert_eq!(state.field_value(field), value);
            let sender = &state.invoice.sender;
            let expected = match field {
                Field::SenderTaxNumber => Sender {
                    tax_number: value.to_string(),
                    ..Sender::default()
                },
                Field::SenderVatId => Sender {
                    vat_id: value.to_string(),
                    ..Sender::default()
                },
                _ => Sender {
                    email: value.to_string(),
                    ..Sender::default()
                },
            };
            assert_eq!(*sender, expected, "{field:?} reaches Invoice.sender");
            assert_eq!(
                state.identity().sender,
                expected,
                "{field:?} reaches identity()"
            );
        }
    }

    #[test]
    fn set_sender_layout_updates_invoice_sender_and_identity() {
        let mut state = AppState::new(Identity::default());
        assert_eq!(state.invoice.sender.layout, SenderLayout::Letter);

        for layout in [
            SenderLayout::Header,
            SenderLayout::Footer,
            SenderLayout::Letter,
        ] {
            state.set_sender_layout(layout);
            assert_eq!(state.invoice.sender.layout, layout);
            assert_eq!(state.identity().sender.layout, layout);
        }
    }

    #[test]
    fn invalid_iban_on_first_run_saves_sender_and_an_empty_bank() {
        let scratch = ScratchDir::new("invalid-first");
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::SenderName, "Alexandra Morgan");
        state.set_field(Field::BankName, "Ellery");
        state.set_field(Field::Iban, "DE59 3704 0044 0532 0130 00");

        let outcome = state
            .persist_identity_to(&scratch.0)
            .expect("saving the sender is not an error");
        assert_eq!(outcome, PersistOutcome::SavedSenderOnly);

        let stored = identity_store::load_from(&scratch.0).expect("load");
        assert_eq!(stored.sender.name, "Alexandra Morgan");
        assert_eq!(stored.bank, Bank::default());
    }

    #[test]
    fn invalid_iban_saves_the_sender_and_keeps_the_last_valid_bank() {
        let scratch = ScratchDir::new("invalid-later");
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::SenderName, "Alexandra Morgan");
        state.set_field(Field::BankName, "Ellery");
        state.set_field(Field::Beneficiary, "Penhallow Morgan");
        state.set_field(Field::Iban, REFERENCE_IBAN);
        state
            .persist_identity_to(&scratch.0)
            .expect("first save succeeds");
        let saved_bank = identity_store::load_from(&scratch.0).expect("load").bank;

        // Edit the sender and the bank fields; the IBAN is now invalid.
        state.set_field(Field::SenderName, "Someone Else");
        state.set_field(Field::BankName, "Wise");
        state.set_field(Field::Iban, "DE59 3704 0044 0532 0130 00");
        let outcome = state
            .persist_identity_to(&scratch.0)
            .expect("saving the sender is not an error");

        assert_eq!(outcome, PersistOutcome::SavedSenderOnly);
        let stored = identity_store::load_from(&scratch.0).expect("load");
        assert_eq!(stored.sender.name, "Someone Else", "the sender is retained");
        assert_eq!(stored.bank, saved_bank, "the last valid bank is unchanged");
        assert_eq!(stored.bank.iban, REFERENCE_IBAN);
        assert!(iban::validate(&stored.bank.iban).is_ok());
    }

    #[test]
    fn empty_iban_is_saved_because_it_is_not_an_error() {
        let scratch = ScratchDir::new("empty-iban");
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::SenderName, "Alexandra Morgan");

        let outcome = state
            .persist_identity_to(&scratch.0)
            .expect("save should succeed");
        assert_eq!(outcome, PersistOutcome::Saved);
        assert_eq!(
            identity_store::load_from(&scratch.0)
                .expect("load")
                .sender
                .name,
            "Alexandra Morgan"
        );
    }
}

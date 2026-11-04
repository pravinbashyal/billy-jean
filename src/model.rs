//! Invoice domain model and pure total calculations (design D4, D5, D6).
//!
//! Representation choices (no binary floating point anywhere):
//! - [`Money`] is an integer count of EUR cents (`i64`). Currency is fixed to EUR.
//! - [`Quantity`] is an integer count of thousandths (`i64`), so fractional
//!   quantities such as 2.5 are exact. `2.5` is stored as `2500`.
//! - [`VatRate`] is an integer count of basis points (`u32`). `0%` is `0`,
//!   `19%` is `1900`, and `7.5%` is `750`.
//!
//! Rounding rule: every product is computed exactly in `i128` and then rounded
//! to the nearest cent, with ties rounded half away from zero.
//! - Line total = round(Quantity_thousandths x Rate_cents / 1000)
//! - Net        = sum of line totals (already whole cents, so exact)
//! - VAT        = round(Net_cents x VatRate_bps / 10000)
//! - Gross      = Net + VAT

use std::iter::Sum;
use std::ops::{Add, AddAssign, Sub};

use serde::{Deserialize, Serialize};

/// Divides `numerator` by `denominator` (must be positive), rounding to the
/// nearest integer with ties rounded away from zero.
fn div_round_half_away(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(denominator > 0);
    let quotient = numerator / denominator; // truncates toward zero
    let remainder = numerator % denominator; // same sign as numerator
    if 2 * remainder.abs() >= denominator {
        if numerator < 0 {
            quotient - 1
        } else {
            quotient + 1
        }
    } else {
        quotient
    }
}

/// An amount of money in EUR, stored as integer cents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    /// Creates an amount from a whole number of EUR cents.
    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    /// Creates an amount from a whole number of EUR.
    pub const fn from_euros(euros: i64) -> Self {
        Money(euros * 100)
    }

    /// Returns the amount in cents.
    pub const fn cents(self) -> i64 {
        self.0
    }
}

/// The single EUR formatter for every amount shown in the app and the PDF
/// (carry-forward: euro formatting is single-sourced here). Renders cents as
/// two decimals followed by the euro sign, e.g. `200.00 €`, `0.00 €`, `-1.50 €`.
impl std::fmt::Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02} €", abs / 100, abs % 100)
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        Money(self.0 + rhs.0)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        Money(self.0 - rhs.0)
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        self.0 += rhs.0;
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, |acc, m| acc + m)
    }
}

/// A line-item quantity, stored as integer thousandths so fractional
/// quantities are exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct Quantity(i64);

impl Quantity {
    /// Creates a quantity from a count of thousandths (`2.5` is `2500`).
    pub const fn from_thousandths(thousandths: i64) -> Self {
        Quantity(thousandths)
    }

    /// Creates a whole-number quantity.
    pub const fn whole(n: i64) -> Self {
        Quantity(n * 1000)
    }

    /// Returns the quantity in thousandths.
    pub const fn thousandths(self) -> i64 {
        self.0
    }
}

/// A VAT rate, stored as integer basis points (1 bp = 0.01%).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct VatRate(u32);

impl VatRate {
    /// The 0% rate, the default under the Kleinunternehmer (§19 UStG) scheme.
    pub const ZERO: VatRate = VatRate(0);

    /// Creates a rate from basis points (`1900` is 19%).
    pub const fn from_basis_points(bps: u32) -> Self {
        VatRate(bps)
    }

    /// Creates a rate from a whole percentage (`19` is 19%).
    pub const fn from_percent(percent: u32) -> Self {
        VatRate(percent * 100)
    }

    /// Returns the rate in basis points.
    pub const fn basis_points(self) -> u32 {
        self.0
    }

    /// True when the rate is 0%.
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

/// One row of the Line items table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineItem {
    pub description: String,
    pub quantity: Quantity,
    /// Unit rate in EUR.
    pub rate: Money,
}

impl LineItem {
    /// Row Total = Quantity x Rate, rounded to the nearest cent (ties away from zero).
    pub fn total(&self) -> Money {
        let product = self.quantity.thousandths() as i128 * self.rate.cents() as i128;
        Money::from_cents(div_round_half_away(product, 1000) as i64)
    }
}

/// Computed invoice totals, all in EUR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Totals {
    pub net: Money,
    pub vat: Money,
    pub gross: Money,
}

/// Which sender treatment the PDF (and preview) uses (design D5). Serialised
/// lowercase. `Letter` is the default, so identity files written before the
/// layout existed load as German letter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SenderLayout {
    /// German letter: right-aligned sender block with a return line above "Billed to".
    #[default]
    Letter,
    /// Bold header: large sender name at the top left.
    Header,
    /// Footer columns: sender, tax/contact, and bank in three columns after the content.
    Footer,
}

/// Sender identity: who issues the invoice (D5, persisted across runs).
///
/// The tax, VAT ID, email, and layout fields are additive (D5): each has a
/// `serde` default, so identity files from the earlier four-field shape load
/// with those values empty and the layout set to [`SenderLayout::Letter`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Sender {
    pub name: String,
    pub street: String,
    pub postal_code_city: String,
    pub country: String,
    /// German Steuernummer, required for export (checked by the export gate).
    #[serde(default)]
    pub tax_number: String,
    /// VAT identification number (USt-IdNr.). Optional; shown only when filled.
    #[serde(default)]
    pub vat_id: String,
    /// Contact email. Optional; shown only when filled.
    #[serde(default)]
    pub email: String,
    /// The chosen sender treatment.
    #[serde(default)]
    pub layout: SenderLayout,
}

/// Bank identity: where the client pays (D5, persisted across runs).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Bank {
    pub bank_name: String,
    pub beneficiary: String,
    pub iban: String,
}

/// Billed-to party: the client being invoiced (invoice content, not persisted).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Client {
    pub name: String,
    pub street: String,
    pub postal_code_city: String,
    pub country: String,
}

/// Invoice metadata. Dates are free-text DD/MM/YYYY strings at the model
/// layer (D6); parsing and validation are a later task.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InvoiceMeta {
    pub number: String,
    pub invoice_date: String,
    pub service_date: String,
    pub due_date: String,
}

/// The single source of truth for one invoice (D4). Derived totals are
/// computed by [`Invoice::totals`] and are never stored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Invoice {
    pub sender: Sender,
    pub client: Client,
    pub meta: InvoiceMeta,
    pub line_items: Vec<LineItem>,
    pub vat_rate: VatRate,
    pub notes: String,
    pub bank: Bank,
}

impl Invoice {
    /// Net = sum of row totals.
    pub fn net(&self) -> Money {
        self.line_items.iter().map(LineItem::total).sum()
    }

    /// VAT = Net x VAT rate, rounded to the nearest cent (ties away from zero).
    pub fn vat(&self) -> Money {
        let product = self.net().cents() as i128 * self.vat_rate.basis_points() as i128;
        Money::from_cents(div_round_half_away(product, 10_000) as i64)
    }

    /// Gross = Net + VAT.
    pub fn gross(&self) -> Money {
        self.net() + self.vat()
    }

    /// All three totals in one call.
    pub fn totals(&self) -> Totals {
        let net = self.net();
        let vat = self.vat();
        Totals {
            net,
            vat,
            gross: net + vat,
        }
    }

    /// The standing §19 UStG note for this invoice; see [`section_19_note`].
    pub fn section_19_note(&self) -> Option<&'static str> {
        section_19_note(self.vat_rate)
    }
}

/// The standing note shown on Kleinunternehmer invoices (§19 UStG), verbatim
/// as on the source invoice. Shared by the Notes default, the preview, and the
/// PDF so the wording is defined in exactly one place.
pub const SECTION_19_NOTE: &str = "According to § 19 UStG no VAT is charged";

/// Decides whether the §19 UStG note is the standing note for a VAT rate.
///
/// - At 0% the note is present and returns `Some(SECTION_19_NOTE)`.
/// - At any non-zero rate it is suppressed and returns `None`, because a
///   non-zero VAT amount alongside that note is contradictory.
///
/// This is only the standing note. The Notes field is free text and is shown
/// verbatim (spec "Editing notes"); this function does not inspect or rewrite
/// it. The UI seeds the Notes field with [`SECTION_19_NOTE`] at 0%, and the
/// projections render the standing note separately from the custom notes.
pub const fn section_19_note(vat_rate: VatRate) -> Option<&'static str> {
    if vat_rate.is_zero() {
        Some(SECTION_19_NOTE)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(qty: Quantity, rate_cents: i64) -> LineItem {
        LineItem {
            description: String::new(),
            quantity: qty,
            rate: Money::from_cents(rate_cents),
        }
    }

    fn invoice(rows: Vec<LineItem>, vat: VatRate) -> Invoice {
        Invoice {
            line_items: rows,
            vat_rate: vat,
            ..Invoice::default()
        }
    }

    // --- Reference row: 1 x 200 EUR ---

    #[test]
    fn reference_row_total_is_200_eur() {
        let item = row(Quantity::whole(1), 20_000);
        assert_eq!(item.total(), Money::from_euros(200));
    }

    #[test]
    fn reference_row_at_zero_percent_net_200_vat_0_gross_200() {
        let inv = invoice(vec![row(Quantity::whole(1), 20_000)], VatRate::ZERO);
        let t = inv.totals();
        assert_eq!(t.net, Money::from_euros(200));
        assert_eq!(t.vat, Money::ZERO);
        assert_eq!(t.gross, Money::from_euros(200));
    }

    #[test]
    fn reference_row_at_nineteen_percent_net_200_vat_38_gross_238() {
        let inv = invoice(
            vec![row(Quantity::whole(1), 20_000)],
            VatRate::from_percent(19),
        );
        let t = inv.totals();
        assert_eq!(t.net, Money::from_euros(200));
        assert_eq!(t.vat, Money::from_euros(38));
        assert_eq!(t.gross, Money::from_euros(238));
    }

    // --- Multi-row, 0% ---

    #[test]
    fn multi_row_at_zero_percent_net_is_sum_of_row_totals() {
        // 2 x 50.00 = 100.00; 1.5 x 100.00 = 150.00; 3 x 0.10 = 0.30
        let inv = invoice(
            vec![
                row(Quantity::whole(2), 5_000),
                row(Quantity::from_thousandths(1_500), 10_000),
                row(Quantity::whole(3), 10),
            ],
            VatRate::ZERO,
        );
        let t = inv.totals();
        assert_eq!(t.net, Money::from_cents(25_030));
        assert_eq!(t.vat, Money::ZERO);
        assert_eq!(t.gross, Money::from_cents(25_030));
    }

    // --- Multi-row, non-zero rate ---

    #[test]
    fn multi_row_at_nineteen_percent_recomputes_vat_and_gross() {
        // 2.5 x 10.00 = 25.00; 1 x 75.37 = 75.37; Net = 100.37
        // VAT = 100.37 x 19% = 19.0703 -> 19.07; Gross = 119.44
        let inv = invoice(
            vec![
                row(Quantity::from_thousandths(2_500), 1_000),
                row(Quantity::whole(1), 7_537),
            ],
            VatRate::from_percent(19),
        );
        let t = inv.totals();
        assert_eq!(t.net, Money::from_cents(10_037));
        assert_eq!(t.vat, Money::from_cents(1_907));
        assert_eq!(t.gross, Money::from_cents(11_944));
        assert_eq!(t.gross, t.net + t.vat);
    }

    #[test]
    fn totals_contract_vat_equals_net_times_rate_for_exact_case() {
        // Net 500.00 at 7.5% (750 bps) is exactly 37.50.
        let inv = invoice(
            vec![row(Quantity::whole(5), 10_000)],
            VatRate::from_basis_points(750),
        );
        let t = inv.totals();
        assert_eq!(t.net, Money::from_euros(500));
        assert_eq!(t.vat, Money::from_cents(3_750));
        assert_eq!(t.gross, Money::from_cents(53_750));
    }

    // --- Rounding rule ---

    #[test]
    fn line_total_rounds_half_away_from_zero() {
        // 0.005 x 1.00 = 0.005 EUR = 0.5 cent -> rounds up to 1 cent.
        let item = row(Quantity::from_thousandths(5), 100);
        assert_eq!(item.total(), Money::from_cents(1));
    }

    #[test]
    fn line_total_below_half_cent_rounds_down() {
        // 0.001 x 0.50 = 0.0005 EUR = 0.05 cent -> 0 cents.
        let item = row(Quantity::from_thousandths(1), 50);
        assert_eq!(item.total(), Money::ZERO);
    }

    #[test]
    fn vat_rounds_half_away_from_zero() {
        // Net 0.05 EUR at 10% = 0.005 EUR = 0.5 cent -> 1 cent.
        let inv = invoice(vec![row(Quantity::whole(1), 5)], VatRate::from_percent(10));
        assert_eq!(inv.vat(), Money::from_cents(1));
        assert_eq!(inv.gross(), Money::from_cents(6));
    }

    #[test]
    fn empty_invoice_has_zero_totals() {
        let t = Invoice::default().totals();
        assert_eq!(t, Totals::default());
    }

    #[test]
    fn removing_a_row_recomputes_totals() {
        let mut inv = invoice(
            vec![
                row(Quantity::whole(1), 20_000),
                row(Quantity::whole(1), 5_000),
            ],
            VatRate::from_percent(19),
        );
        assert_eq!(inv.net(), Money::from_euros(250));
        inv.line_items.remove(1);
        let t = inv.totals();
        assert_eq!(t.net, Money::from_euros(200));
        assert_eq!(t.vat, Money::from_euros(38));
        assert_eq!(t.gross, Money::from_euros(238));
    }

    // --- §19 UStG standing note (task 2.2) ---

    #[test]
    fn section_19_note_text_matches_source_invoice() {
        assert_eq!(SECTION_19_NOTE, "According to § 19 UStG no VAT is charged");
    }

    #[test]
    fn section_19_note_is_present_at_zero_percent() {
        assert_eq!(
            section_19_note(VatRate::ZERO),
            Some("According to § 19 UStG no VAT is charged")
        );
    }

    #[test]
    fn section_19_note_is_absent_at_nineteen_percent() {
        assert_eq!(section_19_note(VatRate::from_percent(19)), None);
    }

    #[test]
    fn section_19_note_boundary_one_basis_point_is_absent() {
        // The smallest non-zero rate (0.01%) must already suppress the note.
        assert_eq!(section_19_note(VatRate::from_basis_points(1)), None);
    }

    #[test]
    fn section_19_note_is_absent_at_fractional_rate() {
        assert_eq!(section_19_note(VatRate::from_basis_points(750)), None);
    }

    #[test]
    fn invoice_accessor_follows_its_vat_rate() {
        let zero = Invoice::default();
        assert_eq!(zero.section_19_note(), Some(SECTION_19_NOTE));

        let nineteen = Invoice {
            vat_rate: VatRate::from_percent(19),
            ..Invoice::default()
        };
        assert_eq!(nineteen.section_19_note(), None);
    }

    #[test]
    fn section_19_note_does_not_depend_on_custom_notes() {
        // Custom notes are free text and do not change the standing-note rule.
        let inv = Invoice {
            notes: "Payment within 14 days".to_string(),
            ..Invoice::default()
        };
        assert_eq!(inv.section_19_note(), Some(SECTION_19_NOTE));
        assert_eq!(inv.notes, "Payment within 14 days");
    }

    // --- Shared EUR formatter (Display for Money) ---

    #[test]
    fn money_display_formats_whole_euros_with_two_decimals_and_euro_sign() {
        assert_eq!(Money::from_cents(20_000).to_string(), "200.00 €");
    }

    #[test]
    fn money_display_formats_zero() {
        assert_eq!(Money::ZERO.to_string(), "0.00 €");
    }

    #[test]
    fn money_display_pads_single_digit_cents() {
        assert_eq!(Money::from_cents(5).to_string(), "0.05 €");
        assert_eq!(Money::from_cents(10_907).to_string(), "109.07 €");
    }

    #[test]
    fn money_display_formats_negative_amounts_with_leading_sign() {
        assert_eq!(Money::from_cents(-150).to_string(), "-1.50 €");
    }
}

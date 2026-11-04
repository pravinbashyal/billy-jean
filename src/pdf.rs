//! PDF projection and in-process rendering (task 4.1, design D2).
//!
//! Two layers, kept separate so the field content is testable without parsing
//! a PDF:
//! 1. [`project`] turns an [`Invoice`] into a [`Projection`]: every field group
//!    already formatted as strings. Pure; no PDF types involved.
//! 2. [`render_projection`] lays a [`Projection`] out with `genpdf` and returns
//!    the PDF bytes. [`render_invoice`] and [`render_to_path`] compose both.
//!
//! Only in-process code is used. There is no external process, binary, or
//! network access. The font is embedded at compile time, so export works from
//! any working directory.

use std::error::Error as StdError;
use std::fmt;
use std::path::Path;

use genpdf::elements::{CellDecorator, LinearLayout, Paragraph, TableLayout};
use genpdf::error::Error as GenpdfError;
use genpdf::fonts::{FontData, FontFamily};
use genpdf::render::Area;
use genpdf::style::{Color, Style, StyledString};
use genpdf::{
    Alignment, Context, Document, Element, Margins, Mm, Position, RenderResult, Size,
    SimplePageDecorator,
};

use crate::fonts;
use crate::iban;
use crate::model::{Invoice, LineItem, Quantity, Sender, SenderLayout, VatRate};

/// Style tokens for the invoice page (design D4). Font sizes are whole points,
/// because genpdf sizes are `u8`. Margins are millimetres.
mod style {
    use genpdf::style::Color;

    /// Near-black ink for values and body text (`#111827`).
    pub const INK: Color = Color::Rgb(17, 24, 39);
    /// Muted grey for labels, sender detail, the return line, and the bank footer (`#6B7280`).
    pub const MUTED: Color = Color::Rgb(107, 114, 128);
    /// Light grey for hairline rules (`#E5E7EB`).
    pub const RULE: Color = Color::Rgb(229, 231, 235);
    /// Slightly darker grey under the last line-items row (`#D1D5DB`).
    pub const CLOSING_RULE: Color = Color::Rgb(209, 213, 219);
    /// The single accent colour, cyan (`#00B8D9`). Used only by the rule under the table header.
    pub const ACCENT: Color = Color::Rgb(0, 184, 217);

    pub const RETURN_LINE_PT: u8 = 8;
    pub const LABEL_PT: u8 = 8;
    pub const BANK_FOOTER_PT: u8 = 8;
    pub const SENDER_DETAIL_PT: u8 = 9;
    pub const BODY_PT: u8 = 10;
    pub const GROSS_PT: u8 = 11;
    pub const SENDER_NAME_PT: u8 = 13;
    pub const CLIENT_NAME_PT: u8 = 14;
    pub const HEADER_NAME_PT: u8 = 16;
    pub const FOOTER_NAME_PT: u8 = 8;

    pub const MARGIN_TOP_MM: f32 = 18.0;
    pub const MARGIN_SIDE_MM: f32 = 20.0;
    pub const MARGIN_BOTTOM_MM: f32 = 18.0;

    /// Line-items cell padding, top and bottom, in millimetres.
    pub const TABLE_PAD_Y_MM: f32 = 2.5;
    /// Line-items cell padding between columns, in millimetres. The first and
    /// last columns drop the outer side so they sit flush with the margins.
    pub const TABLE_PAD_X_MM: f32 = 2.0;
    /// Line spacing for line-items body cells (design D4).
    pub const TABLE_BODY_LINE_SPACING: f64 = 1.3;
    /// Line-items column weights: Description, Quantity, Rate, Total.
    pub const TABLE_WEIGHTS: [usize; 4] = [10, 2, 3, 3];
}

/// Vertical padding above and below a [`Hairline`], in millimetres.
const HAIRLINE_PAD_MM: f32 = 1.0;

// ---------------------------------------------------------------------------
// Projection (pure)
// ---------------------------------------------------------------------------

/// One rendered line-item row. Every cell is already formatted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineItemRow {
    pub description: String,
    pub quantity: String,
    pub rate: String,
    pub total: String,
}

/// One labelled amount in the totals block, e.g. `Net` / `200.00 €`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotalRow {
    pub label: String,
    pub amount: String,
}

/// The sender as displayed: name, address, tax and contact lines, and the
/// one-line return line (design D6). Blank optional values produce no line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SenderBlock {
    /// The sender name, trimmed.
    pub name: String,
    /// The street, trimmed. The Bold header layout prints it on its own line.
    pub street: String,
    /// Street, postal code/city, and country, each non-empty value on its own line.
    pub address_lines: Vec<String>,
    /// `Postal code/City, Country` joined with `, `, empty segments skipped. The
    /// Bold header layout prints it on one line.
    pub city_country: String,
    /// `(label, value)` pairs: `Tax number` always, then `VAT ID` only when filled.
    pub tax_lines: Vec<(&'static str, String)>,
    /// The contact email, `None` when blank.
    pub email: Option<String>,
    /// `Name · Street · Postal City` (see [`return_line`]).
    pub return_line: String,
}

/// Every invoice field group, formatted for display. This is the single
/// content contract shared by the PDF renderer and (task 8.1) the live preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    /// The sender block, shared by every sender layout.
    pub sender: SenderBlock,
    /// The Sender layout chosen in the Sender panel (design D5). It picks the
    /// sender treatment, the return line, and where the bank details sit.
    pub sender_layout: SenderLayout,
    /// Billed-to lines in order: name, street, postal code/city, country.
    /// Empty fields are omitted.
    pub billed_to: Vec<String>,
    /// Invoice metadata as `(label, value)` pairs, in display order.
    pub meta: Vec<(&'static str, String)>,
    pub line_items: Vec<LineItemRow>,
    /// Net, VAT (labelled with its rate), and Gross, in that order.
    pub totals: Vec<TotalRow>,
    /// The standing §19 UStG note. Present only at 0% VAT, rendered as its own
    /// element and never merged into [`Projection::custom_notes`].
    pub standing_note: Option<&'static str>,
    /// The free-text Notes, verbatim. `None` when empty or whitespace-only.
    pub custom_notes: Option<String>,
    /// Bank block as `(label, value)` pairs: Bank, Beneficiary, IBAN in 4-char blocks.
    pub bank: Vec<(&'static str, String)>,
}

/// Projects an [`Invoice`] into formatted field groups. Pure and deterministic.
pub fn project(invoice: &Invoice) -> Projection {
    let totals = invoice.totals();
    Projection {
        sender: sender_block(&invoice.sender),
        sender_layout: invoice.sender.layout,
        billed_to: billed_to_lines(invoice),
        meta: vec![
            ("Invoice number", invoice.meta.number.trim().to_string()),
            ("Invoice date", invoice.meta.invoice_date.trim().to_string()),
            ("Service date", invoice.meta.service_date.trim().to_string()),
            ("Due date", invoice.meta.due_date.trim().to_string()),
        ],
        line_items: invoice.line_items.iter().map(line_item_row).collect(),
        totals: vec![
            TotalRow {
                label: "Total amount (Net)".to_string(),
                amount: totals.net.to_string(),
            },
            TotalRow {
                label: format!("VAT ({})", format_vat_rate(invoice.vat_rate)),
                amount: totals.vat.to_string(),
            },
            TotalRow {
                label: "Total amount (Gross)".to_string(),
                amount: totals.gross.to_string(),
            },
        ],
        standing_note: invoice.section_19_note(),
        custom_notes: if invoice.notes.trim().is_empty() {
            None
        } else {
            Some(invoice.notes.clone())
        },
        bank: vec![
            ("Bank", invoice.bank.bank_name.trim().to_string()),
            ("Beneficiary", effective_beneficiary(invoice)),
            ("IBAN", iban::group_in_blocks(&invoice.bank.iban)),
        ],
    }
}

/// The beneficiary shown on the invoice: the Beneficiary field, or the sender
/// Name when Beneficiary is blank (design D6). The stored field is not changed,
/// so later edits to the Name still flow through.
fn effective_beneficiary(invoice: &Invoice) -> String {
    let beneficiary = invoice.bank.beneficiary.trim();
    if beneficiary.is_empty() {
        invoice.sender.name.trim().to_string()
    } else {
        beneficiary.to_string()
    }
}

/// The return line `Name · Street · Postal City` (design D6). Country is left
/// out, and empty segments are skipped so no stray separator is left behind.
pub fn return_line(sender: &Sender) -> String {
    [
        sender.name.trim(),
        sender.street.trim(),
        sender.postal_code_city.trim(),
    ]
    .into_iter()
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join(" · ")
}

fn sender_block(sender: &Sender) -> SenderBlock {
    let mut tax_lines = vec![("Tax number", sender.tax_number.trim().to_string())];
    if !sender.vat_id.trim().is_empty() {
        tax_lines.push(("VAT ID", sender.vat_id.trim().to_string()));
    }
    let email = sender.email.trim();
    SenderBlock {
        name: sender.name.trim().to_string(),
        street: sender.street.trim().to_string(),
        address_lines: [
            sender.street.trim(),
            sender.postal_code_city.trim(),
            sender.country.trim(),
        ]
        .into_iter()
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect(),
        city_country: [sender.postal_code_city.trim(), sender.country.trim()]
            .into_iter()
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        tax_lines,
        email: (!email.is_empty()).then(|| email.to_string()),
        return_line: return_line(sender),
    }
}

fn billed_to_lines(invoice: &Invoice) -> Vec<String> {
    let client = &invoice.client;
    [
        client.name.trim(),
        client.street.trim(),
        client.postal_code_city.trim(),
        client.country.trim(),
    ]
    .into_iter()
    .filter(|line| !line.is_empty())
    .map(str::to_string)
    .collect()
}

fn line_item_row(item: &LineItem) -> LineItemRow {
    LineItemRow {
        description: item.description.trim().to_string(),
        quantity: format_quantity(item.quantity),
        rate: item.rate.to_string(),
        total: item.total().to_string(),
    }
}

/// Formats a quantity without trailing zeros: `1`, `2.5`, `0.005`.
fn format_quantity(quantity: Quantity) -> String {
    let thousandths = quantity.thousandths();
    let sign = if thousandths < 0 { "-" } else { "" };
    let abs = thousandths.unsigned_abs();
    let whole = abs / 1000;
    let frac = abs % 1000;
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        let frac_digits = format!("{frac:03}");
        format!("{sign}{whole}.{}", frac_digits.trim_end_matches('0'))
    }
}

/// Formats a VAT rate as a percentage without trailing zeros: `0%`, `19%`, `7.5%`.
fn format_vat_rate(rate: VatRate) -> String {
    let bps = rate.basis_points();
    let whole = bps / 100;
    let frac = bps % 100;
    if frac == 0 {
        format!("{whole}%")
    } else {
        let frac_digits = format!("{frac:02}");
        format!("{whole}.{}%", frac_digits.trim_end_matches('0'))
    }
}

// ---------------------------------------------------------------------------
// Rendering (genpdf)
// ---------------------------------------------------------------------------

/// Errors from PDF generation or writing the output file.
#[derive(Debug)]
pub enum PdfError {
    /// The bundled font failed to load, or genpdf failed while laying out or
    /// serialising the document.
    Genpdf(genpdf::error::Error),
    /// Writing the PDF to disk failed.
    Io(std::io::Error),
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PdfError::Genpdf(err) => write!(f, "PDF generation failed: {err}"),
            PdfError::Io(err) => write!(f, "could not write PDF: {err}"),
        }
    }
}

impl StdError for PdfError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            PdfError::Genpdf(err) => Some(err),
            PdfError::Io(err) => Some(err),
        }
    }
}

impl From<genpdf::error::Error> for PdfError {
    fn from(err: genpdf::error::Error) -> Self {
        PdfError::Genpdf(err)
    }
}

impl From<std::io::Error> for PdfError {
    fn from(err: std::io::Error) -> Self {
        PdfError::Io(err)
    }
}

/// Builds the document font family from the embedded faces. Bold and bold italic
/// use the bundled Bold face. There is no italic face, so italic falls back to
/// Regular (design D3). SemiBold is UI-only, because genpdf has a single bold flag.
fn font_family() -> Result<FontFamily<FontData>, PdfError> {
    Ok(FontFamily {
        regular: FontData::new(fonts::REGULAR.to_vec(), None)?,
        bold: FontData::new(fonts::BOLD.to_vec(), None)?,
        italic: FontData::new(fonts::REGULAR.to_vec(), None)?,
        bold_italic: FontData::new(fonts::BOLD.to_vec(), None)?,
    })
}

/// A paragraph at `size` pt in `color`, bold when `bold` is set.
fn styled(text: impl Into<String>, size: u8, color: Color, bold: bool) -> Paragraph {
    let mut style = Style::new().with_font_size(size).with_color(color);
    if bold {
        style.set_bold();
    }
    Paragraph::new(StyledString::new(text, style))
}

/// Right-aligns a paragraph within its area.
fn right(mut paragraph: Paragraph) -> Paragraph {
    paragraph.set_alignment(Alignment::Right);
    paragraph
}

/// Vertical space of a fixed height in millimetres. Draws nothing. Spacing is
/// done with these rather than with line breaks (design D4).
struct Gap {
    height_mm: f32,
}

fn gap(height_mm: f32) -> Gap {
    Gap { height_mm }
}

impl Element for Gap {
    fn render(
        &mut self,
        _context: &Context,
        area: Area<'_>,
        _style: Style,
    ) -> Result<RenderResult, GenpdfError> {
        let size = area.size();
        let height = Mm::from(self.height_mm);
        if size.height < height {
            // Does not fit: nothing is used here, and the space moves to the next page.
            return Ok(RenderResult {
                size: Size::new(0, 0),
                has_more: true,
            });
        }
        Ok(RenderResult {
            size: Size::new(size.width, height),
            has_more: false,
        })
    }
}

/// A full-width hairline in the rule colour, padded above and below (design D4).
/// genpdf cannot set line thickness, so the rule is told apart by colour.
struct Hairline;

impl Element for Hairline {
    fn render(
        &mut self,
        _context: &Context,
        area: Area<'_>,
        _style: Style,
    ) -> Result<RenderResult, GenpdfError> {
        let size = area.size();
        let block = Mm::from(2.0 * HAIRLINE_PAD_MM);
        if size.height < block {
            return Ok(RenderResult {
                size: Size::new(0, 0),
                has_more: true,
            });
        }
        let y = Mm::from(HAIRLINE_PAD_MM);
        area.draw_line(
            vec![Position::new(0, y), Position::new(size.width, y)],
            Style::new().with_color(style::RULE),
        );
        Ok(RenderResult {
            size: Size::new(size.width, block),
            has_more: false,
        })
    }
}

/// A text style at `size` pt in `color`, for paragraphs built from several runs.
fn text_style(size: u8, color: Color) -> Style {
    Style::new().with_font_size(size).with_color(color)
}

/// `label value` in one paragraph: the label muted, the value in ink (design D4).
fn labelled_value(label: &str, value: &str, size: u8) -> Paragraph {
    let mut paragraph = Paragraph::new(StyledString::new(
        format!("{label} "),
        text_style(size, style::MUTED),
    ));
    paragraph.push(StyledString::new(value, text_style(size, style::INK)));
    paragraph
}

/// The Bold header's one-line summary `Tax number <value> · VAT ID <value> · <email>`.
/// Labels, separators, and the email are muted; the tax values are in ink. Blank
/// VAT ID and email are already absent from the projection.
fn detail_line(sender: &SenderBlock, size: u8) -> Paragraph {
    let mut segments: Vec<Vec<StyledString>> = sender
        .tax_lines
        .iter()
        .map(|(label, value)| {
            vec![
                StyledString::new(format!("{label} "), text_style(size, style::MUTED)),
                StyledString::new(value.as_str(), text_style(size, style::INK)),
            ]
        })
        .collect();
    if let Some(email) = &sender.email {
        segments.push(vec![StyledString::new(
            email.as_str(),
            text_style(size, style::MUTED),
        )]);
    }

    let mut pieces: Vec<StyledString> = Vec::new();
    for (index, segment) in segments.into_iter().enumerate() {
        if index > 0 {
            pieces.push(StyledString::new(" · ", text_style(size, style::MUTED)));
        }
        pieces.extend(segment);
    }
    let mut pieces = pieces.into_iter();
    let first = pieces
        .next()
        .unwrap_or_else(|| StyledString::new("", text_style(size, style::MUTED)));
    let mut paragraph = Paragraph::new(first);
    for piece in pieces {
        paragraph.push(piece);
    }
    paragraph
}

/// The German letter sender block (design D6): a right-aligned block at the top
/// right. The name is bold, then the street, postal code/city, and country, then
/// the tax lines, the VAT ID when filled, and the email when filled.
fn letter_sender(sender: &SenderBlock) -> LinearLayout {
    let mut block = LinearLayout::vertical();
    block.push(right(styled(
        &sender.name,
        style::SENDER_NAME_PT,
        style::INK,
        true,
    )));
    for line in &sender.address_lines {
        block.push(right(styled(
            line,
            style::SENDER_DETAIL_PT,
            style::MUTED,
            false,
        )));
    }
    block.push(gap(2.0));
    for (label, value) in &sender.tax_lines {
        block.push(right(labelled_value(label, value, style::SENDER_DETAIL_PT)));
    }
    if let Some(email) = &sender.email {
        block.push(right(styled(
            email,
            style::SENDER_DETAIL_PT,
            style::MUTED,
            false,
        )));
    }
    block
}

/// The Bold header sender block (design D6): the name in large bold type at the
/// top left, the street, `Postal City, Country` on one line, the one-line tax and
/// contact summary, then a full-width hairline. No return line is printed.
fn header_sender(sender: &SenderBlock) -> LinearLayout {
    let mut block = LinearLayout::vertical();
    block.push(styled(
        &sender.name,
        style::HEADER_NAME_PT,
        style::INK,
        true,
    ));
    if !sender.street.is_empty() {
        block.push(styled(
            &sender.street,
            style::SENDER_DETAIL_PT,
            style::MUTED,
            false,
        ));
    }
    if !sender.city_country.is_empty() {
        block.push(styled(
            &sender.city_country,
            style::SENDER_DETAIL_PT,
            style::MUTED,
            false,
        ));
    }
    block.push(gap(2.0));
    block.push(detail_line(sender, style::SENDER_DETAIL_PT));
    block.push(Hairline);
    block
}

/// The Footer columns closing block (design D6). After the content, a hairline
/// and three equal muted columns: the sender (bold name, then the address), tax
/// and contact, and bank. It replaces the standalone bank footer.
fn footer_columns(projection: &Projection) -> Result<LinearLayout, PdfError> {
    let sender = &projection.sender;
    let size = style::BANK_FOOTER_PT;

    let mut address = LinearLayout::vertical();
    address.push(styled(
        &sender.name,
        style::FOOTER_NAME_PT,
        style::INK,
        true,
    ));
    for line in &sender.address_lines {
        address.push(styled(line, size, style::MUTED, false));
    }

    let mut contact = LinearLayout::vertical();
    for (label, value) in &sender.tax_lines {
        contact.push(labelled_value(label, value, size));
    }
    if let Some(email) = &sender.email {
        contact.push(styled(email, size, style::MUTED, false));
    }

    let mut bank = LinearLayout::vertical();
    for (index, (label, value)) in projection.bank.iter().enumerate() {
        if index > 0 {
            bank.push(gap(1.0));
        }
        bank.push(bank_pair(label, value));
    }

    let mut columns = TableLayout::new(vec![1, 1, 1]);
    columns.push_row(vec![
        Box::new(address),
        Box::new(contact),
        Box::new(bank),
    ])?;

    let mut footer = LinearLayout::vertical();
    footer.push(Hairline);
    footer.push(gap(3.0));
    footer.push(columns);
    Ok(footer)
}

/// The "Billed to" block on the left and the 2x2 invoice-meta grid on the right
/// (Invoice date | Service date / Invoice number | Due date). Labels are muted;
/// values are bold. In the German letter layout the left column opens with the
/// muted return line and a hairline above "Billed to".
fn billed_to_band(projection: &Projection) -> Result<TableLayout, PdfError> {
    let mut client = LinearLayout::vertical();
    if projection.sender_layout == SenderLayout::Letter {
        client.push(styled(
            &projection.sender.return_line,
            style::RETURN_LINE_PT,
            style::MUTED,
            false,
        ));
        client.push(Hairline);
        client.push(gap(3.0));
    }
    client.push(styled("Billed to", style::LABEL_PT, style::MUTED, false));
    client.push(gap(1.0));
    // `billed_to` lists the client name first, then the address lines.
    for (index, line) in projection.billed_to.iter().enumerate() {
        if index == 0 {
            client.push(styled(line, style::CLIENT_NAME_PT, style::INK, true));
        } else {
            client.push(styled(line, style::BODY_PT, style::INK, false));
        }
    }

    let mut grid = TableLayout::new(vec![1, 1]);
    grid.push_row(vec![
        meta_cell(projection, "Invoice date"),
        meta_cell(projection, "Service date"),
    ])?;
    grid.push_row(vec![
        meta_cell(projection, "Invoice number"),
        meta_cell(projection, "Due date"),
    ])?;

    let mut band = TableLayout::new(vec![1, 1]);
    band.push_row(vec![Box::new(client), Box::new(grid)])?;
    Ok(band)
}

/// One cell of the meta grid: a muted label above its bold value. The value is
/// looked up by label, so the grid order is independent of the projection order.
fn meta_cell(projection: &Projection, label: &str) -> Box<dyn Element> {
    let value = projection
        .meta
        .iter()
        .find(|(name, _)| *name == label)
        .map_or("", |(_, value)| value.as_str());
    let mut cell = LinearLayout::vertical();
    cell.push(styled(label, style::LABEL_PT, style::MUTED, false));
    cell.push(styled(value, style::BODY_PT, style::INK, true));
    Box::new(cell.padded(Margins::trbl(0, 4, 3, 0)))
}

/// Which horizontal rule closes a line-items row (design D4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowRule {
    /// Under the header row, in cyan.
    Accent,
    /// Between two body rows, in the light rule grey.
    Hairline,
    /// Under the last row, in the slightly darker grey.
    Closing,
}

impl RowRule {
    fn color(self) -> Color {
        match self {
            RowRule::Accent => style::ACCENT,
            RowRule::Hairline => style::RULE,
            RowRule::Closing => style::CLOSING_RULE,
        }
    }
}

/// The rule drawn under `row` of a table with `num_rows` rows, header included.
/// Row 0 is the header and gets the accent. The last row gets the closing rule,
/// and every row in between gets a hairline. For `N >= 2` rows that is one accent
/// rule, `N - 2` hairlines, and one closing rule. No vertical rule is ever drawn.
fn row_rule(row: usize, num_rows: usize) -> Option<RowRule> {
    if row >= num_rows {
        None
    } else if row == 0 {
        Some(RowRule::Accent)
    } else if row + 1 == num_rows {
        Some(RowRule::Closing)
    } else {
        Some(RowRule::Hairline)
    }
}

/// Draws horizontal rules only, under each row (design D4). It never draws
/// vertical lines or an outer frame. A row that continues onto the next page
/// gets its rule when it finishes.
#[derive(Debug, Default)]
struct InvoiceTableDecorator {
    num_rows: usize,
}

impl CellDecorator for InvoiceTableDecorator {
    fn set_table_size(&mut self, _num_columns: usize, num_rows: usize) {
        self.num_rows = num_rows;
    }

    fn decorate_cell(
        &mut self,
        _column: usize,
        row: usize,
        has_more: bool,
        area: Area<'_>,
        _style: Style,
    ) {
        if has_more {
            return;
        }
        let Some(rule) = row_rule(row, self.num_rows) else {
            return;
        };
        let size = area.size();
        area.draw_line(
            vec![
                Position::new(0, size.height),
                Position::new(size.width, size.height),
            ],
            Style::new().with_color(rule.color()),
        );
    }
}

/// Renders its inner element with a fixed line spacing. genpdf 0.2 reads line
/// spacing only from the outer style, so it can't be set on a single paragraph.
struct LineSpaced<E> {
    inner: E,
    spacing: f64,
}

impl<E: Element> Element for LineSpaced<E> {
    fn render(
        &mut self,
        context: &Context,
        area: Area<'_>,
        style: Style,
    ) -> Result<RenderResult, GenpdfError> {
        self.inner
            .render(context, area, style.with_line_spacing(self.spacing))
    }
}

/// Wraps a line-items cell in its padding. Every cell gets 2.5 mm above and
/// below and 2 mm on each side, except that the first column has no left padding
/// and the last column has no right padding (design D4).
fn table_cell(element: impl Element + 'static, column: usize, columns: usize) -> Box<dyn Element> {
    let left = if column == 0 {
        0.0
    } else {
        style::TABLE_PAD_X_MM
    };
    let right = if column + 1 == columns {
        0.0
    } else {
        style::TABLE_PAD_X_MM
    };
    Box::new(element.padded(Margins::trbl(
        style::TABLE_PAD_Y_MM,
        right,
        style::TABLE_PAD_Y_MM,
        left,
    )))
}

/// The line-items table (design D4). The header is 8 pt muted, with the numeric
/// headers right-aligned. Body cells are 10 pt ink at 1.3 line spacing. The
/// numeric cells are right-aligned, and a wrapped description stays in its row.
fn line_items_table(projection: &Projection) -> Result<TableLayout, PdfError> {
    const COLUMNS: usize = style::TABLE_WEIGHTS.len();
    let mut table = TableLayout::new(style::TABLE_WEIGHTS.to_vec());
    table.set_cell_decorator(InvoiceTableDecorator::default());

    let header = |text: &str, column: usize| {
        let paragraph = styled(text, style::LABEL_PT, style::MUTED, false);
        let paragraph = if column == 0 {
            paragraph
        } else {
            right(paragraph)
        };
        table_cell(paragraph, column, COLUMNS)
    };
    table.push_row(vec![
        header("Description", 0),
        header("Quantity", 1),
        header("Rate", 2),
        header("Total", 3),
    ])?;

    let body_cell = |text: &str, align_right: bool| {
        let paragraph = styled(text, style::BODY_PT, style::INK, false);
        LineSpaced {
            inner: if align_right {
                right(paragraph)
            } else {
                paragraph
            },
            spacing: style::TABLE_BODY_LINE_SPACING,
        }
    };
    for row in &projection.line_items {
        table.push_row(vec![
            table_cell(body_cell(&row.description, false), 0, COLUMNS),
            table_cell(body_cell(&row.quantity, true), 1, COLUMNS),
            table_cell(body_cell(&row.rate, true), 2, COLUMNS),
            table_cell(body_cell(&row.total, true), 3, COLUMNS),
        ])?;
    }
    Ok(table)
}

/// The totals block: right-aligned, about 45 % of the width (columns 55:25:20 of
/// spacer, label, amount). A hairline sits above the bold Gross row, drawn as two
/// adjacent hairline cells so it spans the label and amount columns only.
fn totals_block(totals: &[TotalRow]) -> Result<TableLayout, PdfError> {
    let mut table = TableLayout::new(vec![55, 25, 20]);
    if let Some((gross, others)) = totals.split_last() {
        for total in others {
            table.push_row(vec![
                Box::new(gap(0.0)),
                Box::new(styled(&total.label, style::BODY_PT, style::INK, false)),
                Box::new(right(styled(&total.amount, style::BODY_PT, style::INK, false))),
            ])?;
        }
        table.push_row(vec![Box::new(gap(0.0)), Box::new(Hairline), Box::new(Hairline)])?;
        table.push_row(vec![
            Box::new(gap(0.0)),
            Box::new(styled(&gross.label, style::GROSS_PT, style::INK, true)),
            Box::new(right(styled(&gross.amount, style::GROSS_PT, style::INK, true))),
        ])?;
    }
    Ok(table)
}

/// The Notes section: the standing §19 note and the free-text notes. Only called
/// when at least one of them is present.
fn notes_block(projection: &Projection) -> LinearLayout {
    let mut block = LinearLayout::vertical();
    block.push(styled("Notes", style::LABEL_PT, style::MUTED, false));
    block.push(gap(1.0));
    if let Some(note) = projection.standing_note {
        block.push(styled(note, style::BODY_PT, style::INK, false));
    }
    if let Some(notes) = &projection.custom_notes {
        block.push(styled(notes, style::BODY_PT, style::INK, false));
    }
    block
}

/// One bank detail: a muted label over its muted value, both at footer size.
fn bank_pair(label: &str, value: &str) -> LinearLayout {
    let mut cell = LinearLayout::vertical();
    cell.push(styled(label, style::BANK_FOOTER_PT, style::MUTED, false));
    cell.push(styled(value, style::BANK_FOOTER_PT, style::MUTED, false));
    cell
}

/// The bank footer used by the German letter and Bold header layouts: a
/// full-width hairline, then Bank, Beneficiary, and IBAN in three equal muted
/// columns, each a label over its value.
fn bank_footer(bank: &[(&'static str, String)]) -> Result<LinearLayout, PdfError> {
    let mut cells: Vec<Box<dyn Element>> = Vec::new();
    for (label, value) in bank {
        cells.push(Box::new(bank_pair(label, value)));
    }
    let mut columns = TableLayout::new(vec![1; cells.len()]);
    columns.push_row(cells)?;

    let mut footer = LinearLayout::vertical();
    footer.push(Hairline);
    footer.push(gap(3.0));
    footer.push(columns);
    Ok(footer)
}

/// Lays a [`Projection`] out as an A4 PDF and returns the bytes.
///
/// Order: the sender treatment of the chosen Sender layout (design D6), the
/// Billed to and meta band, the line-items table, the totals block, Notes (only
/// with content), then the bank details. The German letter and Bold header
/// layouts end with the bank footer. The Footer columns layout ends with its
/// three-column block instead. There is no document title, wave, or colour band.
pub fn render_projection(projection: &Projection) -> Result<Vec<u8>, PdfError> {
    let mut doc = Document::new(font_family()?);
    doc.set_title("Invoice");
    doc.set_font_size(style::BODY_PT);
    doc.set_line_spacing(1.25);

    let mut decorator = SimplePageDecorator::new();
    decorator.set_margins(Margins::trbl(
        style::MARGIN_TOP_MM,
        style::MARGIN_SIDE_MM,
        style::MARGIN_BOTTOM_MM,
        style::MARGIN_SIDE_MM,
    ));
    doc.set_page_decorator(decorator);

    match projection.sender_layout {
        SenderLayout::Letter => doc.push(letter_sender(&projection.sender)),
        SenderLayout::Header => doc.push(header_sender(&projection.sender)),
        SenderLayout::Footer => doc.push(styled(
            &projection.sender.return_line,
            style::RETURN_LINE_PT,
            style::MUTED,
            false,
        )),
    }
    doc.push(gap(6.0));
    doc.push(billed_to_band(projection)?);
    doc.push(gap(6.0));
    doc.push(line_items_table(projection)?);
    doc.push(gap(6.0));
    doc.push(totals_block(&projection.totals)?);

    if projection.standing_note.is_some() || projection.custom_notes.is_some() {
        doc.push(gap(6.0));
        doc.push(notes_block(projection));
    }

    doc.push(gap(8.0));
    if projection.sender_layout == SenderLayout::Footer {
        doc.push(footer_columns(projection)?);
    } else {
        doc.push(bank_footer(&projection.bank)?);
    }

    let mut buffer = Vec::new();
    doc.render(&mut buffer)?;
    Ok(buffer)
}

/// Projects and renders an invoice to PDF bytes, entirely in-process.
pub fn render_invoice(invoice: &Invoice) -> Result<Vec<u8>, PdfError> {
    render_projection(&project(invoice))
}

/// Projects, renders, and writes an invoice PDF to `path`, overwriting any
/// existing file.
pub fn render_to_path(invoice: &Invoice, path: &Path) -> Result<(), PdfError> {
    let bytes = render_invoice(invoice)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Bank, Client, InvoiceMeta, Money, SECTION_19_NOTE};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// The reference invoice from the source PDF (invoice number 4).
    fn reference_invoice() -> Invoice {
        Invoice {
            sender: Sender {
                name: "Alexandra Morgan".to_string(),
                street: "Whitfield. 42".to_string(),
                postal_code_city: "10115 Musterstadt".to_string(),
                country: "Germany".to_string(),
                ..Sender::default()
            },
            client: Client {
                name: "Ostrander Quennell".to_string(),
                street: "Rayburn Str. 42".to_string(),
                postal_code_city: "80331 Musterstadt".to_string(),
                country: "Germany".to_string(),
            },
            meta: InvoiceMeta {
                number: "4".to_string(),
                invoice_date: "03/10/2026".to_string(),
                service_date: "11/09/2026".to_string(),
                due_date: "15/10/2026".to_string(),
            },
            line_items: vec![LineItem {
                description: "Live musician performance - Tallis Vestry, Gorham Bank"
                    .to_string(),
                quantity: Quantity::whole(1),
                rate: Money::from_euros(200),
            }],
            vat_rate: VatRate::ZERO,
            notes: String::new(),
            bank: Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: "DE89370400440532013000".to_string(),
            },
        }
    }

    /// A second invoice that exercises every optional path: non-zero VAT,
    /// multiple rows, fractional quantity, and custom notes.
    fn busy_invoice() -> Invoice {
        let mut invoice = reference_invoice();
        invoice.line_items.push(LineItem {
            description: "Consulting".to_string(),
            quantity: Quantity::from_thousandths(2_500),
            rate: Money::from_cents(1_000),
        });
        invoice.vat_rate = VatRate::from_percent(19);
        invoice.notes = "Payment within 14 days".to_string();
        invoice
    }

    /// A uniquely named scratch directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> TempDir {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock before epoch")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "billing-pdf-{name}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            TempDir(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // --- Projection: reference invoice field contract (task 4.1 verify) ---

    #[test]
    fn projection_sender_block_is_name_address_tax_and_return_line() {
        let p = project(&reference_invoice());
        assert_eq!(p.sender.name, "Alexandra Morgan");
        assert_eq!(
            p.sender.address_lines,
            vec!["Whitfield. 42", "10115 Musterstadt", "Germany"]
        );
        assert_eq!(p.sender.tax_lines, vec![("Tax number", String::new())]);
        assert_eq!(p.sender.email, None);
        assert_eq!(
            p.sender.return_line,
            "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt"
        );
        assert_eq!(p.sender_layout, SenderLayout::Letter);
    }

    #[test]
    fn projection_carries_the_chosen_sender_layout() {
        let mut invoice = reference_invoice();
        invoice.sender.layout = SenderLayout::Footer;
        assert_eq!(project(&invoice).sender_layout, SenderLayout::Footer);
    }

    #[test]
    fn projection_shows_vat_id_and_email_only_when_filled() {
        let blank = project(&reference_invoice());
        assert_eq!(blank.sender.tax_lines.len(), 1, "no VAT ID line when blank");
        assert_eq!(blank.sender.email, None, "no email when blank");

        let mut invoice = reference_invoice();
        invoice.sender.tax_number = "21/815/08150".to_string();
        invoice.sender.vat_id = "  DE123456789 ".to_string();
        invoice.sender.email = " hello@example.de ".to_string();
        let filled = project(&invoice);
        assert_eq!(
            filled.sender.tax_lines,
            vec![
                ("Tax number", "21/815/08150".to_string()),
                ("VAT ID", "DE123456789".to_string()),
            ]
        );
        assert_eq!(filled.sender.email.as_deref(), Some("hello@example.de"));
    }

    #[test]
    fn beneficiary_falls_back_to_sender_name_only_when_blank() {
        let mut invoice = reference_invoice();

        invoice.bank.beneficiary = String::new();
        assert!(project(&invoice)
            .bank
            .contains(&("Beneficiary", "Alexandra Morgan".to_string())));

        invoice.bank.beneficiary = "   ".to_string();
        assert!(project(&invoice)
            .bank
            .contains(&("Beneficiary", "Alexandra Morgan".to_string())));

        invoice.bank.beneficiary = "Penhallow Morgan".to_string();
        assert!(project(&invoice)
            .bank
            .contains(&("Beneficiary", "Penhallow Morgan".to_string())));

        // The fallback is a projection only: the stored Beneficiary stays blank.
        invoice.bank.beneficiary = String::new();
        invoice.sender.name = "Someone Else".to_string();
        assert_eq!(invoice.bank.beneficiary, "");
        assert!(project(&invoice)
            .bank
            .contains(&("Beneficiary", "Someone Else".to_string())));
    }

    #[test]
    fn beneficiary_fallback_is_blank_when_both_names_are_blank() {
        let mut invoice = reference_invoice();
        invoice.sender.name = String::new();
        invoice.bank.beneficiary = String::new();
        assert!(project(&invoice)
            .bank
            .contains(&("Beneficiary", String::new())));
    }

    #[test]
    fn projection_billed_to_contains_client_name_and_address() {
        let p = project(&reference_invoice());
        assert_eq!(
            p.billed_to,
            vec![
                "Ostrander Quennell",
                "Rayburn Str. 42",
                "80331 Musterstadt",
                "Germany",
            ]
        );
    }

    #[test]
    fn projection_totals_are_euro_formatted_at_zero_percent() {
        let p = project(&reference_invoice());
        assert_eq!(
            p.totals,
            vec![
                TotalRow {
                    label: "Total amount (Net)".to_string(),
                    amount: "200.00 €".to_string(),
                },
                TotalRow {
                    label: "VAT (0%)".to_string(),
                    amount: "0.00 €".to_string(),
                },
                TotalRow {
                    label: "Total amount (Gross)".to_string(),
                    amount: "200.00 €".to_string(),
                },
            ]
        );
    }

    #[test]
    fn projection_bank_block_shows_iban_in_four_char_blocks() {
        let p = project(&reference_invoice());
        assert!(p.bank.contains(&("Bank", "Ellery".to_string())));
        assert!(p
            .bank
            .contains(&("Beneficiary", "Penhallow Morgan".to_string())));
        assert!(p
            .bank
            .contains(&("IBAN", "DE89 3704 0044 0532 0130 00".to_string())));
    }

    #[test]
    fn projection_shows_standing_section_19_note_at_zero_percent() {
        let p = project(&reference_invoice());
        assert_eq!(p.standing_note, Some(SECTION_19_NOTE));
        assert_eq!(p.custom_notes, None);
    }

    #[test]
    fn projection_hides_standing_note_above_zero_percent_and_keeps_custom_notes() {
        let p = project(&busy_invoice());
        assert_eq!(p.standing_note, None);
        assert_eq!(p.custom_notes.as_deref(), Some("Payment within 14 days"));
    }

    #[test]
    fn projection_custom_notes_do_not_absorb_the_standing_note() {
        // The §19 text must come only from the standing-note path, never via notes.
        let mut invoice = reference_invoice();
        invoice.notes = "Payment within 14 days".to_string();
        let p = project(&invoice);
        assert_eq!(p.standing_note, Some(SECTION_19_NOTE));
        assert_eq!(p.custom_notes.as_deref(), Some("Payment within 14 days"));
    }

    #[test]
    fn projection_whitespace_only_notes_are_omitted() {
        let mut invoice = reference_invoice();
        invoice.notes = "   \n ".to_string();
        assert_eq!(project(&invoice).custom_notes, None);
    }

    #[test]
    fn projection_line_item_row_is_formatted() {
        let p = project(&reference_invoice());
        assert_eq!(
            p.line_items,
            vec![LineItemRow {
                description: "Live musician performance - Tallis Vestry, Gorham Bank"
                    .to_string(),
                quantity: "1".to_string(),
                rate: "200.00 €".to_string(),
                total: "200.00 €".to_string(),
            }]
        );
    }

    #[test]
    fn projection_meta_rows_carry_number_and_three_dates() {
        let p = project(&reference_invoice());
        assert_eq!(
            p.meta,
            vec![
                ("Invoice number", "4".to_string()),
                ("Invoice date", "03/10/2026".to_string()),
                ("Service date", "11/09/2026".to_string()),
                ("Due date", "15/10/2026".to_string()),
            ]
        );
    }

    #[test]
    fn projection_labels_vat_with_its_rate_above_zero() {
        let p = project(&busy_invoice());
        assert_eq!(p.totals[1].label, "VAT (19%)");
    }

    // --- Formatting helpers ---

    #[test]
    fn return_line_joins_name_street_and_city_without_country() {
        let sender = Sender {
            name: "Alexandra Sharma".to_string(),
            street: "Musterstraße 12".to_string(),
            postal_code_city: "10115 Musterstadt".to_string(),
            country: "Germany".to_string(),
            ..Sender::default()
        };
        assert_eq!(
            return_line(&sender),
            "Alexandra Sharma · Musterstraße 12 · 10115 Musterstadt"
        );
    }

    #[test]
    fn return_line_skips_empty_segments_without_stray_separators() {
        let name_and_city = Sender {
            name: "Alexandra Sharma".to_string(),
            street: "  ".to_string(),
            postal_code_city: "10115 Musterstadt".to_string(),
            country: "Germany".to_string(),
            ..Sender::default()
        };
        assert_eq!(
            return_line(&name_and_city),
            "Alexandra Sharma · 10115 Musterstadt"
        );

        let name_only = Sender {
            name: " Alexandra Sharma ".to_string(),
            country: "Germany".to_string(),
            ..Sender::default()
        };
        assert_eq!(return_line(&name_only), "Alexandra Sharma");

        let city_only = Sender {
            postal_code_city: "10115 Musterstadt".to_string(),
            ..Sender::default()
        };
        assert_eq!(return_line(&city_only), "10115 Musterstadt");
    }

    #[test]
    fn return_line_is_empty_for_empty_sender() {
        assert_eq!(return_line(&Sender::default()), "");
    }

    #[test]
    fn quantity_formats_without_trailing_zeros() {
        assert_eq!(format_quantity(Quantity::whole(1)), "1");
        assert_eq!(format_quantity(Quantity::from_thousandths(2_500)), "2.5");
        assert_eq!(format_quantity(Quantity::from_thousandths(5)), "0.005");
    }

    #[test]
    fn vat_rate_formats_as_percentage() {
        assert_eq!(format_vat_rate(VatRate::ZERO), "0%");
        assert_eq!(format_vat_rate(VatRate::from_percent(19)), "19%");
        assert_eq!(format_vat_rate(VatRate::from_basis_points(750)), "7.5%");
        assert_eq!(format_vat_rate(VatRate::from_basis_points(1)), "0.01%");
    }

    // --- In-process rendering ---

    #[test]
    fn render_invoice_returns_pdf_bytes() {
        let bytes = render_invoice(&reference_invoice()).expect("render");
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn render_to_temp_path_writes_a_pdf_file() {
        let dir = TempDir::new("reference");
        let path = dir.0.join("invoice.pdf");

        render_to_path(&reference_invoice(), &path).expect("render to path");

        assert!(path.is_file(), "PDF file should exist at {}", path.display());
        let bytes = std::fs::read(&path).expect("read back PDF");
        assert!(!bytes.is_empty(), "PDF should be non-empty");
        assert!(bytes.starts_with(b"%PDF"), "PDF should begin with %PDF magic");
    }

    #[test]
    fn render_exercises_all_optional_paths() {
        let dir = TempDir::new("busy");
        let path = dir.0.join("busy.pdf");

        render_to_path(&busy_invoice(), &path).expect("render busy invoice");

        let bytes = std::fs::read(&path).expect("read back PDF");
        assert!(bytes.starts_with(b"%PDF"));
    }

    /// Spike check for design D1 gate (b), bold half. Renders regular and bold
    /// lines with the real `font_family()`, writes the PDF and a 144 dpi PNG to
    /// `output-test/spike/`, and checks that the bold PNG has more ink than the
    /// regular one. Ignored by default; run it with `--ignored --nocapture`.
    #[test]
    #[ignore = "spike artefact writer; run with --ignored"]
    fn spike_bold_vs_regular_sample() {
        use crate::raster::rasterize;

        let sample = "Straße Größe Äpfel € § ä ß ö 0123456789";
        let line = |bold: bool| -> Vec<u8> {
            let mut doc = Document::new(font_family().expect("font family"));
            let style = if bold {
                Style::new().bold().with_font_size(14)
            } else {
                Style::new().with_font_size(14)
            };
            doc.push(Paragraph::new(StyledString::new(sample, style)));
            let mut bytes = Vec::new();
            doc.render(&mut bytes).expect("render bold-vs-regular sample");
            bytes
        };

        // Two single-line documents, so the whole-page ink count isolates one weight each.
        let regular_bytes = line(false);
        let bold_bytes = line(true);

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output-test/spike");
        std::fs::create_dir_all(&dir).expect("create output-test/spike");
        std::fs::write(dir.join("bold-vs-regular-regular.pdf"), &regular_bytes).expect("write");
        std::fs::write(dir.join("bold-vs-regular-bold.pdf"), &bold_bytes).expect("write");

        let ink = |bytes: &[u8], name: &str| -> u64 {
            let pages = rasterize(bytes, 2.0).expect("rasterize sample");
            let page = &pages[0];
            let file = std::fs::File::create(dir.join(name)).expect("create png");
            let mut encoder =
                png::Encoder::new(std::io::BufWriter::new(file), page.width, page.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .expect("png header")
                .write_image_data(&page.rgba)
                .expect("png data");
            page.rgba
                .chunks_exact(4)
                .filter(|px| (px[0] as u32 + px[1] as u32 + px[2] as u32) < 3 * 128)
                .count() as u64
        };
        let regular = ink(&regular_bytes, "hayro-bold-vs-regular-regular-144dpi.png");
        let bold = ink(&bold_bytes, "hayro-bold-vs-regular-bold-144dpi.png");
        println!("dark pixels at 144 dpi: regular={regular} bold={bold}");
        assert!(bold > regular, "bold line should carry more ink than regular");
    }

    #[test]
    fn bold_text_embeds_the_bold_face() {
        let dir = TempDir::new("bold-face");
        let path = dir.0.join("bold.pdf");

        let mut doc = Document::new(font_family().expect("font family"));
        doc.push(Paragraph::new(StyledString::new("Bold heading", Style::new().bold())));
        doc.push(Paragraph::new("Regular body"));
        let mut bytes = Vec::new();
        doc.render(&mut bytes).expect("render bold sample");
        std::fs::write(&path, &bytes).expect("write sample PDF");

        let output = std::process::Command::new("pdffonts")
            .arg(&path)
            .output()
            .expect("pdffonts must be installed to verify embedded fonts");
        assert!(
            output.status.success(),
            "pdffonts failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // pdffonts lists genpdf's anonymised names (F0..F3), so it cannot tell the
        // faces apart by name. Each embedded FontFile2 stream records its uncompressed
        // size in /Length1, which is the face's file size. Check both faces are there.
        let listing = String::from_utf8_lossy(&output.stdout);
        assert!(
            listing.contains("CID TrueType"),
            "pdffonts lists no embedded TrueType fonts:\n{listing}"
        );
        let pdf = String::from_utf8_lossy(&bytes);
        for (face, len) in [
            ("Regular", fonts::REGULAR.len()),
            ("Bold", fonts::BOLD.len()),
        ] {
            assert!(
                pdf.contains(&format!("/Length1 {len}")),
                "embedded {face} face ({len} bytes) missing from PDF:\n{listing}"
            );
        }
    }

    // --- Page skeleton (task 3.1) ---

    /// Text of a PDF's bytes via `pdftotext` (Poppler), one output line per line.
    fn pdftotext_of(name: &str, bytes: &[u8]) -> String {
        pdftotext_with(name, bytes, &[])
    }

    /// Like [`pdftotext_of`], with extra `pdftotext` flags such as `-layout`.
    fn pdftotext_with(name: &str, bytes: &[u8], flags: &[&str]) -> String {
        let dir = TempDir::new(name);
        let path = dir.0.join("invoice.pdf");
        std::fs::write(&path, bytes).expect("write PDF for pdftotext");
        let output = std::process::Command::new("pdftotext")
            .args(flags)
            .arg(&path)
            .arg("-")
            .output()
            .expect("pdftotext must be installed to verify rendered PDF text");
        assert!(
            output.status.success(),
            "pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[test]
    fn pdf_text_has_no_label_value_metadata_line() {
        let invoice = reference_invoice();
        let projection = project(&invoice);
        let bytes = render_invoice(&invoice).expect("render");
        let text = pdftotext_of("no-label-value", &bytes);

        for (label, value) in projection.meta.iter().chain(projection.bank.iter()) {
            assert!(
                !text.lines().any(|line| line.contains(&format!("{label}:"))),
                "{label} is printed as a Label: value line:\n{text}"
            );
            assert!(text.contains(label), "label {label:?} missing:\n{text}");
            assert!(
                text.contains(value.as_str()),
                "value {value:?} of {label:?} missing:\n{text}"
            );
        }
    }

    #[test]
    fn notes_label_is_absent_at_nonzero_vat_with_empty_notes() {
        let mut invoice = reference_invoice();
        invoice.vat_rate = VatRate::from_percent(19);
        invoice.notes = String::new();
        let text = pdftotext_of("notes-absent", &render_invoice(&invoice).expect("render"));
        assert!(!text.contains("Notes"), "Notes label printed:\n{text}");
    }

    #[test]
    fn notes_label_is_present_when_there_is_a_note() {
        let text = pdftotext_of("notes-present", &render_invoice(&busy_invoice()).expect("render"));
        assert!(text.contains("Notes"), "Notes label missing:\n{text}");
        assert!(text.contains("Payment within 14 days"), "notes missing:\n{text}");
    }

    // --- Padded line-items table (task 3.2) ---

    /// Three line items, the middle one with a description long enough to wrap.
    fn three_row_invoice() -> Invoice {
        let mut invoice = reference_invoice();
        invoice.line_items = vec![
            LineItem {
                description: "Discovery workshop".to_string(),
                quantity: Quantity::whole(2),
                rate: Money::from_euros(150),
            },
            LineItem {
                description: "Live musician performance for the full tallis vestry, \
                              including sound check, rehearsal time, setup and strike of the \
                              stage equipment, and travel within Musterstadt"
                    .to_string(),
                quantity: Quantity::from_thousandths(1_500),
                rate: Money::from_cents(12_345),
            },
            LineItem {
                description: "Consulting".to_string(),
                quantity: Quantity::from_thousandths(2_500),
                rate: Money::from_cents(1_000),
            },
        ];
        invoice
    }

    #[test]
    fn line_items_rule_plan_is_one_accent_hairlines_and_one_closing_rule() {
        for rows in 2..=12 {
            let plan: Vec<RowRule> = (0..rows).filter_map(|row| row_rule(row, rows)).collect();
            let count = |kind: RowRule| plan.iter().filter(|rule| **rule == kind).count();
            assert_eq!(plan.len(), rows, "{rows} rows: one rule per row");
            assert_eq!(count(RowRule::Accent), 1, "{rows} rows: accent rules");
            assert_eq!(count(RowRule::Hairline), rows - 2, "{rows} rows: hairlines");
            assert_eq!(count(RowRule::Closing), 1, "{rows} rows: closing rules");
            assert_eq!(plan[0], RowRule::Accent, "{rows} rows: header rule");
            assert_eq!(
                plan[rows - 1],
                RowRule::Closing,
                "{rows} rows: last row rule"
            );
        }
        assert_eq!(row_rule(3, 3), None, "no rule past the last row");
    }

    #[test]
    fn line_items_rule_colours_match_the_design() {
        assert_eq!(RowRule::Accent.color(), Color::Rgb(0, 184, 217));
        assert_eq!(RowRule::Hairline.color(), Color::Rgb(229, 231, 235));
        assert_eq!(RowRule::Closing.color(), Color::Rgb(209, 213, 219));
    }

    #[test]
    fn line_items_text_keeps_every_row_value() {
        let invoice = three_row_invoice();
        let projection = project(&invoice);
        let text = pdftotext_of("line-items", &render_invoice(&invoice).expect("render"));
        for header in ["Description", "Quantity", "Rate", "Total"] {
            assert!(text.contains(header), "header {header:?} missing:\n{text}");
        }
        for row in &projection.line_items {
            assert!(
                text.contains(&row.quantity),
                "quantity {:?} missing",
                row.quantity
            );
            assert!(text.contains(&row.rate), "rate {:?} missing", row.rate);
            assert!(text.contains(&row.total), "total {:?} missing", row.total);
        }
        // pdftotext breaks the wrapped description across lines, so check the
        // last line of it on its own.
        assert!(
            text.contains("within Musterstadt"),
            "wrapped description lost its last line:\n{text}"
        );
    }

    /// Writes `output-test/pdf-3-2.png`: a three-row invoice with one wrapped
    /// description, rasterized with `raster::rasterize` at 2.0 px/pt (task 3.2).
    /// Ignored by default; run it with `--ignored`.
    #[test]
    #[ignore = "artefact writer; run with --ignored"]
    fn write_line_items_preview_png() {
        use crate::raster::rasterize;

        let bytes = render_invoice(&three_row_invoice()).expect("render three-row invoice");
        let pages = rasterize(&bytes, 2.0).expect("rasterize three-row invoice");
        assert_eq!(pages.len(), 1, "three-row invoice is one page");
        let page = &pages[0];

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output-test");
        std::fs::create_dir_all(&dir).expect("create output-test");
        let file = std::fs::File::create(dir.join("pdf-3-2.png")).expect("create png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), page.width, page.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("png header")
            .write_image_data(&page.rgba)
            .expect("png data");
    }

    /// Writes `output-test/pdf-3-1.png`: the reference invoice rasterized with
    /// `raster::rasterize` at 2.0 px/pt for visual review (task 3.1). Ignored by
    /// default; run it with `--ignored`.
    #[test]
    #[ignore = "artefact writer; run with --ignored"]
    fn write_reference_invoice_preview_png() {
        use crate::raster::rasterize;

        let bytes = render_invoice(&reference_invoice()).expect("render reference invoice");
        let pages = rasterize(&bytes, 2.0).expect("rasterize reference invoice");
        assert_eq!(pages.len(), 1, "reference invoice is one page");
        let page = &pages[0];

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output-test");
        std::fs::create_dir_all(&dir).expect("create output-test");
        let file = std::fs::File::create(dir.join("pdf-3-1.png")).expect("create png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), page.width, page.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .expect("png header")
            .write_image_data(&page.rgba)
            .expect("png data");
    }

    // --- Selectable sender layouts (task 3.3) ---

    /// The reference invoice with the sender's tax number, VAT ID, and email filled
    /// in, in the given sender layout.
    fn sender_invoice(layout: SenderLayout) -> Invoice {
        let mut invoice = reference_invoice();
        invoice.sender.tax_number = "12/345/67890".to_string();
        invoice.sender.vat_id = "DE123456789".to_string();
        invoice.sender.email = "hello@example.de".to_string();
        invoice.sender.layout = layout;
        invoice
    }

    /// `text` with every run of whitespace, newlines included, collapsed to one
    /// space, so a phrase is found whatever line `pdftotext` broke it on.
    fn collapsed(text: &str) -> String {
        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn sender_text(name: &str, invoice: &Invoice) -> String {
        collapsed(&pdftotext_of(name, &render_invoice(invoice).expect("render")))
    }

    #[test]
    fn letter_layout_prints_sender_block_return_line_and_bank_footer() {
        let text = sender_text("letter", &sender_invoice(SenderLayout::Letter));
        for expected in [
            "Alexandra Morgan",
            "Whitfield. 42",
            "10115 Musterstadt",
            "Germany",
            "Tax number 12/345/67890",
            "VAT ID DE123456789",
            "hello@example.de",
            "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt",
            "Ellery",
        ] {
            assert!(text.contains(expected), "{expected:?} missing:\n{text}");
        }
    }

    #[test]
    fn header_layout_prints_street_city_line_and_tax_summary_without_return_line() {
        let text = sender_text("header", &sender_invoice(SenderLayout::Header));
        for expected in [
            "Alexandra Morgan",
            "Whitfield. 42",
            "10115 Musterstadt, Germany",
            "Tax number 12/345/67890 · VAT ID DE123456789 · hello@example.de",
            "Ellery",
        ] {
            assert!(text.contains(expected), "{expected:?} missing:\n{text}");
        }
        assert!(
            !text.contains("Alexandra Morgan · Whitfield. 42"),
            "Bold header must not print the return line:\n{text}"
        );
    }

    #[test]
    fn footer_layout_prints_return_line_above_and_three_columns_below() {
        let invoice = sender_invoice(SenderLayout::Footer);
        let text = sender_text("footer", &invoice);
        for expected in [
            "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt",
            "Whitfield. 42",
            "10115 Musterstadt",
            "Tax number 12/345/67890",
            "VAT ID DE123456789",
            "hello@example.de",
            "Ellery",
            "DE89 3704 0044 0532 0130 00",
        ] {
            assert!(text.contains(expected), "{expected:?} missing:\n{text}");
        }
        assert!(
            text.find("Alexandra Morgan · Whitfield. 42")
                < text.find("Billed to"),
            "return line must come above Billed to:\n{text}"
        );
    }

    /// True when one `pdftotext -layout` line carries the sender name, tax number,
    /// and bank label side by side, which only the Footer columns layout does.
    fn has_three_column_footer(layout_text: &str) -> bool {
        layout_text.lines().any(|line| {
            line.contains("Tax number")
                && line.contains("Bank")
                && line.contains("Alexandra Morgan")
        })
    }

    #[test]
    fn only_footer_layout_has_three_column_footer() {
        let layout_text = |layout: SenderLayout| {
            let name = format!("three-col-{layout:?}");
            let bytes = render_invoice(&sender_invoice(layout)).expect("render");
            pdftotext_with(&name, &bytes, &["-layout"])
        };
        assert!(has_three_column_footer(&layout_text(SenderLayout::Footer)));
        assert!(!has_three_column_footer(&layout_text(SenderLayout::Letter)));
        assert!(!has_three_column_footer(&layout_text(SenderLayout::Header)));
    }

    #[test]
    fn only_letter_and_footer_layouts_print_the_return_line() {
        let return_line = "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt";
        let has_return_line = |name: &str, layout: SenderLayout| {
            sender_text(name, &sender_invoice(layout)).contains(return_line)
        };
        assert!(has_return_line("rl-letter", SenderLayout::Letter));
        assert!(has_return_line("rl-footer", SenderLayout::Footer));
        assert!(!has_return_line("rl-header", SenderLayout::Header));
    }

    #[test]
    fn blank_vat_id_and_email_produce_no_line_in_any_layout() {
        for layout in [SenderLayout::Letter, SenderLayout::Header, SenderLayout::Footer] {
            let mut invoice = reference_invoice();
            invoice.sender.layout = layout;
            let text = sender_text(&format!("blank-{layout:?}"), &invoice);
            assert!(!text.contains("VAT ID"), "{layout:?}: VAT ID line printed:\n{text}");
            assert!(!text.contains('@'), "{layout:?}: email line printed:\n{text}");
            assert!(text.contains("Tax number"), "{layout:?}: tax line missing:\n{text}");
        }
    }

    #[test]
    fn projection_splits_street_and_city_country_for_the_header_layout() {
        let p = project(&reference_invoice());
        assert_eq!(p.sender.street, "Whitfield. 42");
        assert_eq!(p.sender.city_country, "10115 Musterstadt, Germany");
        let mut no_country = reference_invoice();
        no_country.sender.country = "  ".to_string();
        assert_eq!(project(&no_country).sender.city_country, "10115 Musterstadt");
    }

    /// Writes `output-test/pdf-layout-{letter,header,footer}.png`: the same
    /// invoice in each sender layout, rasterized with `raster::rasterize` at 2.0
    /// px/pt (task 3.3). Ignored by default; run it with `--ignored`.
    #[test]
    #[ignore = "artefact writer; run with --ignored"]
    fn write_sender_layout_preview_pngs() {
        use crate::raster::rasterize;

        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output-test");
        std::fs::create_dir_all(&dir).expect("create output-test");
        for (layout, name) in [
            (SenderLayout::Letter, "letter"),
            (SenderLayout::Header, "header"),
            (SenderLayout::Footer, "footer"),
        ] {
            let bytes = render_invoice(&sender_invoice(layout)).expect("render sender layout");
            let pages = rasterize(&bytes, 2.0).expect("rasterize sender layout");
            assert_eq!(pages.len(), 1, "{name}: one page");
            let page = &pages[0];

            let file = std::fs::File::create(dir.join(format!("pdf-layout-{name}.png")))
                .expect("create png");
            let mut encoder =
                png::Encoder::new(std::io::BufWriter::new(file), page.width, page.height);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .expect("png header")
                .write_image_data(&page.rgba)
                .expect("png data");
        }
    }
}

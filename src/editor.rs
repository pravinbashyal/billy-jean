//! GPUI window shell for the invoice editor (tasks 5.1, 5.2, 6.1).
//!
//! A thin wrapper over [`billing::app_state::AppState`]: it lays out the two
//! regions and renders each panel. The field-to-model mapping ([`Field`],
//! [`AppState::set_field`], [`AppState::field_value`]) and the display lines
//! live in the library so they are unit-tested without a window. This file
//! only wires gpui-component inputs to that mapping.
//!
//! Two-way binding: each input is created from the model value
//! ([`AppState::field_value`]); every `InputEvent::Change` writes the new text
//! back with [`AppState::set_field`] and re-renders the view.
//!
//! Line items (task 6.1): every row has description/quantity/rate inputs plus
//! a read-only Total computed by the model. Add and Remove mutate the single
//! `invoice.line_items` list, then the row inputs are rebuilt from the model
//! so each row's change subscription always carries its current index.
//!
//! Live preview (tasks 8.1, 5.1): every edit goes through [`EditorView::model_changed`],
//! which re-renders the form at once and schedules a debounced refresh of the
//! preview. The refresh projects the invoice with [`billing::pdf::project`] and
//! renders that projection to PDF bytes with [`billing::pdf::render_projection`]
//! (the bytes Export PDF writes). It rasterizes them with [`billing::raster`] on the
//! background executor, so the preview shows the exported pages themselves.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use billing::app_state::{AppState, Field, LineItemField, PANEL_ORDER, Panel};
use billing::date::Date as ModelDate;
use billing::export;
use billing::export_gate::{self, ExportIssue};
use billing::model::SenderLayout;
use billing::pdf::{Projection, project, render_projection};
use billing::raster::{self, PageImage};
use chrono::{Local, Months, NaiveDate, TimeDelta, Weekday};
use gpui::{
    AnyElement, ClickEvent, Context, Entity, FocusHandle, IntoElement, KeyBinding, ParentElement,
    Render, RenderImage, ScrollHandle, Styled, Subscription, Task, Window, div, img, prelude::*, px,
};
use gpui_component::ActiveTheme;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::date_picker::{DatePicker, DatePickerEvent, DatePickerState};
use gpui_component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_component::radio::RadioGroup;

// Dev-only: the extra gpui types and the inspect bridge used to synthesize input
// and publish the semantic tree. Compiled only with `--features gpui-mcp`.
#[cfg(feature = "gpui-mcp")]
use gpui::{
    App, Focusable, Keystroke, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PlatformInput, ScrollDelta, ScrollEvent, point, size,
};
#[cfg(feature = "gpui-mcp")]
use crate::inspect::{
    InspectionAction, InspectionActionResult, Inspector, SemanticBounds, SemanticNode, node,
};

/// A 13 px muted field or caption label (design D9). Colour comes from the theme's
/// `muted_foreground`, so the label follows the active light or dark theme.
fn muted_label(text: impl Into<String>, cx: &gpui::App) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// Converts the server's framework-neutral modifier set to GPUI's.
#[cfg(feature = "gpui-mcp")]
fn to_modifiers(m: gpui_mcp::NormalizedModifiers) -> Modifiers {
    Modifiers {
        control: m.control,
        alt: m.alt,
        shift: m.shift,
        platform: m.platform,
        function: m.function,
    }
}

/// Sender panel fields, in display order, with their labels.
const SENDER_FIELDS: [(Field, &str); 7] = [
    (Field::SenderName, "Name"),
    (Field::SenderStreet, "Street"),
    (Field::SenderPostalCodeCity, "Postal code/City"),
    (Field::SenderCountry, "Country"),
    (Field::SenderTaxNumber, "Tax number"),
    (Field::SenderVatId, "VAT ID"),
    (Field::SenderEmail, "Email"),
];

/// The Sender layout choices in the order the radio group shows them (design D10).
const SENDER_LAYOUT_CHOICES: [(SenderLayout, &str); 3] = [
    (SenderLayout::Letter, "German letter"),
    (SenderLayout::Header, "Bold header"),
    (SenderLayout::Footer, "Footer columns"),
];

/// The radio index that shows `layout` as selected.
fn sender_layout_index(layout: SenderLayout) -> usize {
    SENDER_LAYOUT_CHOICES
        .iter()
        .position(|(choice, _)| *choice == layout)
        .unwrap_or(0)
}

/// The option part of a Sender layout radio's semantic id, `sender.layout.<option>`
/// (design D10). The match is exhaustive, so a new layout cannot ship without an id.
#[cfg(feature = "gpui-mcp")]
fn sender_layout_option(layout: SenderLayout) -> &'static str {
    match layout {
        SenderLayout::Letter => "letter",
        SenderLayout::Header => "header",
        SenderLayout::Footer => "footer",
    }
}

/// Bill to panel fields, in display order, with their labels.
const BILL_TO_FIELDS: [(Field, &str); 4] = [
    (Field::ClientName, "Name"),
    (Field::ClientStreet, "Street"),
    (Field::ClientPostalCodeCity, "Postal code/City"),
    (Field::ClientCountry, "Country"),
];

/// Invoice details panel fields, in display order, with their labels.
const INVOICE_DETAIL_FIELDS: [(Field, &str); 4] = [
    (Field::InvoiceNumber, "Invoice number"),
    (Field::InvoiceDate, "Invoice date"),
    (Field::ServiceDate, "Service date"),
    (Field::DueDate, "Due date"),
];

/// The stored and displayed format of every date field (spec "Editing invoice
/// number and dates"). The model keeps this text, so the export gate is unchanged.
const DATE_FORMAT: &str = "%d/%m/%Y";

/// The three invoice dates. Each is a calendar `DatePicker`, not a text input
/// (task 4.2, design D2).
const DATE_FIELDS: [Field; 3] = [Field::InvoiceDate, Field::ServiceDate, Field::DueDate];

fn is_date_field(field: Field) -> bool {
    DATE_FIELDS.contains(&field)
}

/// The calendar date a stored `DD/MM/YYYY` string names, or `None` when it does not
/// parse with [`ModelDate::parse`]. Used to initialise a picker from the model.
fn parse_model_date(text: &str) -> Option<NaiveDate> {
    ModelDate::parse(text).ok()?;
    NaiveDate::parse_from_str(text, DATE_FORMAT).ok()
}

/// The key context of a date field's keyboard wrapper (task 4.3, design D2).
const DATE_FIELD_CONTEXT: &str = "InvoiceDateField";

gpui::actions!(
    invoice_date_field,
    [
        DateDayBack,
        DateDayForward,
        DateWeekBack,
        DateWeekForward,
        DateMonthBack,
        DateMonthForward,
        DateYearBack,
        DateYearForward,
    ]
);

/// One keyboard step on a date field (task 4.3, design D2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DateStep {
    Days(i64),
    Months(i32),
}

impl DateStep {
    /// The date this step lands on. A month or year step clamps to the last day of
    /// the target month (31 Jan + 1 month = 28 Feb, or 29 Feb in a leap year). `None`
    /// only when the result is outside chrono's range.
    fn apply(self, date: NaiveDate) -> Option<NaiveDate> {
        match self {
            DateStep::Days(days) => date.checked_add_signed(TimeDelta::days(days)),
            DateStep::Months(months) => {
                let amount = Months::new(months.unsigned_abs());
                if months < 0 {
                    date.checked_sub_months(amount)
                } else {
                    date.checked_add_months(amount)
                }
            }
        }
    }
}

/// Binds the date keys in the `InvoiceDateField` context: Left/Right one day,
/// Up/Down one week, Page Up/Down one month, Shift+Page Up/Down one year. The stock
/// Enter, Escape, and Delete bindings stay with the picker. Called once at startup.
pub fn bind_date_field_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("left", DateDayBack, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("right", DateDayForward, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("up", DateWeekBack, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("down", DateWeekForward, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("pageup", DateMonthBack, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("pagedown", DateMonthForward, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("shift-pageup", DateYearBack, Some(DATE_FIELD_CONTEXT)),
        KeyBinding::new("shift-pagedown", DateYearForward, Some(DATE_FIELD_CONTEXT)),
    ]);
}

/// The key context of the full-screen preview overlay (task 5.2, design D8). Its Escape
/// binding applies only while the overlay has focus, so it never shadows another
/// context's Escape (the DatePicker's, for one).
const FULLSCREEN_CONTEXT: &str = "FullscreenPreview";

gpui::actions!(
    preview_fullscreen,
    [
        ToggleFullscreen,
        ExitFullscreen,
        FullscreenZoomIn,
        FullscreenZoomOut,
        FullscreenFitWidth,
        FullscreenPageBack,
        FullscreenPageForward,
    ]
);

/// Binds the full-screen preview keys. Cmd+Shift+F toggles the overlay from anywhere in
/// the window (task 5.2). Inside the overlay, Escape exits (5.2); Cmd+= and Cmd+- step the
/// zoom, Cmd+0 returns to Fit width, and Page Up / Page Down move one page (task 5.3).
/// Called once at startup.
pub fn bind_preview_keys(cx: &mut gpui::App) {
    cx.bind_keys([
        KeyBinding::new("cmd-shift-f", ToggleFullscreen, None),
        KeyBinding::new("escape", ExitFullscreen, Some(FULLSCREEN_CONTEXT)),
        KeyBinding::new("cmd-=", FullscreenZoomIn, Some(FULLSCREEN_CONTEXT)),
        KeyBinding::new("cmd--", FullscreenZoomOut, Some(FULLSCREEN_CONTEXT)),
        KeyBinding::new("cmd-0", FullscreenFitWidth, Some(FULLSCREEN_CONTEXT)),
        KeyBinding::new("pageup", FullscreenPageBack, Some(FULLSCREEN_CONTEXT)),
        KeyBinding::new("pagedown", FullscreenPageForward, Some(FULLSCREEN_CONTEXT)),
    ]);
}

/// Bank details panel fields, in display order, with their labels (task 7.1).
const BANK_FIELDS: [(Field, &str); 3] = [
    (Field::BankName, "Bank"),
    (Field::Beneficiary, "Beneficiary"),
    (Field::Iban, "IBAN"),
];

/// Quiet period after the last edit before the preview re-renders (task 8.1).
const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(300);

/// Width of an A4 page in PDF points. The preview scales the page to the column.
const A4_WIDTH_PT: f32 = 595.28;

/// Width of the form column in the two-column layout (see `render`).
const FORM_COLUMN_PX: f32 = 460.;

/// Horizontal padding of the preview column (`p_6` on each side, see `render`).
const PREVIEW_PADDING_PX: f32 = 2. * 24.;

/// A column-width change larger than this re-renders the preview (design D1).
const PREVIEW_RERENDER_PX: f32 = 8.;

/// The narrowest column the preview renders at, so a very small window still gets
/// a usable raster.
const PREVIEW_MIN_COLUMN_PX: f32 = 120.;

/// Horizontal padding of the full-screen page body, on each side (task 5.2, design D8).
const FULLSCREEN_PADDING_PX: f32 = 2. * 24.;

/// The full-screen zoom steps in percent (task 5.2, design D8). 100 % is A4 at one
/// point per logical pixel.
const ZOOM_STEPS: [u16; 7] = [50, 75, 100, 125, 150, 200, 300];

/// Renders one projection to PDF and rasterizes it at `px_per_pt`, returning one
/// GPUI image per page. Pure and thread-safe, so it runs on the background executor
/// (design D1).
type PreviewRenderFn = fn(&Projection, f32) -> Result<Vec<Arc<RenderImage>>, String>;

/// The production preview renderer: [`render_projection`] then [`raster::rasterize`].
/// The PDF bytes are the same ones Export PDF writes for this projection.
fn render_preview(
    projection: &Projection,
    px_per_pt: f32,
) -> Result<Vec<Arc<RenderImage>>, String> {
    let source = render_projection(projection).map_err(|err| err.to_string())?;
    let pages = raster::rasterize(&source, px_per_pt).map_err(|err| err.to_string())?;
    pages
        .iter()
        .map(|page| render_image(page).map(Arc::new))
        .collect()
}

/// Wraps one rasterized page in a GPUI image. GPUI expects BGRA bytes, so the
/// channels are swapped here. The frame keeps the `image` crate's RGBA type only
/// as a byte container.
fn render_image(page: &PageImage) -> Result<RenderImage, String> {
    let mut bytes = page.rgba.clone();
    to_bgra(&mut bytes);
    let buffer = image::RgbaImage::from_raw(page.width, page.height, bytes)
        .ok_or_else(|| "rasterized page has the wrong byte count".to_string())?;
    Ok(RenderImage::new([image::Frame::new(buffer)]))
}

/// Swaps the red and blue channels of every RGBA pixel in place, giving BGRA.
fn to_bgra(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
}

/// The width of the preview's sheets for a window of this size (logical px).
/// Mirrors the two-column layout in `render`: the form column has a fixed width,
/// and the preview column is what remains less its padding.
fn preview_column_width(window: &Window) -> f32 {
    let viewport = f32::from(window.viewport_size().width);
    (viewport - FORM_COLUMN_PX - PREVIEW_PADDING_PX).max(PREVIEW_MIN_COLUMN_PX)
}

/// The state the right-hand preview shows, for the `preview.pages` node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PreviewStatus {
    /// The shown pages are the latest projection.
    Ok,
    /// A newer render is in flight; the previous pages stay on screen.
    Rendering,
    /// The latest render failed; the previous pages stay on screen with a notice.
    Error,
}

/// Which background render a request belongs to (tasks 5.1, 5.3). The column preview and
/// the full-screen overlay each keep their own generation and task slot, so a request on
/// one never supersedes the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderStream {
    Column,
    FullScreen,
}

impl PreviewStatus {
    /// The status word the `preview.pages` node publishes.
    #[cfg_attr(not(feature = "gpui-mcp"), allow(dead_code))]
    const fn label(self) -> &'static str {
        match self {
            PreviewStatus::Ok => "ok",
            PreviewStatus::Rendering => "rendering",
            PreviewStatus::Error => "error",
        }
    }
}

/// The zoom of the full-screen preview (task 5.2, design D8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Zoom {
    /// The page spans the overlay's width. Every entry starts here.
    FitWidth,
    /// A fixed step from [`ZOOM_STEPS`].
    Percent(u16),
}

/// The state of an open full-screen preview (task 5.2, design D8).
struct FullscreenState {
    zoom: Zoom,
    /// The page body's scroll position. Read for the "Page n of N" indicator.
    scroll: ScrollHandle,
    /// Focused while the overlay is open, so its key context is active.
    focus: FocusHandle,
    /// The field that had focus when the overlay opened. Focus returns here on exit.
    restore_focus: Option<FocusHandle>,
}

/// The width (logical px) of one sheet at `zoom`. Fit width uses the overlay's inner width.
fn sheet_width(zoom: Zoom, fit_width_px: f32) -> f32 {
    match zoom {
        Zoom::FitWidth => fit_width_px,
        Zoom::Percent(percent) => A4_WIDTH_PT * f32::from(percent) / 100.,
    }
}

/// The zoom, in percent, that [`sheet_width`] gives for `zoom`.
fn zoom_percent(zoom: Zoom, fit_width_px: f32) -> f32 {
    sheet_width(zoom, fit_width_px) / A4_WIDTH_PT * 100.
}

/// The next zoom step above (`up`) or below `current_percent`, clamped to the ends of
/// [`ZOOM_STEPS`]. A half-percent tolerance keeps a fit-width percentage such as 99.6 %
/// from stepping onto the same step.
fn stepped_zoom(current_percent: f32, up: bool) -> Zoom {
    const TOLERANCE: f32 = 0.5;
    let step = if up {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|&step| f32::from(step) > current_percent + TOLERANCE)
            .unwrap_or(ZOOM_STEPS[ZOOM_STEPS.len() - 1])
    } else {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|&step| f32::from(step) < current_percent - TOLERANCE)
            .unwrap_or(ZOOM_STEPS[0])
    };
    Zoom::Percent(step)
}

/// The 0-based index of the page mostly in view: the page whose vertical extent overlaps
/// the viewport the most. Both are `(top, bottom)` pairs in one coordinate space. Returns
/// 0 when no page overlaps yet, such as before the first layout.
fn page_in_view(viewport: (f32, f32), pages: &[(f32, f32)]) -> usize {
    let mut best = (0, 0.0_f32);
    for (index, &(top, bottom)) in pages.iter().enumerate() {
        let overlap = bottom.min(viewport.1) - top.max(viewport.0);
        if overlap > best.1 {
            best = (index, overlap);
        }
    }
    best.0
}

/// The width the full-screen page body has at fit width for a window that is
/// `viewport_px` wide (logical px).
fn fullscreen_fit_width(viewport_px: f32) -> f32 {
    (viewport_px - FULLSCREEN_PADDING_PX).max(PREVIEW_MIN_COLUMN_PX)
}

/// The pixels per PDF point for a full-screen render at `zoom` (task 5.3, design D8):
/// the sheet's width in logical px over A4's width in points, times the window's scale
/// factor. At 100 % that is `scale_factor`, and at 200 % it is `2 * scale_factor`.
fn fullscreen_px_per_pt(zoom: Zoom, fit_width_px: f32, scale_factor: f32) -> f32 {
    sheet_width(zoom, fit_width_px) / A4_WIDTH_PT * scale_factor
}

/// The vertical padding above the first page in the full-screen body (`py_4`).
const FULLSCREEN_PAGES_TOP_PX: f32 = 16.;

/// Each page's `(top, bottom)` in the body's content coordinates (task 5.3). The
/// extents come from the child bounds laid out last frame, taken relative to the first
/// page. A scroll offset moves every child by the same amount, so these differences
/// stay right after an offset change, before the next layout. Before the first layout
/// every extent is zero.
fn fullscreen_page_extents(state: &FullscreenState, count: usize) -> Vec<(f32, f32)> {
    let laid_out: Vec<(f32, f32)> = (0..count)
        .map(|index| {
            state
                .scroll
                .bounds_for_item(index)
                .map_or((0., 0.), |b| (f32::from(b.top()), f32::from(b.size.height)))
        })
        .collect();
    let first_top = laid_out.first().map_or(0., |&(top, _)| top);
    laid_out
        .iter()
        .map(|&(top, height)| {
            let start = FULLSCREEN_PAGES_TOP_PX + (top - first_top);
            (start, start + height)
        })
        .collect()
}

/// The 0-based index of the page mostly in view (task 5.2, design D8). The viewport is
/// read from the scroll offset, so the index is right as soon as the offset changes.
/// Returns 0 before the first layout.
fn fullscreen_page_index(state: &FullscreenState, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let top = -f32::from(state.scroll.offset().y);
    let height = f32::from(state.scroll.bounds().size.height);
    page_in_view((top, top + height), &fullscreen_page_extents(state, count))
}

/// The "Page n of N" text for the open overlay (task 5.2).
fn fullscreen_page_text(state: &FullscreenState, count: usize) -> String {
    if count == 0 {
        return "No pages".to_string();
    }
    format!("Page {} of {count}", fullscreen_page_index(state, count) + 1)
}

/// The page that Page Up or Page Down moves to from `current` (task 5.3): one page on,
/// clamped to the last page, or one page back, clamped to the first.
fn paged_index(current: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    if forward {
        (current + 1).min(count - 1)
    } else {
        current.saturating_sub(1)
    }
}

/// How far a fit-width or zoom change can move the scale before the full-screen pages
/// are re-rendered (task 5.3). A smaller change keeps the sharp pages already shown.
const FULLSCREEN_RERENDER_TOLERANCE: f32 = 0.01;

/// The Beneficiary placeholder: the sender Name, trimmed as the projection trims it.
fn beneficiary_placeholder(state: &AppState) -> String {
    state.invoice.sender.name.trim().to_string()
}

/// The editable inputs of one line-item row. The Total is not an input.
struct LineItemRowInputs {
    description: Entity<InputState>,
    quantity: Entity<InputState>,
    rate: Entity<InputState>,
}

/// The root view: owns the app state and the input widgets bound to it.
pub struct EditorView {
    state: AppState,
    /// One single-line input per simple text field.
    inputs: Vec<(Field, Entity<InputState>)>,
    /// One calendar picker per invoice date (task 4.2). Each writes its
    /// `DD/MM/YYYY` text, or `""` when cleared, into the model on change.
    date_pickers: Vec<(Field, Entity<DatePickerState>)>,
    /// The multi-line Notes field.
    notes: Entity<TextareaState>,
    /// The Totals panel's VAT-rate control (percent text, e.g. `19` or `7.5`).
    vat_rate_input: Entity<InputState>,
    /// One entry per model line item, in the same order.
    line_item_rows: Vec<LineItemRowInputs>,
    /// The projection the right-hand preview currently shows. Refreshed only by
    /// the debounced task below, so it can briefly lag the model while typing.
    preview: Projection,
    /// The pending debounced preview refresh. Replacing it drops the previous
    /// task, which cancels that task's timer (GPUI cancels on drop).
    preview_refresh: Option<Task<()>>,
    /// The pages of the last successful preview render (task 5.1). Kept through a
    /// failed render, so the preview never goes blank.
    preview_pages: Vec<Arc<RenderImage>>,
    /// Set when the latest render failed; cleared by the next successful render.
    preview_error: bool,
    /// Incremented for every render request. Only a result that carries the latest
    /// value is applied (task 5.1).
    preview_generation: u64,
    /// The generation of the last applied result. It trails `preview_generation`
    /// while a render is in flight.
    preview_applied_generation: u64,
    /// The pending background render. Replacing it cancels the previous render.
    preview_render: Option<Task<()>>,
    /// The column width (logical px) and window scale factor, kept current by the
    /// window-bounds observer. The next render targets them.
    preview_column_px: f32,
    preview_scale: f32,
    /// The column width and scale of the last render request. A change beyond
    /// `PREVIEW_RERENDER_PX` schedules a new render.
    preview_requested_column_px: f32,
    preview_requested_scale: f32,
    /// The renderer for each request. Production uses [`render_preview`]; tests
    /// substitute one that fails.
    preview_renderer: PreviewRenderFn,
    /// The form column's scroll position. It is persistent, so the offset survives the
    /// full-screen overlay unmounting the form (task 5.2).
    form_scroll: ScrollHandle,
    /// The full-screen preview while it is open (task 5.2). `None` shows the form.
    fullscreen: Option<FullscreenState>,
    /// The window's width (logical px), kept current by the window-bounds observer. The
    /// full-screen fit width and its render scale come from it (task 5.3).
    viewport_px: f32,
    /// The pages rendered sharply at the overlay's current zoom (task 5.3). Empty until
    /// the first such render; the overlay shows `preview_pages`, scaled, until then.
    fullscreen_pages: Vec<Arc<RenderImage>>,
    /// The scale (px per PDF point) of the latest full-screen request. Zero before any
    /// request, so the next one always renders.
    fullscreen_requested_px_per_pt: f32,
    /// Incremented for every full-screen request and on exit. Only a result that carries
    /// the latest value is applied (task 5.3).
    fullscreen_generation: u64,
    /// The pending full-screen render. Replacing it cancels the previous one.
    fullscreen_render: Option<Task<()>>,
    /// Focused at startup and tracked by the root element. GPUI dispatches actions from
    /// the focused element up, so this keeps Cmd+Shift+F working before any field has
    /// focus (task 5.2).
    root_focus: FocusHandle,
    /// Directory the identity store saves to after each edit (task 7.2). `None`
    /// means edits are not persisted. `main` sets the per-user config dir; tests
    /// set a scratch dir, so they never write to the real one.
    identity_dir: Option<PathBuf>,
    /// Set by the Export PDF control (task 9.1). Until it is set no export error
    /// is shown, so a fresh form is not covered in errors; once set, every
    /// render re-checks the model, so an error clears as soon as its field is fixed.
    export_attempted: bool,
    /// In-flight save-dialog + write task (task 9.2). Kept so a second click does
    /// not drop a still-running export; replacing it cancels the previous one.
    export_task: Option<Task<()>>,
    /// The failure from the last PDF write, shown in the export bar (task 9.2).
    /// Cleared at the start of every Export PDF click and on a successful write,
    /// so a stale error never outlives the attempt that produced it.
    export_error: Option<String>,
    /// Change subscriptions. They must stay alive or the binding silently stops.
    _subscriptions: Vec<Subscription>,
    /// Subscriptions for the line-item rows. Replaced wholesale on every Add or
    /// Remove, so no subscription survives with an index that no longer matches.
    _line_item_subscriptions: Vec<Subscription>,
    /// Dev-only bridge to the `gpui-mcp` server. `None` until [`Self::attach_inspector`]
    /// runs; once set, every render publishes a semantic snapshot through it.
    #[cfg(feature = "gpui-mcp")]
    inspector: Option<Inspector>,
}

impl EditorView {
    pub fn new(state: AppState, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let preview = project(&state.invoice);
        let mut inputs = Vec::new();
        let mut subscriptions = Vec::new();

        let single_line_fields = SENDER_FIELDS
            .iter()
            .chain(BILL_TO_FIELDS.iter())
            .chain(INVOICE_DETAIL_FIELDS.iter())
            .chain(BANK_FIELDS.iter())
            .filter(|&&(field, _)| !is_date_field(field));
        for &(field, _label) in single_line_fields {
            let initial = state.field_value(field).to_string();
            // Beneficiary shows the sender Name as its placeholder (design D6).
            let placeholder = if field == Field::Beneficiary {
                beneficiary_placeholder(&state)
            } else {
                String::new()
            };
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(initial)
                    .placeholder(placeholder)
            });
            subscriptions.push(cx.subscribe_in(
                &input,
                window,
                move |this: &mut Self, input, event: &InputEvent, window, cx| {
                    if let InputEvent::Change = event {
                        this.state
                            .set_field(field, input.read(cx).value().to_string());
                        if field == Field::SenderName {
                            this.sync_beneficiary_placeholder(window, cx);
                        }
                        this.model_changed(cx);
                    }
                },
            ));
            inputs.push((field, input));
        }

        let mut date_pickers = Vec::new();
        for &field in &DATE_FIELDS {
            let picker = cx.new(|cx| {
                DatePickerState::new(window, cx)
                    .date_format(DATE_FORMAT)
                    .first_day_of_week(Weekday::Mon)
            });
            // A model value that parses shows as the picker's selected day (design D2).
            if let Some(date) = parse_model_date(state.field_value(field)) {
                picker.update(cx, |picker, cx| picker.set_date(date, window, cx));
            }
            subscriptions.push(cx.subscribe_in(
                &picker,
                window,
                move |this: &mut Self, _picker, event: &DatePickerEvent, _window, cx| {
                    let DatePickerEvent::Change(value) = event;
                    let text = value
                        .format(DATE_FORMAT)
                        .map(|text| text.to_string())
                        .unwrap_or_default();
                    this.state.set_field(field, text);
                    this.model_changed(cx);
                },
            ));
            date_pickers.push((field, picker));
        }

        let notes_initial = state.field_value(Field::Notes).to_string();
        let notes = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(3, 8)
                .default_value(notes_initial)
        });
        subscriptions.push(cx.subscribe_in(
            &notes,
            window,
            |this: &mut Self, notes, event: &InputEvent, _window, cx| {
                if let InputEvent::Change = event {
                    this.state
                        .set_field(Field::Notes, notes.read(cx).value().to_string());
                    this.model_changed(cx);
                }
            },
        ));

        let vat_initial = state.vat_rate_text();
        let vat_rate_input = cx.new(|cx| InputState::new(window, cx).default_value(vat_initial));
        subscriptions.push(cx.subscribe_in(
            &vat_rate_input,
            window,
            |this: &mut Self, input, event: &InputEvent, _window, cx| {
                if let InputEvent::Change = event {
                    this.state.set_vat_rate_text(&input.read(cx).value());
                    this.model_changed(cx);
                }
            },
        ));

        let (line_item_rows, line_item_subscriptions) =
            Self::build_line_item_rows(&state, window, cx);

        // A resize changes the preview's column width. The observer re-renders the
        // preview once the width has moved by more than PREVIEW_RERENDER_PX (task 5.1).
        subscriptions.push(cx.observe_window_bounds(
            window,
            |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                this.preview_window_changed(window, cx);
            },
        ));

        let column = preview_column_width(window);
        let scale = window.scale_factor();
        let root_focus = cx.focus_handle();
        window.focus(&root_focus, cx);
        let mut view = EditorView {
            state,
            inputs,
            date_pickers,
            notes,
            vat_rate_input,
            line_item_rows,
            preview,
            preview_refresh: None,
            preview_pages: Vec::new(),
            preview_error: false,
            preview_generation: 0,
            preview_applied_generation: 0,
            preview_render: None,
            preview_column_px: column,
            preview_scale: scale,
            preview_requested_column_px: column,
            preview_requested_scale: scale,
            preview_renderer: render_preview,
            form_scroll: ScrollHandle::new(),
            fullscreen: None,
            viewport_px: f32::from(window.viewport_size().width),
            fullscreen_pages: Vec::new(),
            fullscreen_requested_px_per_pt: 0.,
            fullscreen_generation: 0,
            fullscreen_render: None,
            root_focus,
            identity_dir: None,
            export_attempted: false,
            export_task: None,
            export_error: None,
            _subscriptions: subscriptions,
            _line_item_subscriptions: line_item_subscriptions,
            #[cfg(feature = "gpui-mcp")]
            inspector: None,
        };
        // The first render, so the preview shows pages without waiting for an edit.
        view.schedule_preview_refresh(cx);
        view
    }

    /// Creates the inputs and change subscriptions for every model line item.
    /// Each subscription captures its row index; callers must rebuild all rows
    /// after any structural change so those indices stay valid.
    fn build_line_item_rows(
        state: &AppState,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Vec<LineItemRowInputs>, Vec<Subscription>) {
        let mut rows = Vec::new();
        let mut subscriptions = Vec::new();
        for index in 0..state.invoice.line_items.len() {
            let description = Self::line_item_input(
                state,
                index,
                LineItemField::Description,
                window,
                cx,
                &mut subscriptions,
            );
            let quantity = Self::line_item_input(
                state,
                index,
                LineItemField::Quantity,
                window,
                cx,
                &mut subscriptions,
            );
            let rate = Self::line_item_input(
                state,
                index,
                LineItemField::Rate,
                window,
                cx,
                &mut subscriptions,
            );
            rows.push(LineItemRowInputs {
                description,
                quantity,
                rate,
            });
        }
        (rows, subscriptions)
    }

    /// One row input, initialised from the model, with its Change subscription
    /// writing back to the row at `index`.
    fn line_item_input(
        state: &AppState,
        index: usize,
        field: LineItemField,
        window: &mut Window,
        cx: &mut Context<Self>,
        subscriptions: &mut Vec<Subscription>,
    ) -> Entity<InputState> {
        let initial = state.line_item_text(index, field).unwrap_or_default();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        subscriptions.push(cx.subscribe_in(
            &input,
            window,
            move |this: &mut Self, input, event: &InputEvent, _window, cx| {
                if let InputEvent::Change = event {
                    this.state
                        .set_line_item_text(index, field, &input.read(cx).value());
                    this.model_changed(cx);
                }
            },
        ));
        input
    }

    /// Rebuilds every row's inputs and subscriptions from the current model.
    fn rebuild_line_item_rows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (rows, subscriptions) = Self::build_line_item_rows(&self.state, window, cx);
        self.line_item_rows = rows;
        self._line_item_subscriptions = subscriptions;
    }

    /// Persists the Sender and Bank identity to `dir` (task 7.2). Enables saving
    /// on every edit; `None` disables it.
    pub fn with_identity_dir(mut self, dir: Option<PathBuf>) -> Self {
        self.identity_dir = dir;
        self
    }

    /// Called after every edit to the model. Saves the identity (task 7.2),
    /// re-renders the form immediately, and schedules the debounced preview
    /// refresh (task 8.1). The save runs here, in the edit handler, never in render.
    fn model_changed(&mut self, cx: &mut Context<Self>) {
        self.save_identity();
        cx.notify();
        self.schedule_preview_refresh(cx);
    }

    /// Writes the identity to the configured store. The Sender is always saved;
    /// with an invalid IBAN the last valid Bank is kept (see
    /// `AppState::persist_identity_to`). A failed save never blocks editing; the
    /// error goes to stderr.
    fn save_identity(&self) {
        let Some(dir) = &self.identity_dir else {
            return;
        };
        if let Err(e) = self.state.persist_identity_to(dir) {
            eprintln!("warning: could not save identity: {e}");
        }
    }

    /// Restarts the debounce window. Each call replaces `preview_refresh`, which
    /// drops the previous pending task and cancels its timer, so a burst of edits
    /// refreshes the preview once, `PREVIEW_DEBOUNCE` after the last one. The
    /// refresh re-projects the current `Invoice`, the same projection export uses,
    /// then starts a background render of it (task 5.1).
    fn schedule_preview_refresh(&mut self, cx: &mut Context<Self>) {
        self.preview_refresh = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            this.update(cx, |view, cx| {
                let projection = project(&view.state.invoice);
                // A width-only refresh keeps the same projection. Only a changed projection
                // forces the open overlay to re-render; otherwise the fit-width tolerance
                // in `request_fullscreen_render` decides.
                if projection != view.preview {
                    view.preview = projection;
                    view.fullscreen_requested_px_per_pt = 0.;
                }
                view.start_preview_render(cx);
                // An open overlay shows the new projection too, at its current zoom.
                view.request_fullscreen_render(cx);
                cx.notify();
            })
            .ok();
        }));
    }

    /// Starts a render of the current column projection at the column's pixel scale
    /// (task 5.1). The column's request is recorded so a width change can be measured
    /// against it.
    fn start_preview_render(&mut self, cx: &mut Context<Self>) {
        self.preview_requested_column_px = self.preview_column_px;
        self.preview_requested_scale = self.preview_scale;
        let px_per_pt = self.preview_px_per_pt();
        self.spawn_render(RenderStream::Column, px_per_pt, cx);
    }

    /// Starts a background render of the current `preview` projection for `stream`
    /// (tasks 5.1, 5.3). The request takes the stream's next generation and its task
    /// replaces the stream's previous one, which cancels it. Only the newest request's
    /// result is applied (see [`Self::finish_render`]).
    fn spawn_render(&mut self, stream: RenderStream, px_per_pt: f32, cx: &mut Context<Self>) {
        let generation = self.bump_generation(stream);
        let projection = self.preview.clone();
        let render = self.preview_renderer;
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { render(&projection, px_per_pt) })
                .await;
            this.update(cx, |view, cx| {
                view.finish_render(stream, generation, result, cx)
            })
            .ok();
        });
        match stream {
            RenderStream::Column => self.preview_render = Some(task),
            RenderStream::FullScreen => self.fullscreen_render = Some(task),
        }
    }

    /// Advances the generation of `stream` and returns the new value.
    fn bump_generation(&mut self, stream: RenderStream) -> u64 {
        match stream {
            RenderStream::Column => self.preview_generation += 1,
            RenderStream::FullScreen => self.fullscreen_generation += 1,
        }
        self.latest_generation(stream)
    }

    /// The generation of the newest request of `stream`.
    fn latest_generation(&self, stream: RenderStream) -> u64 {
        match stream {
            RenderStream::Column => self.preview_generation,
            RenderStream::FullScreen => self.fullscreen_generation,
        }
    }

    /// Applies a finished render of `stream` (tasks 5.1, 5.3). A result is dropped when a
    /// newer request of its stream has started, and a full-screen result is also dropped
    /// once the overlay has closed. The two streams differ only in how a failure shows:
    /// the column keeps its last good pages and sets the notice; the overlay logs and
    /// clears its sharp pages, so it falls back to the column pages.
    fn finish_render(
        &mut self,
        stream: RenderStream,
        generation: u64,
        result: Result<Vec<Arc<RenderImage>>, String>,
        cx: &mut Context<Self>,
    ) {
        let closed_overlay = stream == RenderStream::FullScreen && self.fullscreen.is_none();
        if generation != self.latest_generation(stream) || closed_overlay {
            return;
        }
        match stream {
            RenderStream::Column => {
                self.preview_applied_generation = generation;
                match result {
                    Ok(pages) => {
                        self.preview_pages = pages;
                        self.preview_error = false;
                    }
                    Err(message) => {
                        eprintln!("warning: preview could not be updated: {message}");
                        self.preview_error = true;
                    }
                }
            }
            RenderStream::FullScreen => match result {
                Ok(pages) => self.fullscreen_pages = pages,
                Err(message) => {
                    eprintln!("warning: full-screen preview could not be re-rendered: {message}");
                    // Drop the sharp pages of an older projection, so the overlay shows the
                    // column pages (which follow the projection) rather than stale ones.
                    self.fullscreen_pages.clear();
                }
            },
        }
        cx.notify();
    }

    /// The scale the open overlay's pages are rendered at, or `None` while it is closed
    /// (task 5.3, design D8).
    fn fullscreen_scale(&self) -> Option<f32> {
        let state = self.fullscreen.as_ref()?;
        Some(fullscreen_px_per_pt(
            state.zoom,
            fullscreen_fit_width(self.viewport_px),
            self.preview_scale,
        ))
    }

    /// Re-renders the projection sharply at the overlay's current scale (task 5.3). The
    /// render runs on the background executor like the column render. A request within
    /// `FULLSCREEN_RERENDER_TOLERANCE` of the last one is skipped, and the previous pages
    /// stay on screen until a newer render is applied.
    fn request_fullscreen_render(&mut self, cx: &mut Context<Self>) {
        let Some(px_per_pt) = self.fullscreen_scale() else {
            return;
        };
        let last = self.fullscreen_requested_px_per_pt;
        if last > 0. && (px_per_pt - last).abs() <= last * FULLSCREEN_RERENDER_TOLERANCE {
            return;
        }
        self.fullscreen_requested_px_per_pt = px_per_pt;
        self.spawn_render(RenderStream::FullScreen, px_per_pt, cx);
    }

    /// The pages the overlay draws: the sharp ones for its scale when they have arrived,
    /// otherwise the column pages, scaled (task 5.3).
    fn fullscreen_shown_pages(&self) -> &[Arc<RenderImage>] {
        if self.fullscreen_pages.is_empty() {
            &self.preview_pages
        } else {
            &self.fullscreen_pages
        }
    }

    /// The pixels per PDF point for the current column and scale (design D1).
    fn preview_px_per_pt(&self) -> f32 {
        self.preview_column_px * self.preview_scale / A4_WIDTH_PT
    }

    /// Called when the window's bounds change. Records the new column width and
    /// scale, and schedules a re-render through the debounce once the width has
    /// moved by more than `PREVIEW_RERENDER_PX` (design D1).
    fn preview_window_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.preview_column_px = preview_column_width(window);
        self.preview_scale = window.scale_factor();
        self.viewport_px = f32::from(window.viewport_size().width);
        let moved = (self.preview_column_px - self.preview_requested_column_px).abs();
        if moved > PREVIEW_RERENDER_PX || self.preview_scale != self.preview_requested_scale {
            self.schedule_preview_refresh(cx);
        }
        // An open overlay's fit width or scale may have changed too (task 5.3). The
        // tolerance inside the request keeps small drags from re-rendering.
        self.request_fullscreen_render(cx);
    }

    /// Whether the preview shows the latest projection, has a render in flight (or
    /// none has finished yet), or last failed (task 5.1).
    fn preview_status(&self) -> PreviewStatus {
        if self.preview_error {
            PreviewStatus::Error
        } else if self.preview_applied_generation != self.preview_generation
            || self.preview_pages.is_empty()
        {
            PreviewStatus::Rendering
        } else {
            PreviewStatus::Ok
        }
    }

    /// Keeps the Beneficiary placeholder equal to the sender Name (design D6).
    /// Called whenever the Name input changes.
    fn sync_beneficiary_placeholder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let name = beneficiary_placeholder(&self.state);
        self.input_for(Field::Beneficiary)
            .update(cx, |input, cx| input.set_placeholder(name, window, cx));
    }

    /// Writes the layout chosen in the "Sender layout" radio group and saves
    /// the identity through the same path as every other Sender edit.
    fn sender_layout_chosen(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(&(layout, _)) = SENDER_LAYOUT_CHOICES.get(index) else {
            return;
        };
        self.state.set_sender_layout(layout);
        self.model_changed(cx);
    }

    /// The muted return-line caption under the Sender fields (design D6), or
    /// `None` while every segment is blank.
    fn sender_caption(&self) -> Option<String> {
        let line = self.state.return_line();
        (!line.is_empty()).then_some(line)
    }

    /// The horizontal "Sender layout" radio group (`sender.layout`, design D10).
    fn sender_layout_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = sender_layout_index(self.state.invoice.sender.layout);
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(muted_label("Sender layout", cx))
            .child(
                RadioGroup::horizontal("sender.layout")
                    .children(SENDER_LAYOUT_CHOICES.iter().map(|(_, label)| *label))
                    .selected_index(Some(selected))
                    .on_change(cx.listener(|this: &mut Self, index: &usize, _window, cx| {
                        this.sender_layout_chosen(*index, cx);
                    })),
            )
    }

    fn input_for(&self, field: Field) -> &Entity<InputState> {
        self.inputs
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, input)| input)
            .expect("every simple text field has an input")
    }

    fn date_picker_for(&self, field: Field) -> &Entity<DatePickerState> {
        self.date_pickers
            .iter()
            .find(|(f, _)| *f == field)
            .map(|(_, picker)| picker)
            .expect("every date field has a date picker")
    }

    /// The export issues to show inline (task 9.1). Empty until Export PDF has
    /// been clicked; afterwards it is re-derived from the model on every call.
    fn export_issues(&self) -> Vec<ExportIssue> {
        if self.export_attempted {
            export_gate::check(&self.state.invoice)
        } else {
            Vec::new()
        }
    }

    /// The inline error beneath a field. The export gate's message wins once
    /// export has been attempted. For the IBAN, the live check from task 7.1
    /// shows otherwise. Both use the same message text, so they never double up.
    fn field_error(&self, field: Field) -> Option<String> {
        if let Some(issue) = self
            .export_issues()
            .into_iter()
            .find(|issue| issue.field() == Some(field))
        {
            return Some(issue.to_string());
        }
        if field == Field::Iban {
            return self.state.iban_error().map(|error| error.to_string());
        }
        None
    }

    /// One labelled field. Date fields get a calendar picker (no typed entry); every
    /// other field gets a text input. The inline export error sits beneath either.
    fn field_row(&self, label: &str, field: Field, cx: &Context<Self>) -> AnyElement {
        let error = self.field_error(field).map(|text| error_line(text, cx));
        let control = if is_date_field(field) {
            self.date_field_control(field, cx)
        } else {
            Input::new(self.input_for(field)).into_any_element()
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(muted_label(label, cx))
            .child(control)
            .children(error)
            .into_any_element()
    }

    /// A date field's calendar picker inside its keyboard wrapper (task 4.3, design D2).
    /// The wrapper's key context carries the step actions; the picker's own Enter,
    /// Escape, and Delete bindings are untouched.
    fn date_field_control(&self, field: Field, cx: &Context<Self>) -> AnyElement {
        div()
            .w_full()
            .key_context(DATE_FIELD_CONTEXT)
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateDayBack, window, cx| {
                    this.step_date(field, DateStep::Days(-1), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateDayForward, window, cx| {
                    this.step_date(field, DateStep::Days(1), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateWeekBack, window, cx| {
                    this.step_date(field, DateStep::Days(-7), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateWeekForward, window, cx| {
                    this.step_date(field, DateStep::Days(7), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateMonthBack, window, cx| {
                    this.step_date(field, DateStep::Months(-1), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateMonthForward, window, cx| {
                    this.step_date(field, DateStep::Months(1), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateYearBack, window, cx| {
                    this.step_date(field, DateStep::Months(-12), window, cx);
                }),
            )
            .on_action(
                cx.listener(move |this: &mut Self, _: &DateYearForward, window, cx| {
                    this.step_date(field, DateStep::Months(12), window, cx);
                }),
            )
            .child(
                DatePicker::new(self.date_picker_for(field))
                    .placeholder("DD/MM/YYYY")
                    .cleanable(true),
            )
            .into_any_element()
    }

    /// Moves a date field by one keyboard step (task 4.3, design D2). An empty or
    /// unparseable field takes today's date. `set_date` does not emit `Change`, so
    /// the model is written here, once, and the save and preview run as for any edit.
    ///
    /// Known limit (PO-waived, see design D2): gpui-component 0.9's `set_date` also sets
    /// the picker's `open` flag to false, so a step while the popover is open closes
    /// it. The D2 reopen cannot be done conditionally: the only reopen route is the
    /// picker's Confirm action, which toggles from the last rendered `open` value, and
    /// that value is private, so the prior state cannot be known.
    fn step_date(
        &mut self,
        field: Field,
        step: DateStep,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let next = match parse_model_date(self.state.field_value(field)) {
            Some(current) => step.apply(current),
            None => Some(Local::now().date_naive()),
        };
        let Some(next) = next else {
            return;
        };
        let picker = self.date_picker_for(field).clone();
        picker.update(cx, |picker, cx| picker.set_date(next, window, cx));
        self.state
            .set_field(field, next.format(DATE_FORMAT).to_string());
        self.model_changed(cx);
    }

    fn fields_body(&self, fields: &[(Field, &str)], cx: &Context<Self>) -> impl IntoElement {
        div().flex().flex_col().gap_3().children(
            fields
                .iter()
                .map(|&(field, label)| self.field_row(label, field, cx)),
        )
    }

    /// The Invoice details panel as a 2x2 grid, two fields per row, in the order
    /// Invoice number / Invoice date, then Service date / Due date (design D9).
    fn invoice_details_body(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_3()
            .children(INVOICE_DETAIL_FIELDS.chunks(2).map(|pair| {
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .children(pair.iter().map(|&(field, label)| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(self.field_row(label, field, cx))
                    }))
            }))
    }

    /// A captioned element used for the line-item row cells.
    fn captioned(label: &str, child: impl IntoElement, cx: &gpui::App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .child(muted_label(label, cx))
            .child(child)
    }

    /// The Line items panel: one editable block per row, a read-only Total per
    /// row, a Remove control per row, and an Add item control.
    fn line_items_body(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // The only panel-level export issue is "no line items" (task 9.1).
        let panel_error = self
            .export_issues()
            .into_iter()
            .find(|issue| issue.field().is_none())
            .map(|issue| error_line(issue.to_string(), cx));
        let border = cx.theme().border;
        let rows = self.line_item_rows.iter().enumerate().map(|(index, row)| {
            let total = self
                .state
                .line_item_total(index)
                .map(|money| money.to_string())
                .unwrap_or_default();
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_2()
                .border_1()
                .border_color(border)
                .rounded_md()
                .child(
                    div()
                        .text_size(px(14.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(format!("Item {}", index + 1)),
                )
                .child(Self::captioned(
                    "Description",
                    Input::new(&row.description),
                    cx,
                ))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(Self::captioned("Quantity", Input::new(&row.quantity), cx))
                        .child(Self::captioned("Rate (EUR)", Input::new(&row.rate), cx)),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .child(div().text_sm().child(format!("Total: {total}")))
                        .child(
                            Button::new(format!("remove-line-item-{index}"))
                                .label("Remove")
                                .ghost()
                                .on_click(cx.listener(
                                    move |this: &mut Self, _: &ClickEvent, window, cx| {
                                        if this.state.remove_line_item(index).is_some() {
                                            this.rebuild_line_item_rows(window, cx);
                                            this.model_changed(cx);
                                        }
                                    },
                                )),
                        ),
                )
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(panel_error)
            .children(rows)
            .child(
                div().flex().flex_row().child(
                    Button::new("add-line-item")
                        .label("Add item")
                        .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, window, cx| {
                            this.state.add_line_item();
                            this.rebuild_line_item_rows(window, cx);
                            this.model_changed(cx);
                        })),
                ),
            )
    }

    /// The Totals panel: the VAT-rate control and the Net/VAT/Gross lines. Every
    /// value is read from the model on each render, so a line-item or VAT edit is
    /// reflected as soon as the view re-renders. The §19 note lives in Notes.
    fn totals_body(&self, cx: &gpui::App) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Self::captioned(
                "VAT rate (%)",
                Input::new(&self.vat_rate_input),
                cx,
            ))
            .children(
                self.state
                    .panel_lines(Panel::Totals)
                    .into_iter()
                    .map(|line| div().text_sm().child(line)),
            )
    }

    /// The standing §19 UStG note shown read-only in the Notes panel and the
    /// preview's Notes section. `Some` only at 0% VAT; single-sourced through
    /// `AppState::standing_note`, so the PDF and the form agree.
    fn notes_standing_note(&self) -> Option<&'static str> {
        self.state.standing_note()
    }

    /// The Bank details panel: Bank and Beneficiary inputs, the IBAN input with
    /// its grouped readout and inline validation error beneath it (task 7.1).
    /// No bank name or beneficiary is hard-coded; every value is model data.
    fn bank_body(&self, cx: &Context<Self>) -> impl IntoElement {
        let grouped = self.state.iban_grouped();
        let readout = (!grouped.is_empty()).then(|| {
            div()
                .text_size(px(13.))
                .text_color(cx.theme().muted_foreground)
                .child(format!("Displayed as: {grouped}"))
        });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.fields_body(&BANK_FIELDS[..2], cx))
            // The field row carries the IBAN's inline error beneath the input.
            .child(self.field_row(BANK_FIELDS[2].1, BANK_FIELDS[2].0, cx))
            .children(readout)
    }

    /// The Export PDF control, its summary message, and the valid-state status
    /// (tasks 9.1 / 9.2). On a valid invoice the click opens a native save
    /// dialog; on an invalid one it only surfaces the gate errors.
    fn export_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let issues = self.export_issues();
        let summary = export_gate::summary(&issues).map(|text| error_line(text, cx));
        let write_error = self.export_error.clone().map(|text| error_line(text, cx));
        let valid = (self.export_attempted && issues.is_empty() && self.export_error.is_none())
            .then(|| {
                div()
                    .text_sm()
                    .text_color(cx.theme().success)
                    .child("All required fields are valid.")
            });
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .bg(cx.theme().secondary)
            .child(
                Button::new("export-pdf")
                    .label("Export PDF")
                    .primary()
                    .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _window, cx| {
                        this.on_export_pdf(cx);
                    })),
            )
            .children(summary)
            .children(write_error)
            .children(valid)
    }

    /// Export PDF click handler (tasks 9.1 / 9.2). Always marks the attempt so
    /// inline errors appear; opens the save dialog only when the gate passes.
    fn on_export_pdf(&mut self, cx: &mut Context<Self>) {
        self.export_attempted = true;
        self.export_error = None;
        cx.notify();
        if !export_gate::check(&self.state.invoice).is_empty() {
            return;
        }
        self.begin_export(cx);
    }

    /// Opens the native save dialog (default filename + last-used folder) and,
    /// on confirmation, writes a PDF matching the live preview projection.
    fn begin_export(&mut self, cx: &mut Context<Self>) {
        // Snapshot at click time. The dialog is async and the form stays editable
        // while it is open, so the write must use exactly what the gate validated.
        let snapshot = self.state.invoice.clone();
        let suggested = export::default_filename_for(&snapshot);
        let directory = export::dialog_directory(self.identity_dir.as_deref());
        let receiver = cx.prompt_for_new_path(&directory, Some(&suggested));
        self.export_task = Some(cx.spawn(async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(path))) => path,
                // Cancelled, picker error, or app teardown — do not write.
                _ => return,
            };
            this.update(cx, |view, cx| {
                match export::write_invoice_pdf(&snapshot, &path) {
                    Ok(_projection) => {
                        view.export_error = None;
                        export::remember_export_path(view.identity_dir.as_deref(), &path);
                    }
                    Err(e) => {
                        view.export_error = Some(format!("Export failed: {e}"));
                    }
                }
                cx.notify();
            })
            .ok();
        }));
    }

    /// The body of one panel. The simple text panels, Line items, Totals, Notes
    /// and Bank details render editable inputs; the Sender panel also renders
    /// its return-line caption and the Sender layout control.
    fn panel_body(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match panel {
            Panel::Sender => div()
                .flex()
                .flex_col()
                .gap_3()
                .child(self.fields_body(&SENDER_FIELDS, cx))
                .children(
                    self.sender_caption()
                        .map(|line| div().text_size(px(13.)).text_color(muted).child(line)),
                )
                .child(self.sender_layout_body(cx))
                .into_any_element(),
            Panel::BillTo => self.fields_body(&BILL_TO_FIELDS, cx).into_any_element(),
            Panel::InvoiceDetails => self.invoice_details_body(cx).into_any_element(),
            Panel::LineItems => self.line_items_body(cx).into_any_element(),
            Panel::Notes => {
                let standing = self
                    .notes_standing_note()
                    .map(|note| div().text_size(px(13.)).text_color(muted).child(note));
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(standing)
                    .child(Textarea::new(&self.notes))
                    .into_any_element()
            }
            Panel::Totals => self.totals_body(cx).into_any_element(),
            Panel::BankDetails => self.bank_body(cx).into_any_element(),
        }
    }

    /// One editor panel: a 14 px SemiBold title over the panel body, on the theme's
    /// background with a theme border and the 16 px panel padding (design D9).
    fn render_panel(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let (border, background) = (cx.theme().border, cx.theme().background);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(background)
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(panel.title()),
            )
            .child(self.panel_body(panel, cx))
            .into_any_element()
    }
}

/// The full-screen preview (task 5.2, design D8). The overlay replaces the two-column
/// layout. Opening it remembers the focused field, and closing it returns focus there.
/// The form's scroll offset survives because `form_scroll` outlives the unmounted form.
impl EditorView {
    /// Cmd+Shift+F: opens the overlay when closed, closes it when open.
    fn toggle_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.fullscreen.is_some() {
            self.exit_fullscreen(window, cx);
        } else {
            self.enter_fullscreen(window, cx);
        }
    }

    /// Opens the overlay at fit width, focusing it so its Escape binding is active.
    fn enter_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.fullscreen.is_some() {
            return;
        }
        let restore_focus = window.focused(cx);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.fullscreen = Some(FullscreenState {
            zoom: Zoom::FitWidth,
            scroll: ScrollHandle::new(),
            focus,
            restore_focus,
        });
        // The column pages show, scaled, until the sharp render at fit width arrives.
        self.fullscreen_pages.clear();
        self.fullscreen_requested_px_per_pt = 0.;
        self.request_fullscreen_render(cx);
        cx.notify();
    }

    /// Closes the overlay and gives focus back to the field that had it. The form's
    /// scroll offset is already kept in `form_scroll`, so it is restored by the remount.
    fn exit_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.fullscreen.take() else {
            return;
        };
        // Drop the sharp pages and any render still in flight (task 5.3).
        self.fullscreen_pages.clear();
        self.fullscreen_requested_px_per_pt = 0.;
        self.fullscreen_generation += 1;
        self.fullscreen_render = None;
        if let Some(handle) = state.restore_focus {
            window.focus(&handle, cx);
        }
        cx.notify();
    }

    /// Export PDF from the overlay's toolbar (task 5.2). It runs the same gate and save
    /// dialog as the export bar. When the gate blocks, the overlay closes so the inline
    /// errors in the form are visible.
    fn fullscreen_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let blocked = !export_gate::check(&self.state.invoice).is_empty();
        self.on_export_pdf(cx);
        if blocked {
            self.exit_fullscreen(window, cx);
        }
    }

    /// Steps the overlay's zoom one step up or down from its current percentage, then
    /// re-renders the pages sharply at the new scale (task 5.3).
    fn step_fullscreen_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let fit = fullscreen_fit_width(self.viewport_px);
        let Some(state) = self.fullscreen.as_mut() else {
            return;
        };
        state.zoom = stepped_zoom(zoom_percent(state.zoom, fit), up);
        self.request_fullscreen_render(cx);
        cx.notify();
    }

    /// Returns the overlay to fit width and re-renders at that scale (task 5.3).
    fn fit_fullscreen_width(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.fullscreen.as_mut() else {
            return;
        };
        state.zoom = Zoom::FitWidth;
        self.request_fullscreen_render(cx);
        cx.notify();
    }

    /// Moves the overlay's view one page up or down (task 5.3). The target page's top
    /// becomes the top of the body. The offset is set now, not on the next layout, so
    /// the "Page n of N" indicator follows in the same repaint.
    fn page_fullscreen(&mut self, forward: bool, cx: &mut Context<Self>) {
        let count = self.fullscreen_shown_pages().len();
        let Some(state) = self.fullscreen.as_ref() else {
            return;
        };
        let target = paged_index(fullscreen_page_index(state, count), count, forward);
        if let Some(&(top, _)) = fullscreen_page_extents(state, count).get(target) {
            let x = state.scroll.offset().x;
            state.scroll.set_offset(gpui::point(x, px(-top)));
        }
        cx.notify();
    }

    /// The row above the preview column that holds the "Full-screen preview" button.
    fn preview_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div().flex().flex_row().justify_end().p_4().child(
            div()
                .debug_selector(|| "preview.fullscreen".to_string())
                .child(
                    Button::new("preview-fullscreen")
                        .label("Full-screen preview")
                        .ghost()
                        .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, window, cx| {
                            this.enter_fullscreen(window, cx);
                        })),
                ),
        )
    }

    /// The full-screen overlay (task 5.2, design D8): a toolbar over the page body. The
    /// body is a persistent scroll container with one child per page, so each page's
    /// laid-out bounds can give the page-in-view indicator.
    fn fullscreen_overlay(
        &self,
        state: &FullscreenState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let fit = fullscreen_fit_width(self.viewport_px);
        let width = sheet_width(state.zoom, fit);
        let pages = self.fullscreen_shown_pages();
        let count = pages.len();
        let zoom_label = format!("{:.0}%", zoom_percent(state.zoom, fit));
        let page_label = fullscreen_page_text(state, count);

        let toolbar = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .p_4()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                div()
                    .debug_selector(|| "fullscreen.close".to_string())
                    .child(
                        Button::new("fullscreen-close")
                            .label("Close")
                            .ghost()
                            .on_click(cx.listener(
                                |this: &mut Self, _: &ClickEvent, window, cx| {
                                    this.exit_fullscreen(window, cx);
                                },
                            )),
                    ),
            )
            .child(
                Button::new("fullscreen-zoom-out")
                    .label("Zoom out")
                    .ghost()
                    .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _window, cx| {
                        this.step_fullscreen_zoom(false, cx);
                    })),
            )
            .child(div().text_sm().w(px(56.)).child(zoom_label))
            .child(
                Button::new("fullscreen-zoom-in")
                    .label("Zoom in")
                    .ghost()
                    .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _window, cx| {
                        this.step_fullscreen_zoom(true, cx);
                    })),
            )
            .child(
                Button::new("fullscreen-fit-width")
                    .label("Fit width")
                    .ghost()
                    .on_click(cx.listener(|this: &mut Self, _: &ClickEvent, _window, cx| {
                        this.fit_fullscreen_width(cx);
                    })),
            )
            .child(div().text_sm().child(page_label))
            .child(div().flex_1())
            .child(
                div()
                    .debug_selector(|| "fullscreen.export-pdf".to_string())
                    .child(
                        Button::new("fullscreen-export-pdf")
                            .label("Export PDF")
                            .primary()
                            .on_click(cx.listener(
                                |this: &mut Self, _: &ClickEvent, window, cx| {
                                    this.fullscreen_export(window, cx);
                                },
                            )),
                    ),
            );

        let notice = (self.preview_status() == PreviewStatus::Error)
            .then(|| div().px_6().pt_4().child(preview_error_notice(cx)));

        // Each sheet is drawn at the overlay's width. The image is the sharp render for
        // that width once it has arrived; until then it is the column render, scaled.
        let sheets: Vec<AnyElement> = pages
            .iter()
            .enumerate()
            .map(|(index, page)| page_sheet(page, width, index, count, cx))
            .collect();

        // The body scrolls both ways: a page wider than the window scrolls horizontally
        // (task 5.3). A scroll moves the page in view, so the label repaints after it.
        let body = div()
            .id("fullscreen-pages")
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .track_scroll(&state.scroll)
            .on_scroll(cx.listener(|_: &mut Self, _: &gpui::ScrollEvent, _window, cx| {
                cx.notify();
            }))
            .px_6()
            .py_4()
            .flex()
            .flex_col()
            .gap_4()
            .children(sheets);

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().secondary)
            .key_context(FULLSCREEN_CONTEXT)
            .track_focus(&state.focus)
            .on_action(
                cx.listener(|this: &mut Self, _: &ExitFullscreen, window, cx| {
                    this.exit_fullscreen(window, cx);
                }),
            )
            .on_action(cx.listener(|this: &mut Self, _: &FullscreenZoomIn, _window, cx| {
                this.step_fullscreen_zoom(true, cx);
            }))
            .on_action(cx.listener(|this: &mut Self, _: &FullscreenZoomOut, _window, cx| {
                this.step_fullscreen_zoom(false, cx);
            }))
            .on_action(cx.listener(|this: &mut Self, _: &FullscreenFitWidth, _window, cx| {
                this.fit_fullscreen_width(cx);
            }))
            .on_action(cx.listener(|this: &mut Self, _: &FullscreenPageBack, _window, cx| {
                this.page_fullscreen(false, cx);
            }))
            .on_action(cx.listener(|this: &mut Self, _: &FullscreenPageForward, _window, cx| {
                this.page_fullscreen(true, cx);
            }))
            .child(toolbar)
            .children(notice)
            .child(body)
    }

    /// The two-column editor layout: the form on the left, the preview on the right.
    fn editor_columns(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Left region: the Export PDF bar stays fixed at the top, and the form
        // below it scrolls vertically with panels in spec order.
        let form = div()
            .id("form-column")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.form_scroll)
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .bg(cx.theme().secondary)
            .children(
                PANEL_ORDER
                    .iter()
                    .map(|panel| self.render_panel(*panel, cx)),
            );
        let left = div()
            .w(px(FORM_COLUMN_PX))
            .h_full()
            .flex()
            .flex_col()
            .bg(cx.theme().secondary)
            .child(self.export_bar(cx))
            .child(form);

        // Right region: the live preview, the rasterized pages of the last render.
        // The canvas takes the theme's secondary token; the sheets bring their own
        // white page and shadow. The button row sits above the scrolling sheets.
        let preview = div()
            .flex_1()
            .h_full()
            .flex()
            .flex_col()
            .bg(cx.theme().secondary)
            .child(self.preview_header(cx))
            .child(
                div()
                    .id("preview-column")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_6()
                    .child(self.preview_sheets(cx)),
            );

        div()
            .size_full()
            .flex()
            .flex_row()
            .child(left)
            .child(preview)
    }
}

/// Dev-only inspection bridge (`--features gpui-mcp`).
///
/// `inspect_publish` mirrors the model into a [`SemanticTree`](crate::inspect::SemanticTree)
/// the server reads; `inspect_execute` is the only place server actions mutate the
/// view, always on the window's foreground executor (see [`crate::inspect::drive`]).
/// Node ids are stable and model-derived so clients target fields by id, not bounds.
#[cfg(feature = "gpui-mcp")]
impl EditorView {
    /// Installs the inspector; the next render publishes the first snapshot.
    pub(crate) fn attach_inspector(&mut self, inspector: Inspector) {
        self.inspector = Some(inspector);
    }

    /// The stable semantic id for a simple text field.
    const fn field_id(field: Field) -> &'static str {
        match field {
            Field::SenderName => "sender.name",
            Field::SenderStreet => "sender.street",
            Field::SenderPostalCodeCity => "sender.postal_code_city",
            Field::SenderCountry => "sender.country",
            Field::SenderTaxNumber => "sender.tax_number",
            Field::SenderVatId => "sender.vat_id",
            Field::SenderEmail => "sender.email",
            Field::ClientName => "bill_to.name",
            Field::ClientStreet => "bill_to.street",
            Field::ClientPostalCodeCity => "bill_to.postal_code_city",
            Field::ClientCountry => "bill_to.country",
            Field::InvoiceNumber => "invoice.number",
            Field::InvoiceDate => "invoice.date",
            Field::ServiceDate => "invoice.service_date",
            Field::DueDate => "invoice.due_date",
            Field::Notes => "notes",
            Field::BankName => "bank.name",
            Field::Beneficiary => "bank.beneficiary",
            Field::Iban => "bank.iban",
        }
    }

    /// The simple text field for a semantic id, if any (inverse of [`Self::field_id`]).
    fn field_from_id(id: &str) -> Option<Field> {
        Some(match id {
            "sender.name" => Field::SenderName,
            "sender.street" => Field::SenderStreet,
            "sender.postal_code_city" => Field::SenderPostalCodeCity,
            "sender.country" => Field::SenderCountry,
            "sender.tax_number" => Field::SenderTaxNumber,
            "sender.vat_id" => Field::SenderVatId,
            "sender.email" => Field::SenderEmail,
            "bill_to.name" => Field::ClientName,
            "bill_to.street" => Field::ClientStreet,
            "bill_to.postal_code_city" => Field::ClientPostalCodeCity,
            "bill_to.country" => Field::ClientCountry,
            "invoice.number" => Field::InvoiceNumber,
            "invoice.date" => Field::InvoiceDate,
            "invoice.service_date" => Field::ServiceDate,
            "invoice.due_date" => Field::DueDate,
            "notes" => Field::Notes,
            "bank.name" => Field::BankName,
            "bank.beneficiary" => Field::Beneficiary,
            "bank.iban" => Field::Iban,
            _ => return None,
        })
    }

    /// Publishes a fresh semantic snapshot built from the current model. Called on
    /// every render, so published values track edits without extra wiring.
    pub(crate) fn inspect_publish(&self, window: &mut Window, cx: &App) {
        let Some(inspector) = self.inspector.as_ref() else {
            return;
        };
        // Driver-synthesized focus/text/clicks only take effect while the window
        // driver is enabled; enable it lazily on the first publish.
        // Only the patched gpui-pre fork has the driver API (`gpui-driver` feature).
        #[cfg(feature = "gpui-driver")]
        if !window.driver_enabled() {
            window.set_driver_enabled(true);
        }

        let viewport = window.viewport_size();
        // Which focusable semantic id currently owns window focus? Clients that
        // call `focus` wait for this flag before typing (gpui-mcp wait_for_focus).
        let focused_id = self.focused_semantic_id(window, cx);

        let mut nodes = Vec::new();
        // Exactly one `application`-role node with real bounds: the server uses it
        // as the semantic root for resize-settling and screenshot sizing.
        nodes.push(SemanticNode {
            id: "app.root".to_owned(),
            role: "application".to_owned(),
            label: "Invoice editor".to_owned(),
            value: None,
            bounds: Some(SemanticBounds {
                x: 0.0,
                y: 0.0,
                width: f32::from(viewport.width),
                height: f32::from(viewport.height),
            }),
            focused: focused_id.is_none(),
            actions: Vec::new(),
        });

        let field_actions = ["focus", "set_text"];
        for &(field, label) in SENDER_FIELDS
            .iter()
            .chain(BILL_TO_FIELDS.iter())
            .chain(INVOICE_DETAIL_FIELDS.iter())
            .chain(BANK_FIELDS.iter())
        {
            if is_date_field(field) {
                continue;
            }
            let id = Self::field_id(field);
            let mut n = node(
                id,
                "textbox",
                label,
                Some(self.state.field_value(field).to_string()),
                &field_actions,
            );
            n.focused = focused_id.as_deref() == Some(id);
            nodes.push(n);
        }
        // The invoice dates are calendar pickers: no `set_text`, so typing into one
        // is refused by the driver (spec "Dates cannot be typed").
        for &(field, label) in INVOICE_DETAIL_FIELDS.iter() {
            if !is_date_field(field) {
                continue;
            }
            let id = Self::field_id(field);
            let mut n = node(
                id,
                "datepicker",
                label,
                Some(self.state.field_value(field).to_string()),
                &["focus"],
            );
            n.focused = focused_id.as_deref() == Some(id);
            nodes.push(n);
        }
        {
            let id = Self::field_id(Field::Notes);
            let mut n = node(
                id,
                "textbox",
                "Notes",
                Some(self.state.field_value(Field::Notes).to_string()),
                &field_actions,
            );
            n.focused = focused_id.as_deref() == Some(id);
            nodes.push(n);
        }
        {
            let mut n = node(
                "vat_rate",
                "textbox",
                "VAT rate (%)",
                Some(self.state.vat_rate_text()),
                &field_actions,
            );
            n.focused = focused_id.as_deref() == Some("vat_rate");
            nodes.push(n);
        }

        for index in 0..self.line_item_rows.len() {
            for (suffix, label, value) in [
                (
                    "description",
                    "Description",
                    self.state.line_item_text(index, LineItemField::Description),
                ),
                (
                    "quantity",
                    "Quantity",
                    self.state.line_item_text(index, LineItemField::Quantity),
                ),
                (
                    "rate",
                    "Rate (EUR)",
                    self.state.line_item_text(index, LineItemField::Rate),
                ),
            ] {
                let id = format!("line_item.{index}.{suffix}");
                let mut n = node(&id, "textbox", label, value, &field_actions);
                n.focused = focused_id.as_deref() == Some(id.as_str());
                nodes.push(n);
            }
            nodes.push(node(
                format!("line_item.{index}.total"),
                "text",
                "Total",
                self.state.line_item_total(index).map(|money| money.to_string()),
                &[],
            ));
            nodes.push(node(
                format!("remove-line-item-{index}"),
                "button",
                "Remove",
                None,
                &["click"],
            ));
        }
        nodes.push(node("add-line-item", "button", "Add item", None, &["click"]));
        nodes.push(node(
            "export-pdf",
            "button",
            "Export PDF",
            None,
            &["click"],
        ));

        // Live preview field groups (same projection export uses). Values let an
        // MCP client confirm the right-hand preview without parsing pixels.
        // The Sender layout radio group, valued with the chosen option's label.
        let layout_label =
            SENDER_LAYOUT_CHOICES[sender_layout_index(self.state.invoice.sender.layout)].1;
        nodes.push(node(
            "sender.layout",
            "radiogroup",
            "Sender layout",
            Some(layout_label.to_owned()),
            &[],
        ));

        // The rasterized preview (task 5.1): the page count and whether the shown
        // pages are current, rendering, or failed.
        nodes.push(node(
            "preview.pages",
            "text",
            "Preview pages",
            Some(format!(
                "{} pages, {}",
                self.preview_pages.len(),
                self.preview_status().label()
            )),
            &[],
        ));

        // The full-screen preview (task 5.2): its entry button while closed, and its
        // toolbar while open (design D10).
        match self.fullscreen.as_ref() {
            None => nodes.push(node(
                "preview.fullscreen",
                "button",
                "Full-screen preview",
                None,
                &["click"],
            )),
            Some(state) => {
                let fit = fullscreen_fit_width(self.viewport_px);
                let count = self.fullscreen_shown_pages().len();
                for (id, label) in [
                    ("fullscreen.close", "Close"),
                    ("fullscreen.zoom-out", "Zoom out"),
                    ("fullscreen.zoom-in", "Zoom in"),
                    ("fullscreen.fit-width", "Fit width"),
                    ("fullscreen.export-pdf", "Export PDF"),
                ] {
                    nodes.push(node(id, "button", label, None, &["click"]));
                }
                nodes.push(node(
                    "fullscreen.zoom",
                    "text",
                    "Zoom",
                    Some(format!("{:.0}%", zoom_percent(state.zoom, fit))),
                    &[],
                ));
                nodes.push(node(
                    "fullscreen.page",
                    "text",
                    "Page",
                    Some(fullscreen_page_text(state, count)),
                    &[],
                ));
            }
        }

        let p = &self.preview;
        // The return line, then the sender block's address, tax, and email lines.
        let mut sender_lines = vec![p.sender.return_line.clone()];
        sender_lines.extend(p.sender.address_lines.iter().cloned());
        sender_lines.extend(
            p.sender
                .tax_lines
                .iter()
                .map(|(label, value)| format!("{label}: {value}")),
        );
        sender_lines.extend(p.sender.email.clone());
        nodes.push(node(
            "preview.sender",
            "text",
            "Preview sender",
            Some(
                sender_lines
                    .into_iter()
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            &[],
        ));
        nodes.push(node(
            "preview.billed_to",
            "text",
            "Preview billed to",
            Some(p.billed_to.join("\n")),
            &[],
        ));
        nodes.push(node(
            "preview.meta",
            "text",
            "Preview meta",
            Some(
                p.meta
                    .iter()
                    .map(|(l, v)| format!("{l}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            &[],
        ));
        nodes.push(node(
            "preview.line_items",
            "text",
            "Preview line items",
            Some(
                p.line_items
                    .iter()
                    .map(|row| {
                        format!(
                            "{} | {} | {} | {}",
                            row.description, row.quantity, row.rate, row.total
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            &[],
        ));
        nodes.push(node(
            "preview.totals",
            "text",
            "Preview totals",
            Some(
                p.totals
                    .iter()
                    .map(|row| format!("{}: {}", row.label, row.amount))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            &[],
        ));
        let mut notes_parts = Vec::new();
        if let Some(note) = p.standing_note {
            notes_parts.push(note.to_string());
        }
        if let Some(custom) = &p.custom_notes {
            notes_parts.push(custom.clone());
        }
        nodes.push(node(
            "preview.notes",
            "text",
            "Preview notes",
            Some(notes_parts.join("\n")),
            &[],
        ));
        // The Notes panel's read-only §19 line. Published only at 0% VAT, so a
        // client sees the node exactly when the note is shown.
        if let Some(note) = self.notes_standing_note() {
            nodes.push(node(
                "notes.standing_note",
                "text",
                "Standing note",
                Some(note.to_string()),
                &[],
            ));
        }
        nodes.push(node(
            "preview.bank",
            "text",
            "Preview bank",
            Some(
                p.bank
                    .iter()
                    .map(|(l, v)| format!("{l}: {v}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
            &[],
        ));

        inspector.publish(nodes);
    }

    /// Executes one server action against the window. The sole entry point for
    /// server-driven mutation; runs on the foreground executor via [`crate::inspect::drive`].
    ///
    /// Takes the entity, not `&mut self`. Input dispatch (`dispatch_keystroke`,
    /// `dispatch_event`), `resize` and `render_to_image` can draw this view, and a draw
    /// needs the view unleased. So only `Click` and `Focus`, which mutate `self`, take a
    /// `view.update` lease, and only for that mutation.
    pub(crate) fn inspect_execute(
        view: &Entity<Self>,
        action: InspectionAction,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<InspectionActionResult, String> {
        match action {
            InspectionAction::Click { id } => {
                view.update(cx, |view, cx| view.inspect_click(&id, window, cx))
            }
            InspectionAction::Focus { id } => {
                view.update(cx, |view, cx| view.inspect_focus(&id, window, cx))
            }
            InspectionAction::TypeText { text } => {
                #[cfg(feature = "gpui-driver")]
                {
                    if window.driver_insert_text(&text, cx) {
                        Ok(InspectionActionResult::Complete)
                    } else {
                        Err("no focused text field to type into; focus one first".to_owned())
                    }
                }
                #[cfg(not(feature = "gpui-driver"))]
                {
                    let _ = (text, window, cx);
                    Err("typing needs the gpui-driver feature (patched gpui-pre)".to_owned())
                }
            }
            InspectionAction::Key { key, modifiers } => {
                let keystroke = Keystroke {
                    modifiers: to_modifiers(modifiers),
                    key,
                    key_char: None,
                };
                window.dispatch_keystroke(keystroke, cx);
                Ok(InspectionActionResult::Complete)
            }
            InspectionAction::Resize { width, height } => {
                window.resize(size(px(width as f32), px(height as f32)));
                Ok(InspectionActionResult::Complete)
            }
            InspectionAction::Scroll {
                position,
                delta_x,
                delta_y,
                modifiers,
            } => {
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: point(px(position.x), px(position.y)),
                        delta: ScrollDelta::Pixels(point(px(delta_x), px(delta_y))),
                        modifiers: to_modifiers(modifiers),
                        ..Default::default()
                    }),
                    cx,
                );
                Ok(InspectionActionResult::Complete)
            }
            InspectionAction::Drag { from, to } => {
                let from = point(px(from.x), px(from.y));
                let to = point(px(to.x), px(to.y));
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Left,
                        position: from,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                        first_mouse: false,
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: to,
                        pressed_button: Some(MouseButton::Left),
                        modifiers: Modifiers::default(),
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        button: MouseButton::Left,
                        position: to,
                        modifiers: Modifiers::default(),
                        click_count: 1,
                    }),
                    cx,
                );
                Ok(InspectionActionResult::Complete)
            }
            InspectionAction::Screenshot {
                path,
                width,
                height,
            } => Self::inspect_screenshot(window, path, width, height),
        }
    }

    /// Executes a `click` action: line-item / export buttons mutate state; any
    /// other id is a field, and clicking it focuses it as a user would.
    fn inspect_click(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<InspectionActionResult, String> {
        // The full-screen preview controls (task 5.2). The toolbar exists only while the
        // overlay is open, so a click on one while closed is an error, not a no-op.
        if id == "preview.fullscreen" {
            self.enter_fullscreen(window, cx);
            return Ok(InspectionActionResult::Complete);
        }
        if let Some(control) = id.strip_prefix("fullscreen.") {
            if self.fullscreen.is_none() {
                return Err(format!(
                    "full-screen preview is not open; cannot click {id:?}"
                ));
            }
            match control {
                "close" => self.exit_fullscreen(window, cx),
                "zoom-in" => self.step_fullscreen_zoom(true, cx),
                "zoom-out" => self.step_fullscreen_zoom(false, cx),
                "fit-width" => self.fit_fullscreen_width(cx),
                "export-pdf" => self.fullscreen_export(window, cx),
                _ => return Err(format!("{id:?} is not clickable")),
            }
            return Ok(InspectionActionResult::Complete);
        }
        // One Sender layout option by id, `sender.layout.letter|header|footer` (design D10).
        // Choosing it is the same path as the radio group's own change.
        if let Some(option) = id.strip_prefix("sender.layout.") {
            let index = SENDER_LAYOUT_CHOICES
                .iter()
                .position(|(layout, _)| sender_layout_option(*layout) == option)
                .ok_or_else(|| format!("{id:?} is not a sender layout option"))?;
            self.sender_layout_chosen(index, cx);
            return Ok(InspectionActionResult::Complete);
        }
        if id == "add-line-item" {
            self.state.add_line_item();
            self.rebuild_line_item_rows(window, cx);
            self.model_changed(cx);
            return Ok(InspectionActionResult::Complete);
        }
        if id == "export-pdf" {
            self.on_export_pdf(cx);
            return Ok(InspectionActionResult::Complete);
        }
        if let Some(rest) = id.strip_prefix("remove-line-item-") {
            let index: usize = rest
                .parse()
                .map_err(|_| format!("malformed remove-line-item id {id:?}"))?;
            if self.state.remove_line_item(index).is_some() {
                self.rebuild_line_item_rows(window, cx);
                self.model_changed(cx);
                return Ok(InspectionActionResult::Complete);
            }
            return Err(format!("no line item at index {index}"));
        }
        self.inspect_focus(id, window, cx)
    }

    /// Executes a `focus` action by moving window focus to the input for `id`.
    fn inspect_focus(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<InspectionActionResult, String> {
        let handle = self
            .focus_handle_for(id, cx)
            .ok_or_else(|| format!("no focusable input with id {id:?}"))?;
        window.focus(&handle, cx);
        // Re-render so the next inspect_publish marks this id as focused before
        // gpui-mcp's wait_for_focus polls the tree.
        cx.notify();
        Ok(InspectionActionResult::Complete)
    }

    /// The focus handle of the input a semantic id names, if it is a focusable one.
    fn focus_handle_for(&self, id: &str, cx: &App) -> Option<FocusHandle> {
        if let Some(field) = Self::field_from_id(id) {
            if field == Field::Notes {
                return Some(self.notes.focus_handle(cx));
            }
            if is_date_field(field) {
                return Some(self.date_picker_for(field).focus_handle(cx));
            }
            return Some(self.input_for(field).focus_handle(cx));
        }
        if id == "vat_rate" {
            return Some(self.vat_rate_input.focus_handle(cx));
        }
        if let Some(rest) = id.strip_prefix("line_item.") {
            let (index, field) = rest.split_once('.')?;
            let index: usize = index.parse().ok()?;
            let row = self.line_item_rows.get(index)?;
            let entity = match field {
                "description" => &row.description,
                "quantity" => &row.quantity,
                "rate" => &row.rate,
                _ => return None,
            };
            return Some(entity.focus_handle(cx));
        }
        None
    }

    /// The stable id of the focusable input that currently has window focus, if any.
    fn focused_semantic_id(&self, window: &Window, cx: &App) -> Option<String> {
        let candidates: Vec<String> = SENDER_FIELDS
            .iter()
            .chain(BILL_TO_FIELDS.iter())
            .chain(INVOICE_DETAIL_FIELDS.iter())
            .chain(BANK_FIELDS.iter())
            .map(|(field, _)| Self::field_id(*field).to_string())
            .chain(std::iter::once(Self::field_id(Field::Notes).to_string()))
            .chain(std::iter::once("vat_rate".to_string()))
            .chain((0..self.line_item_rows.len()).flat_map(|index| {
                [
                    format!("line_item.{index}.description"),
                    format!("line_item.{index}.quantity"),
                    format!("line_item.{index}.rate"),
                ]
            }))
            .collect();
        candidates.into_iter().find(|id| {
            self.focus_handle_for(id, cx)
                .is_some_and(|handle| handle.is_focused(window))
        })
    }

    /// Executes a `screenshot` action by rasterizing the current frame to `path`.
    /// Echoes the server's logical dimensions, which it reports as the artifact size.
    fn inspect_screenshot(
        window: &mut Window,
        path: String,
        width: u32,
        height: u32,
    ) -> Result<InspectionActionResult, String> {
        let image = window
            .render_to_image()
            .map_err(|error| format!("render_to_image failed: {error:#}"))?;
        image
            .save(&path)
            .map_err(|error| format!("write screenshot {path}: {error}"))?;
        Ok(InspectionActionResult::Screenshot {
            path,
            width,
            height,
        })
    }
}

/// The right-hand preview (task 5.1). It draws the rasterized pages of the last
/// successful render as A4 sheets on the theme's canvas, scaled to the column
/// width. Each page is followed by a "Page n of N" caption.
impl EditorView {
    fn preview_sheets(&self, cx: &gpui::App) -> impl IntoElement {
        let column = self.preview_column_px;
        let count = self.preview_pages.len();
        let notice =
            (self.preview_status() == PreviewStatus::Error).then(|| preview_error_notice(cx));
        let sheets = self
            .preview_pages
            .iter()
            .enumerate()
            .map(|(index, page)| page_sheet(page, column, index, count, cx));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .children(notice)
            .children(sheets)
    }
}

/// The notice shown while the latest preview render has failed (task 5.1). Both the
/// column and the full-screen overlay show this one text.
fn preview_error_notice(cx: &gpui::App) -> impl IntoElement {
    muted_label("Preview could not be updated", cx)
}

/// One page of the preview, drawn `width` px wide with a "Page n of N" caption (tasks 5.1,
/// 5.2). The column and the full-screen overlay both draw their sheets with it.
fn page_sheet(
    page: &Arc<RenderImage>,
    width: f32,
    index: usize,
    count: usize,
    cx: &gpui::App,
) -> AnyElement {
    // The raster's own size gives the page's aspect, so the sheet keeps its proportions
    // at any width.
    let size = page.size(0);
    let aspect = size.height.0 as f32 / size.width.0.max(1) as f32;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .w(px(width))
        .child(
            div()
                .w(px(width))
                .shadow_md()
                .child(img(page.clone()).w(px(width)).h(px(width * aspect))),
        )
        .child(muted_label(format!("Page {} of {count}", index + 1), cx))
        .into_any_element()
}

/// An inline error line (task 9.1), shared by field rows, the Line items panel,
/// and the export summary. Coloured with the theme's `danger` token.
fn error_line(text: impl Into<String>, cx: &gpui::App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(cx.theme().danger)
        .child(text.into())
}

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Dev-only: republish the semantic tree so inspection clients see current
        // values and the live viewport bounds. No-op until an inspector is attached.
        #[cfg(feature = "gpui-mcp")]
        self.inspect_publish(_window, cx);

        // The overlay replaces the two columns while it is open (task 5.2). Cmd+Shift+F
        // is handled here, on the root, so it works whichever field has focus.
        let content = match self.fullscreen.as_ref() {
            Some(state) => self
                .fullscreen_overlay(state, cx)
                .into_any_element(),
            None => self.editor_columns(cx).into_any_element(),
        };
        div()
            .size_full()
            .track_focus(&self.root_focus)
            .on_action(
                cx.listener(|this: &mut Self, _: &ToggleFullscreen, window, cx| {
                    this.toggle_fullscreen(window, cx);
                }),
            )
            .child(content)
    }
}

/// Pixel conversion for the preview raster (task 5.1). Plain unit tests, so they
/// also run without `--features gpui-mcp`.
#[cfg(test)]
mod bgra_tests {
    use super::*;

    #[test]
    fn to_bgra_swaps_red_and_blue_and_keeps_alpha() {
        let mut bytes = vec![10, 20, 30, 255, 1, 2, 3, 0];
        to_bgra(&mut bytes);
        assert_eq!(bytes, vec![30, 20, 10, 255, 3, 2, 1, 0]);
    }

    #[test]
    fn render_image_holds_the_page_size_and_bgra_pixels() {
        let page = PageImage {
            width: 2,
            height: 1,
            rgba: vec![10, 20, 30, 255, 1, 2, 3, 0],
        };
        let image = render_image(&page).expect("image");
        let size = image.size(0);
        assert_eq!((size.width.0, size.height.0), (2, 1));
        assert_eq!(image.as_bytes(0), Some(&[30, 20, 10, 255, 3, 2, 1, 0][..]));
    }
}

/// Live-preview debounce checks (task 8.1). They run on GPUI's deterministic test
/// executor, so the debounce timer is advanced by hand rather than waited on.
/// Compiled with `--features gpui-mcp`, the only feature that enables GPUI's test support.
#[cfg(all(test, feature = "gpui-mcp"))]
mod preview_tests {
    use super::*;
    use billing::identity_store::Identity;
    use billing::model::{VatRate, SECTION_19_NOTE};
    use gpui::{TestAppContext, VisualTestContext};

    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|window, cx| {
            EditorView::new(AppState::new(Identity::default()), window, cx)
        })
    }

    /// Applies an edit the same way an input subscription does: write the model,
    /// then `model_changed`.
    fn edit(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        change: impl FnOnce(&mut AppState),
    ) {
        view.update(cx, |view, cx| {
            change(&mut view.state);
            view.model_changed(cx);
        });
    }

    fn billed_to(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.preview.billed_to.clone())
    }

    #[gpui::test]
    fn preview_waits_for_debounce_then_matches_projection(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Acme GmbH"));

        // Immediately after the edit the snapshot still shows the old state.
        assert!(billed_to(&view, cx).is_empty());

        // Half the debounce window later it is still stale.
        cx.executor().advance_clock(PREVIEW_DEBOUNCE / 2);
        cx.run_until_parked();
        assert!(billed_to(&view, cx).is_empty());

        // Once the edit has settled, the preview re-renders with no manual trigger.
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        assert_eq!(billed_to(&view, cx), vec!["Acme GmbH".to_string()]);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview, project(&view.state.invoice));
        });
    }

    #[gpui::test]
    fn burst_of_edits_refreshes_once_after_last_edit(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "A"));
        cx.executor().advance_clock(PREVIEW_DEBOUNCE / 2);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Acme GmbH"));

        // 300 ms after the first edit but only 150 ms after the second: the
        // timer was restarted, so nothing has been refreshed yet.
        cx.executor().advance_clock(PREVIEW_DEBOUNCE / 2);
        cx.run_until_parked();
        assert!(billed_to(&view, cx).is_empty());

        cx.executor().advance_clock(PREVIEW_DEBOUNCE / 2);
        cx.run_until_parked();
        assert_eq!(billed_to(&view, cx), vec!["Acme GmbH".to_string()]);
    }

    #[gpui::test]
    fn preview_bank_block_shows_grouped_iban(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| {
            s.set_field(Field::Iban, "DE89370400440532013000");
        });
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();

        let bank = view.read_with(cx, |view, _| view.preview.bank.clone());
        assert!(bank.contains(&("IBAN", "DE89 3704 0044 0532 0130 00".to_string())));
    }

    #[gpui::test]
    fn notes_panel_shows_section_19_note_at_zero_percent(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        // The Kleinunternehmer default is 0%, so the Notes panel carries the note.
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.vat_rate, VatRate::ZERO);
            assert_eq!(view.notes_standing_note(), Some(SECTION_19_NOTE));
        });
    }

    #[gpui::test]
    fn notes_panel_hides_section_19_note_above_zero_percent(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| assert!(s.set_vat_rate_text("19")));
        view.read_with(cx, |view, _| {
            assert_eq!(view.notes_standing_note(), None);
        });
    }

    #[gpui::test]
    fn preview_projection_carries_standing_note_for_notes_group(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        // The Notes group of the exported PDF carries this standing note.
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview.standing_note, Some(SECTION_19_NOTE));
            assert_eq!(view.preview.custom_notes, None);
        });
    }

    /// Advances past the preview debounce and lets the background render finish.
    fn settle(cx: &mut VisualTestContext) {
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
    }

    /// A renderer that always fails, for the failed-render path (task 5.1).
    fn failing_render(_: &Projection, _: f32) -> Result<Vec<Arc<RenderImage>>, String> {
        Err("forced failure".to_string())
    }

    fn set_renderer(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        renderer: PreviewRenderFn,
    ) {
        view.update(cx, |view, _| view.preview_renderer = renderer);
    }

    #[gpui::test]
    fn preview_shows_rasterized_pages_once_rendered(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        // The first render is scheduled at startup and has not finished yet.
        view.read_with(cx, |view, _| {
            assert!(view.preview_pages.is_empty());
            assert_eq!(view.preview_status(), PreviewStatus::Rendering);
        });

        settle(cx);
        view.read_with(cx, |view, _| {
            let expected =
                raster::rasterize(&render_projection(&view.preview).expect("render"), 1.0)
                    .expect("rasterize");
            assert_eq!(view.preview_pages.len(), expected.len());
            assert_eq!(view.preview_status(), PreviewStatus::Ok);
            for page in &view.preview_pages {
                let size = page.size(0);
                let aspect = size.height.0 as f64 / size.width.0 as f64;
                assert!((aspect - std::f64::consts::SQRT_2).abs() < 0.01);
            }
        });
    }

    /// The preview is the raster of the same PDF Export PDF writes. The PDF bytes
    /// are not reproducible across calls (printpdf writes a random trailer ID and a
    /// creation timestamp), so the check compares the pixels of a fresh
    /// `render_projection(&view.preview)` with the shown pages.
    #[gpui::test]
    fn preview_pixels_are_the_raster_of_the_export_pdf(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Acme GmbH"));
        settle(cx);
        view.read_with(cx, |view, _| {
            let px_per_pt = view.preview_px_per_pt();
            let export_bytes = render_projection(&view.preview).expect("render");
            let expected = raster::rasterize(&export_bytes, px_per_pt).expect("rasterize");
            assert_eq!(view.preview_pages.len(), expected.len());
            for (shown, page) in view.preview_pages.iter().zip(&expected) {
                let mut bgra = page.rgba.clone();
                to_bgra(&mut bgra);
                assert_eq!(shown.as_bytes(0), Some(bgra.as_slice()));
            }
        });
    }

    #[gpui::test]
    fn failed_render_keeps_last_good_pages_and_shows_notice(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Acme GmbH"));
        settle(cx);
        let good = view.read_with(cx, |view, _| view.preview_pages.clone());
        assert!(!good.is_empty());

        set_renderer(&view, cx, failing_render);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Beta AG"));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview_status(), PreviewStatus::Error);
            assert_eq!(view.preview_pages.len(), good.len());
            assert!(
                view.preview_pages
                    .iter()
                    .zip(&good)
                    .all(|(now, before)| Arc::ptr_eq(now, before)),
                "the last good pages stay on screen"
            );
        });

        // The next successful render clears the notice.
        set_renderer(&view, cx, render_preview);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Gamma SE"));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert!(!view.preview_error);
            assert_eq!(view.preview_status(), PreviewStatus::Ok);
            assert!(!view.preview_pages.is_empty());
        });
    }

    #[gpui::test]
    fn stale_render_result_is_dropped(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| s.set_field(Field::ClientName, "Acme GmbH"));
        settle(cx);
        let (old_generation, old_projection) = view.read_with(cx, |view, _| {
            (view.preview_generation, view.preview.clone())
        });

        edit(&view, cx, |s| s.set_field(Field::ClientName, "Beta AG"));
        settle(cx);
        let current = view.read_with(cx, |view, _| view.preview_pages.clone());

        // A result for the older projection arrives after a newer one was applied.
        let late = render_preview(&old_projection, 1.0).expect("render");
        view.update(cx, |view, cx| {
            view.finish_render(RenderStream::Column, old_generation, Ok(late), cx)
        });
        view.read_with(cx, |view, _| {
            assert!(
                view.preview_pages
                    .iter()
                    .zip(&current)
                    .all(|(now, before)| Arc::ptr_eq(now, before))
                    && view.preview_pages.len() == current.len(),
                "stale result must be dropped"
            );
        });
    }

    #[gpui::test]
    fn window_resize_rerenders_only_past_the_threshold(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        // Viewport 800 px wide: the preview column is 800 - 460 - 48 = 292 px.
        cx.simulate_resize(size(px(800.), px(600.)));
        settle(cx);
        let requested = view.read_with(cx, |view, _| view.preview_requested_column_px);
        assert!((requested - 292.).abs() < 0.01, "column was {requested}");
        let generation = view.read_with(cx, |view, _| view.preview_generation);

        // A 4 px change is within the 8 px threshold: no new render.
        cx.simulate_resize(size(px(804.), px(600.)));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview_generation, generation);
        });

        // A 20 px change re-renders at the new width.
        cx.simulate_resize(size(px(820.), px(600.)));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview_generation, generation + 1);
            assert!((view.preview_requested_column_px - 312.).abs() < 0.01);
        });
    }
}

/// Identity save-on-edit checks (task 7.2). The editor writes to an injected
/// scratch dir, never the per-user config dir. Compiled with `--features gpui-mcp`.
#[cfg(all(test, feature = "gpui-mcp"))]
mod identity_save_tests {
    use super::*;
    use billing::identity_store::{self, Identity};
    use gpui::{TestAppContext, VisualTestContext};
    use std::path::Path;

    const REFERENCE_IBAN: &str = "DE89 3704 0044 0532 0130 00";
    const BAD_CHECKSUM_IBAN: &str = "DE59 3704 0044 0532 0130 00";

    /// A unique scratch dir, removed on drop.
    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            ScratchDir(std::env::temp_dir().join(format!(
                "billing-editor-{label}-{}-{nanos}",
                std::process::id()
            )))
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn editor_saving_to<'a>(
        dir: &Path,
        identity: Identity,
        cx: &'a mut TestAppContext,
    ) -> (Entity<EditorView>, &'a mut VisualTestContext) {
        cx.update(gpui_component::init);
        let dir = dir.to_path_buf();
        cx.add_window_view(move |window, cx| {
            EditorView::new(AppState::new(identity), window, cx).with_identity_dir(Some(dir))
        })
    }

    /// Applies an edit the same way an input subscription does: write the model,
    /// then `model_changed`, which saves.
    fn edit(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        change: impl FnOnce(&mut AppState),
    ) {
        view.update(cx, |view, cx| {
            change(&mut view.state);
            view.model_changed(cx);
        });
    }

    #[gpui::test]
    fn editing_sender_and_bank_writes_the_store_and_reload_restores_them(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("save-reload");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);

        edit(&view, cx, |s| {
            s.set_field(Field::SenderName, "Alexandra Morgan")
        });
        edit(&view, cx, |s| {
            s.set_field(Field::SenderStreet, "Whitfield. 42")
        });
        edit(&view, cx, |s| s.set_field(Field::BankName, "Ellery"));
        edit(&view, cx, |s| s.set_field(Field::Iban, REFERENCE_IBAN));

        let stored = identity_store::load_from(&scratch.0).expect("store is readable");
        assert_eq!(stored.sender.name, "Alexandra Morgan");
        assert_eq!(stored.sender.street, "Whitfield. 42");
        assert_eq!(stored.bank.bank_name, "Ellery");
        assert_eq!(stored.bank.iban, REFERENCE_IBAN);

        // A later launch loads the store and pre-fills the editor from it.
        let (reloaded, cx) = editor_saving_to(&scratch.0, stored, cx);
        reloaded.read_with(cx, |view, _| {
            assert_eq!(
                view.state.field_value(Field::SenderName),
                "Alexandra Morgan"
            );
            assert_eq!(
                view.state.field_value(Field::SenderStreet),
                "Whitfield. 42"
            );
            assert_eq!(view.state.field_value(Field::BankName), "Ellery");
            assert_eq!(view.state.field_value(Field::Iban), REFERENCE_IBAN);
        });
    }

    #[gpui::test]
    fn invalid_iban_edit_keeps_sender_and_last_valid_bank(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("invalid-iban");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);

        edit(&view, cx, |s| {
            s.set_field(Field::SenderName, "Alexandra Morgan")
        });
        edit(&view, cx, |s| s.set_field(Field::BankName, "Ellery"));
        edit(&view, cx, |s| s.set_field(Field::Iban, REFERENCE_IBAN));
        let saved_bank = identity_store::load_from(&scratch.0).expect("load").bank;

        // The IBAN becomes invalid; the sender is edited at the same time.
        edit(&view, cx, |s| s.set_field(Field::Iban, BAD_CHECKSUM_IBAN));
        edit(&view, cx, |s| {
            s.set_field(Field::SenderName, "Someone Else")
        });
        edit(&view, cx, |s| s.set_field(Field::BankName, "Wise"));

        let stored = identity_store::load_from(&scratch.0).expect("load");
        assert_eq!(stored.sender.name, "Someone Else", "the sender is retained");
        assert_eq!(stored.bank, saved_bank, "the last valid bank is unchanged");
        assert_ne!(
            stored.bank.iban, BAD_CHECKSUM_IBAN,
            "an invalid IBAN is never written"
        );
    }

    /// Types `text` into the input for `field` as a user would: the input takes
    /// the new text and emits `Change`, which runs its real subscription.
    fn type_into(view: &Entity<EditorView>, cx: &mut VisualTestContext, field: Field, text: &str) {
        let input = view.read_with(cx, |view, _| view.input_for(field).clone());
        input.update_in(cx, |input, window, cx| {
            input.set_value(text, window, cx);
            cx.emit(InputEvent::Change);
        });
    }

    /// Advances past the preview debounce and lets the refresh run.
    fn settle(cx: &mut VisualTestContext) {
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
    }

    /// The placeholder the Beneficiary input is currently showing.
    fn beneficiary_placeholder_of(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, app| {
            view.input_for(Field::Beneficiary)
                .read(app)
                .presentation()
                .placeholder()
                .to_string()
        })
    }

    #[gpui::test]
    fn typing_tax_vat_and_email_updates_preview_after_debounce_and_saves(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("sender-fields");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);

        type_into(&view, cx, Field::SenderTaxNumber, "21/815/08150");
        type_into(&view, cx, Field::SenderVatId, "DE123456789");
        type_into(&view, cx, Field::SenderEmail, "billing@example.de");

        // Before the debounce settles, the preview still shows the first-run values.
        view.read_with(cx, |view, _| assert_eq!(view.preview.sender.email, None));
        settle(cx);

        view.read_with(cx, |view, _| {
            assert_eq!(
                view.preview.sender.tax_lines,
                vec![
                    ("Tax number", "21/815/08150".to_string()),
                    ("VAT ID", "DE123456789".to_string()),
                ]
            );
            assert_eq!(
                view.preview.sender.email.as_deref(),
                Some("billing@example.de")
            );
        });

        let stored = identity_store::load_from(&scratch.0).expect("store is readable");
        assert_eq!(stored.sender.tax_number, "21/815/08150");
        assert_eq!(stored.sender.vat_id, "DE123456789");
        assert_eq!(stored.sender.email, "billing@example.de");
    }

    #[gpui::test]
    fn choosing_a_sender_layout_updates_preview_saves_and_reload_restores_it(
        cx: &mut TestAppContext,
    ) {
        let scratch = ScratchDir::new("sender-layout");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);
        type_into(&view, cx, Field::SenderTaxNumber, "21/815/08150");
        type_into(&view, cx, Field::SenderName, "Alexandra Sharma");

        // Index 2 is "Footer columns" in the radio group.
        view.update(cx, |view, cx| view.sender_layout_chosen(2, cx));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.preview.sender_layout, SenderLayout::Footer);
        });
        let stored = identity_store::load_from(&scratch.0).expect("store is readable");
        assert_eq!(stored.sender.layout, SenderLayout::Footer);

        // A later launch pre-selects the saved layout and restores the new fields.
        let (reloaded, cx) = editor_saving_to(&scratch.0, stored, cx);
        reloaded.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.sender.layout, SenderLayout::Footer);
            assert_eq!(sender_layout_index(view.state.invoice.sender.layout), 2);
            assert_eq!(
                view.state.field_value(Field::SenderTaxNumber),
                "21/815/08150"
            );
            assert_eq!(
                view.state.field_value(Field::SenderName),
                "Alexandra Sharma"
            );
        });
    }

    /// Dispatches a `click` on a semantic id, the way the gpui-mcp server does.
    fn click_by_id(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        id: &str,
    ) -> Result<InspectionActionResult, String> {
        cx.update(|window, app| view.update(app, |view, cx| view.inspect_click(id, window, cx)))
    }

    #[gpui::test]
    fn clicking_a_sender_layout_option_by_id_chooses_it_and_saves(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("sender-layout-click");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.sender.layout, SenderLayout::Letter);
        });

        assert!(click_by_id(&view, cx, "sender.layout.footer").is_ok());
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.sender.layout, SenderLayout::Footer);
            assert_eq!(view.preview.sender_layout, SenderLayout::Footer);
        });
        let stored = identity_store::load_from(&scratch.0).expect("store is readable");
        assert_eq!(stored.sender.layout, SenderLayout::Footer);

        assert!(click_by_id(&view, cx, "sender.layout.header").is_ok());
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.sender.layout, SenderLayout::Header);
        });

        // An unknown option is an error, and the layout stays where it was.
        assert!(click_by_id(&view, cx, "sender.layout.bogus").is_err());
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.invoice.sender.layout, SenderLayout::Header);
        });
    }

    #[gpui::test]
    fn sender_caption_reads_name_street_and_postal_city(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("caption");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);
        view.read_with(cx, |view, _| assert_eq!(view.sender_caption(), None));

        type_into(&view, cx, Field::SenderName, "Alexandra Sharma");
        type_into(&view, cx, Field::SenderStreet, "Musterstraße 12");
        type_into(&view, cx, Field::SenderPostalCodeCity, "10115 Musterstadt");
        type_into(&view, cx, Field::SenderCountry, "Germany");

        view.read_with(cx, |view, _| {
            assert_eq!(
                view.sender_caption().as_deref(),
                Some("Alexandra Sharma · Musterstraße 12 · 10115 Musterstadt")
            );
        });
    }

    #[gpui::test]
    fn beneficiary_placeholder_follows_the_sender_name(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("placeholder");
        let (view, cx) = editor_saving_to(&scratch.0, Identity::default(), cx);
        assert_eq!(beneficiary_placeholder_of(&view, cx), "");

        type_into(&view, cx, Field::SenderName, "Alexandra Sharma");
        assert_eq!(beneficiary_placeholder_of(&view, cx), "Alexandra Sharma");

        type_into(&view, cx, Field::SenderName, "Someone Else");
        assert_eq!(beneficiary_placeholder_of(&view, cx), "Someone Else");
        // The stored Beneficiary stays empty, so the fallback keeps following the Name.
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.field_value(Field::Beneficiary), "");
        });

        // On the next launch the placeholder comes from the saved Name.
        let stored = identity_store::load_from(&scratch.0).expect("store is readable");
        let (reloaded, cx) = editor_saving_to(&scratch.0, stored, cx);
        assert_eq!(beneficiary_placeholder_of(&reloaded, cx), "Someone Else");
    }
}

/// Keyboard date steps (task 4.3, design D2): the arithmetic that the keys apply.
#[cfg(test)]
mod date_step_tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    /// Asserts that `step` applied to `from` lands on `to`.
    fn assert_step(step: DateStep, from: NaiveDate, to: NaiveDate) {
        assert_eq!(step.apply(from), Some(to), "{step:?} from {from}");
    }

    #[test]
    fn day_steps_cross_month_and_year_boundaries() {
        assert_step(DateStep::Days(1), day(2026, 10, 3), day(2026, 10, 4));
        assert_step(DateStep::Days(-1), day(2026, 10, 3), day(2026, 10, 2));
        assert_step(DateStep::Days(7), day(2026, 10, 4), day(2026, 10, 11));
        assert_step(DateStep::Days(-7), day(2026, 1, 3), day(2025, 12, 27));
    }

    #[test]
    fn month_step_clamps_to_the_last_day_of_a_shorter_month() {
        assert_step(DateStep::Months(1), day(2026, 1, 31), day(2026, 2, 28));
        assert_step(DateStep::Months(1), day(2028, 1, 31), day(2028, 2, 29));
        assert_step(DateStep::Months(-1), day(2026, 3, 31), day(2026, 2, 28));
    }

    #[test]
    fn month_step_keeps_the_day_when_it_exists() {
        assert_step(DateStep::Months(1), day(2026, 10, 11), day(2026, 11, 11));
        assert_step(DateStep::Months(-1), day(2026, 10, 11), day(2026, 9, 11));
    }

    #[test]
    fn year_step_moves_twelve_months_and_clamps_the_leap_day() {
        assert_step(DateStep::Months(12), day(2026, 10, 3), day(2027, 10, 3));
        assert_step(DateStep::Months(12), day(2028, 2, 29), day(2029, 2, 28));
        assert_step(DateStep::Months(-12), day(2028, 2, 29), day(2027, 2, 28));
    }
}

/// Calendar date pickers for the invoice dates (task 4.2, design D2). Compiled
/// with `--features gpui-mcp`.
#[cfg(all(test, feature = "gpui-mcp"))]
mod date_picker_tests {
    use super::*;
    use billing::identity_store::Identity;
    use chrono::NaiveTime;
    use gpui::{TestAppContext, VisualTestContext};
    use gpui_component::calendar::Date as PickerDate;
    use gpui_component::date_picker::DateTime;

    fn editor_with(
        state: AppState,
        cx: &mut TestAppContext,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.update(bind_date_field_keys);
        cx.add_window_view(move |window, cx| EditorView::new(state, window, cx))
    }

    /// Gives keyboard focus to the `field` picker, as Tab would, so key presses
    /// reach its `InvoiceDateField` wrapper.
    fn focus_picker(view: &Entity<EditorView>, cx: &mut VisualTestContext, field: Field) {
        let picker = view.read_with(cx, |view, _| view.date_picker_for(field).clone());
        cx.run_until_parked();
        picker.update_in(cx, |picker, window, cx| {
            let handle = picker.focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.run_until_parked();
    }

    /// Advances past the preview debounce and lets the refresh run.
    fn settle(cx: &mut VisualTestContext) {
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
    }

    /// Makes a pick on the `field` picker as a click on a day (`Some`) or on the
    /// clear control (`None`) does: the picker takes the date and emits `Change`,
    /// which runs the editor's real subscription.
    fn pick(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        field: Field,
        date: Option<NaiveDate>,
    ) {
        let picker = view.read_with(cx, |view, _| view.date_picker_for(field).clone());
        picker.update_in(cx, |picker, window, cx| {
            let value = match date {
                Some(day) => {
                    picker.set_date(day, window, cx);
                    DateTime::Single(Some(day.and_time(NaiveTime::MIN)))
                }
                None => {
                    picker.set_date(PickerDate::Single(None), window, cx);
                    DateTime::Single(None)
                }
            };
            cx.emit(DatePickerEvent::Change(value));
        });
    }

    #[gpui::test]
    fn picking_a_day_writes_dd_mm_yyyy_to_the_model_and_preview(cx: &mut TestAppContext) {
        let (view, cx) = editor_with(AppState::new(Identity::default()), cx);

        let day = NaiveDate::from_ymd_opt(2026, 10, 3);
        pick(&view, cx, Field::InvoiceDate, day);
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.field_value(Field::InvoiceDate), "03/10/2026");
        });

        settle(cx);
        view.read_with(cx, |view, _| {
            let shown = view
                .preview
                .meta
                .iter()
                .find(|(label, _)| *label == "Invoice date")
                .map(|(_, value)| value.to_string());
            assert_eq!(shown.as_deref(), Some("03/10/2026"));
        });
    }

    #[gpui::test]
    fn clearing_a_date_empties_the_model_and_export_asks_for_it(cx: &mut TestAppContext) {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::DueDate, "15/11/2026");
        let (view, cx) = editor_with(state, cx);

        pick(&view, cx, Field::DueDate, None);
        view.read_with(cx, |view, _| {
            assert_eq!(view.state.field_value(Field::DueDate), "");
        });
        settle(cx);

        // Before an export attempt the empty date is not yet an error.
        view.read_with(cx, |view, _| assert_eq!(view.field_error(Field::DueDate), None));
        view.update(cx, |view, cx| {
            view.export_attempted = true;
            cx.notify();
        });
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.field_error(Field::DueDate).as_deref(),
                Some("Enter a date in DD/MM/YYYY format.")
            );
        });
    }

    #[gpui::test]
    fn picker_starts_on_the_model_date_and_leaves_unparseable_text_unselected(
        cx: &mut TestAppContext,
    ) {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::InvoiceDate, "03/10/2026");
        state.set_field(Field::ServiceDate, "2026-10-03");
        let (view, cx) = editor_with(state, cx);

        view.read_with(cx, |view, app| {
            assert_eq!(
                view.date_picker_for(Field::InvoiceDate).read(app).date(),
                PickerDate::Single(NaiveDate::from_ymd_opt(2026, 10, 3))
            );
            assert_eq!(
                view.date_picker_for(Field::ServiceDate).read(app).date(),
                PickerDate::Single(None)
            );
        });
    }

    fn model_date(view: &Entity<EditorView>, cx: &mut VisualTestContext, field: Field) -> String {
        view.read_with(cx, |view, _| view.state.field_value(field).to_string())
    }

    #[gpui::test]
    fn keyboard_steps_move_the_due_date_and_reach_the_model_and_preview(cx: &mut TestAppContext) {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::DueDate, "03/10/2026");
        let (view, cx) = editor_with(state, cx);
        focus_picker(&view, cx, Field::DueDate);

        cx.simulate_keystrokes("right");
        assert_eq!(model_date(&view, cx, Field::DueDate), "04/10/2026");
        cx.simulate_keystrokes("down");
        assert_eq!(model_date(&view, cx, Field::DueDate), "11/10/2026");
        cx.simulate_keystrokes("pagedown");
        assert_eq!(model_date(&view, cx, Field::DueDate), "11/11/2026");

        settle(cx);
        view.read_with(cx, |view, _| {
            let shown = view
                .preview
                .meta
                .iter()
                .find(|(label, _)| *label == "Due date")
                .map(|(_, value)| value.to_string());
            assert_eq!(shown.as_deref(), Some("11/11/2026"));
        });
    }

    #[gpui::test]
    fn shift_page_keys_move_by_a_year_and_escape_leaves_the_value(cx: &mut TestAppContext) {
        let mut state = AppState::new(Identity::default());
        state.set_field(Field::InvoiceDate, "03/10/2026");
        let (view, cx) = editor_with(state, cx);
        focus_picker(&view, cx, Field::InvoiceDate);

        cx.simulate_keystrokes("escape");
        assert_eq!(model_date(&view, cx, Field::InvoiceDate), "03/10/2026");

        cx.simulate_keystrokes("shift-pagedown");
        assert_eq!(model_date(&view, cx, Field::InvoiceDate), "03/10/2027");
        cx.simulate_keystrokes("shift-pageup");
        assert_eq!(model_date(&view, cx, Field::InvoiceDate), "03/10/2026");
    }

    #[gpui::test]
    fn a_keyboard_step_on_an_empty_date_field_takes_today(cx: &mut TestAppContext) {
        let (view, cx) = editor_with(AppState::new(Identity::default()), cx);
        focus_picker(&view, cx, Field::ServiceDate);

        cx.simulate_keystrokes("right");
        let today = Local::now().date_naive().format(DATE_FORMAT).to_string();
        assert_eq!(model_date(&view, cx, Field::ServiceDate), today);
    }
}

/// Export validation surfacing (task 9.1): which inline errors and summary the
/// editor shows once Export PDF has been clicked. Compiled with `--features gpui-mcp`.
#[cfg(all(test, feature = "gpui-mcp"))]
mod export_gate_view_tests {
    use super::*;
    use billing::identity_store::Identity;
    use gpui::{TestAppContext, VisualTestContext};

    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|window, cx| {
            EditorView::new(AppState::new(Identity::default()), window, cx)
        })
    }

    fn edit(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        change: impl FnOnce(&mut AppState),
    ) {
        view.update(cx, |view, cx| {
            change(&mut view.state);
            view.model_changed(cx);
        });
    }

    /// Marks an export attempt without opening the save dialog (gate-only checks).
    fn click_export(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            view.export_attempted = true;
            cx.notify();
        });
    }

    fn field_error(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        field: Field,
    ) -> Option<String> {
        view.read_with(cx, |view, _| view.field_error(field))
    }

    fn summary(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| export_gate::summary(&view.export_issues()))
    }

    fn fill_valid_invoice(state: &mut AppState) {
        state.set_field(Field::SenderTaxNumber, "21/815/08150");
        state.set_field(Field::InvoiceNumber, "4");
        state.set_field(Field::InvoiceDate, "01/11/2026");
        state.set_field(Field::ServiceDate, "31/10/2026");
        state.set_field(Field::DueDate, "15/11/2026");
        state.set_field(Field::Iban, "DE89 3704 0044 0532 0130 00");
        state.add_line_item();
        state.set_line_item_text(0, LineItemField::Quantity, "1");
        state.set_line_item_text(0, LineItemField::Rate, "200");
    }

    #[gpui::test]
    fn no_export_errors_show_before_export_is_clicked(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        assert_eq!(field_error(&view, cx, Field::InvoiceNumber), None);
        assert_eq!(summary(&view, cx), None);
    }

    #[gpui::test]
    fn export_click_on_empty_invoice_surfaces_every_error(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        click_export(&view, cx);

        assert_eq!(
            field_error(&view, cx, Field::InvoiceNumber).as_deref(),
            Some("Enter an invoice number.")
        );
        assert_eq!(
            field_error(&view, cx, Field::InvoiceDate).as_deref(),
            Some("Enter a date in DD/MM/YYYY format.")
        );
        assert_eq!(
            field_error(&view, cx, Field::ServiceDate).as_deref(),
            Some("Enter a date in DD/MM/YYYY format.")
        );
        assert_eq!(
            field_error(&view, cx, Field::DueDate).as_deref(),
            Some("Enter a date in DD/MM/YYYY format.")
        );
        // An empty IBAN gets its own error once export is attempted.
        assert_eq!(
            field_error(&view, cx, Field::Iban).as_deref(),
            Some("Enter an IBAN.")
        );
        view.read_with(cx, |view, _| {
            assert!(view.export_issues().contains(&ExportIssue::NoLineItems));
        });
        assert_eq!(
            summary(&view, cx).as_deref(),
            Some(
                "Export blocked. Fix: Tax number, Line items, Invoice number, Invoice date, Service date, Due date, IBAN."
            )
        );
    }

    #[gpui::test]
    fn export_with_empty_tax_number_shows_inline_error_and_summary_item(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| {
            fill_valid_invoice(s);
            s.set_field(Field::SenderTaxNumber, "");
        });
        click_export(&view, cx);

        assert_eq!(
            field_error(&view, cx, Field::SenderTaxNumber).as_deref(),
            Some("Enter your tax number (Steuernummer).")
        );
        assert_eq!(
            summary(&view, cx).as_deref(),
            Some("Export blocked. Fix: Tax number.")
        );

        // Filling the Tax number clears both the inline error and the summary item.
        edit(&view, cx, |s| {
            s.set_field(Field::SenderTaxNumber, "21/815/08150")
        });
        assert_eq!(field_error(&view, cx, Field::SenderTaxNumber), None);
        assert_eq!(summary(&view, cx), None);
    }

    #[gpui::test]
    fn malformed_date_and_bad_iban_surface_their_own_messages(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, |s| {
            s.set_field(Field::InvoiceDate, "2026-11-01");
            s.set_field(Field::Iban, "DE59 3704 0044 0532 0130 00");
        });
        click_export(&view, cx);
        assert_eq!(
            field_error(&view, cx, Field::InvoiceDate).as_deref(),
            Some("Date must be in DD/MM/YYYY format")
        );
        assert_eq!(
            field_error(&view, cx, Field::Iban).as_deref(),
            Some("IBAN checksum is invalid")
        );
    }

    #[gpui::test]
    fn inline_error_clears_once_its_field_is_fixed(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        click_export(&view, cx);
        assert!(field_error(&view, cx, Field::InvoiceNumber).is_some());

        edit(&view, cx, |s| s.set_field(Field::InvoiceNumber, "4"));
        assert_eq!(field_error(&view, cx, Field::InvoiceNumber), None);
    }

    #[gpui::test]
    fn fully_valid_invoice_after_click_shows_no_errors(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        edit(&view, cx, fill_valid_invoice);
        click_export(&view, cx);
        assert_eq!(summary(&view, cx), None);
        view.read_with(cx, |view, _| assert!(view.export_issues().is_empty()));
        assert_eq!(field_error(&view, cx, Field::Iban), None);
    }
}

/// Export PDF save-dialog flow (task 9.2). Compiled with `--features gpui-mcp`.
#[cfg(all(test, feature = "gpui-mcp"))]
mod export_pdf_tests {
    use super::*;
    use billing::identity_store::Identity;
    use gpui::{TestAppContext, VisualTestContext};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "billing-export-ui-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("scratch dir");
            ScratchDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn editor_with_prefs(
        cx: &mut TestAppContext,
        prefs: PathBuf,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(|window, cx| {
            EditorView::new(AppState::new(Identity::default()), window, cx)
                .with_identity_dir(Some(prefs))
        })
    }

    fn edit(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        change: impl FnOnce(&mut AppState),
    ) {
        view.update(cx, |view, cx| {
            change(&mut view.state);
            view.model_changed(cx);
        });
    }

    fn fill_valid_invoice(state: &mut AppState) {
        state.set_field(Field::SenderTaxNumber, "21/815/08150");
        state.set_field(Field::ClientName, "Ostrander Quennell");
        state.set_field(Field::InvoiceNumber, "4");
        state.set_field(Field::InvoiceDate, "01/11/2026");
        state.set_field(Field::ServiceDate, "31/10/2026");
        state.set_field(Field::DueDate, "15/11/2026");
        state.set_field(Field::Iban, "DE89 3704 0044 0532 0130 00");
        state.add_line_item();
        state.set_line_item_text(0, LineItemField::Quantity, "1");
        state.set_line_item_text(0, LineItemField::Rate, "200");
    }

    /// Text of a PDF via `pdftotext` (Poppler), with whitespace collapsed.
    fn pdf_text(path: &Path) -> String {
        let output = Command::new("pdftotext")
            .arg(path)
            .arg("-")
            .output()
            .expect("pdftotext must be installed to verify exported PDF text");
        assert!(
            output.status.success(),
            "pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Asserts every field group of `projection` appears in the PDF at `path`.
    /// Labels and values are checked separately: the metadata grid and the bank
    /// footer print each label above its value, so `pdftotext` may split them.
    fn assert_pdf_shows_projection(path: &Path, projection: &Projection) {
        let text = pdf_text(path);
        let mut needles: Vec<String> = projection.billed_to.clone();
        for (label, value) in projection.meta.iter().chain(projection.bank.iter()) {
            needles.push(label.to_string());
            needles.push(value.clone());
        }
        for row in &projection.line_items {
            needles.push(row.description.clone());
            needles.push(row.quantity.clone());
            needles.push(row.rate.clone());
            needles.push(row.total.clone());
        }
        for total in &projection.totals {
            needles.push(total.label.clone());
            needles.push(total.amount.clone());
        }
        for needle in needles {
            let needle = needle.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(
                text.contains(&needle),
                "exported PDF is missing {needle:?}; text was:\n{text}"
            );
        }
    }

    #[gpui::test]
    fn invalid_export_does_not_open_save_dialog(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("blocked");
        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(
            !cx.cx.did_prompt_for_new_path(),
            "save dialog must not open when the gate fails"
        );
    }

    #[gpui::test]
    fn valid_export_writes_pdf_matching_preview_at_chosen_path(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("write");
        let out_dir = scratch.path().join("chosen");
        fs::create_dir_all(&out_dir).expect("mkdir");
        let dest = out_dir.join("Ostrander Quennell invoice 4.pdf");

        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        edit(&view, cx, fill_valid_invoice);
        // Let the debounced preview catch up so it matches the model projection.
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();

        // Seed last-used folder so the dialog opens there.
        export::save_last_export_dir(scratch.path(), &out_dir).expect("seed last folder");

        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(
            cx.cx.did_prompt_for_new_path(),
            "valid invoice must open the native save dialog"
        );

        cx.cx.simulate_new_path_selection({
            let dest = dest.clone();
            let out_dir = out_dir.clone();
            move |directory| {
                assert_eq!(directory, out_dir.as_path(), "dialog defaults to last-used folder");
                Some(dest)
            }
        });
        cx.run_until_parked();

        assert!(dest.is_file(), "PDF must exist at the chosen path");
        let written = fs::read(&dest).expect("read exported PDF");
        assert!(written.starts_with(b"%PDF"));
        let preview = view.read_with(cx, |view, _| {
            // Live preview and a fresh model projection agree; export wrote
            // from that same `project` path (see `export::write_invoice_pdf`).
            assert_eq!(view.preview, project(&view.state.invoice));
            assert_eq!(view.preview.billed_to[0], "Ostrander Quennell");
            assert_eq!(
                view.preview
                    .meta
                    .iter()
                    .find(|(l, _)| *l == "Invoice number")
                    .map(|(_, v)| v.as_str()),
                Some("4")
            );
            view.preview.clone()
        });
        assert_pdf_shows_projection(&dest, &preview);
        assert_eq!(
            export::load_last_export_dir(scratch.path()).expect("load prefs"),
            Some(out_dir),
            "successful export remembers the chosen folder"
        );
    }

    #[gpui::test]
    fn failed_write_surfaces_error_in_export_bar(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("unwritable");
        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        edit(&view, cx, fill_valid_invoice);
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();

        // An existing directory is not a writable file path, so the write fails.
        let dir_as_file = scratch.path().join("is-a-directory");
        fs::create_dir_all(&dir_as_file).expect("mkdir");

        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(cx.cx.did_prompt_for_new_path());
        cx.cx.simulate_new_path_selection({
            let dir_as_file = dir_as_file.clone();
            move |_directory| Some(dir_as_file)
        });
        cx.run_until_parked();

        view.read_with(cx, |view, _| {
            let msg = view
                .export_error
                .as_deref()
                .expect("a failed write must be stored so the export bar shows it");
            assert!(
                msg.starts_with("Export failed: could not write PDF"),
                "unexpected export error: {msg}"
            );
        });
        assert!(
            dir_as_file.is_dir(),
            "failed write must not touch the directory"
        );
    }

    #[gpui::test]
    fn next_export_attempt_clears_a_previous_write_error(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("clear-error");
        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        edit(&view, cx, fill_valid_invoice);
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();

        let dir_as_file = scratch.path().join("is-a-directory");
        fs::create_dir_all(&dir_as_file).expect("mkdir");
        view.update(cx, |view, cx| view.on_export_pdf(cx));
        cx.cx
            .simulate_new_path_selection(move |_directory| Some(dir_as_file));
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.export_error.is_some()));

        // The next click starts a new attempt, so the stale error goes away
        // before the dialog opens; cancelling the dialog then leaves it clear.
        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(cx.cx.did_prompt_for_new_path());
        view.read_with(cx, |view, _| {
            assert!(
                view.export_error.is_none(),
                "next attempt must clear the error"
            );
        });
        cx.cx.simulate_new_path_selection(|_directory| None);
        cx.run_until_parked();
        view.read_with(cx, |view, _| assert!(view.export_error.is_none()));
    }

    #[gpui::test]
    fn write_uses_invoice_as_validated_at_click_not_at_confirm(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("snapshot");
        let out_dir = scratch.path().join("chosen");
        fs::create_dir_all(&out_dir).expect("mkdir");
        let dest = out_dir.join("Ostrander Quennell invoice 4.pdf");

        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        edit(&view, cx, fill_valid_invoice);
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        let validated = view.read_with(cx, |view, _| view.state.invoice.clone());

        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(cx.cx.did_prompt_for_new_path());

        // The form stays editable while the save dialog is open: change the
        // invoice number and break the IBAN so the live state fails the gate.
        edit(&view, cx, |state| {
            state.set_field(Field::InvoiceNumber, "5");
            state.set_field(Field::Iban, "");
        });
        view.read_with(cx, |view, _| {
            assert!(
                !export_gate::check(&view.state.invoice).is_empty(),
                "live invoice must now fail the gate"
            );
        });

        cx.cx.simulate_new_path_selection({
            let dest = dest.clone();
            move |_directory| Some(dest)
        });
        cx.run_until_parked();

        // The file must carry the click-time invoice, not the edited one.
        assert_pdf_shows_projection(&dest, &project(&validated));
        let text = pdf_text(&dest);
        // The meta grid prints each label above its value, so check the label and
        // the value tokens separately: the click-time number 4 is present and the
        // edited number 5 is not printed as a standalone value.
        let tokens: Vec<&str> = text.split_whitespace().collect();
        assert!(
            text.contains("Invoice number") && tokens.contains(&"4") && !tokens.contains(&"5"),
            "export must write the invoice validated at click time; text was:\n{text}"
        );
    }

    #[gpui::test]
    fn cancelling_save_dialog_writes_no_file(cx: &mut TestAppContext) {
        let scratch = ScratchDir::new("cancel");
        let (view, cx) = editor_with_prefs(cx, scratch.path().to_path_buf());
        edit(&view, cx, fill_valid_invoice);

        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(cx.cx.did_prompt_for_new_path());
        cx.cx.simulate_new_path_selection(|_dir| None);
        cx.run_until_parked();

        let leftover: Vec<_> = fs::read_dir(scratch.path())
            .expect("read scratch")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|ext| ext == "pdf")
            })
            .collect();
        assert!(leftover.is_empty(), "cancel must not write a PDF");
    }
}

/// End-to-end reference invoice flow (task 10.1): enter the reference values,
/// confirm the live preview, export, reopen the PDF for every field group, then
/// relaunch and confirm Sender/Bank identity persisted.
#[cfg(all(test, feature = "gpui-mcp"))]
mod e2e_reference_flow_tests {
    use super::*;
    use billing::identity_store::{self, Identity};
    use billing::model::SECTION_19_NOTE;
    use gpui::{TestAppContext, VisualTestContext};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    const REFERENCE_IBAN_GROUPED: &str = "DE89 3704 0044 0532 0130 00";
    const REFERENCE_RETURN_LINE: &str = "Alexandra Morgan · Whitfield. 42 · 10115 Musterstadt";
    const REFERENCE_TAX_NUMBER: &str = "21/815/08150";
    const REFERENCE_LINE_DESCRIPTION: &str =
        "Live musician performance - Tallis Vestry, Gorham Bank";

    struct ScratchDir(PathBuf);

    impl ScratchDir {
        fn new(label: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let path = std::env::temp_dir().join(format!(
                "billing-e2e-{label}-{}-{nanos}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("scratch dir");
            ScratchDir(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn editor_with_prefs(
        cx: &mut TestAppContext,
        prefs: PathBuf,
        identity: Identity,
    ) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.add_window_view(move |window, cx| {
            EditorView::new(AppState::new(identity), window, cx).with_identity_dir(Some(prefs))
        })
    }

    fn edit(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        change: impl FnOnce(&mut AppState),
    ) {
        view.update(cx, |view, cx| {
            change(&mut view.state);
            view.model_changed(cx);
        });
    }

    /// Canonical reference invoice from the source PDF / `pdf::tests::reference_invoice`.
    fn fill_reference_invoice(state: &mut AppState) {
        state.set_field(Field::SenderName, "Alexandra Morgan");
        state.set_field(Field::SenderStreet, "Whitfield. 42");
        state.set_field(Field::SenderPostalCodeCity, "10115 Musterstadt");
        state.set_field(Field::SenderCountry, "Germany");
        state.set_field(Field::SenderTaxNumber, REFERENCE_TAX_NUMBER);

        state.set_field(Field::ClientName, "Ostrander Quennell");
        state.set_field(Field::ClientStreet, "Rayburn Str. 42");
        state.set_field(Field::ClientPostalCodeCity, "80331 Musterstadt");
        state.set_field(Field::ClientCountry, "Germany");

        state.set_field(Field::InvoiceNumber, "4");
        state.set_field(Field::InvoiceDate, "03/10/2026");
        state.set_field(Field::ServiceDate, "11/09/2026");
        state.set_field(Field::DueDate, "15/10/2026");

        state.add_line_item();
        state.set_line_item_text(0, LineItemField::Description, REFERENCE_LINE_DESCRIPTION);
        state.set_line_item_text(0, LineItemField::Quantity, "1");
        state.set_line_item_text(0, LineItemField::Rate, "200");

        // VAT stays at the 0% default so the §19 note is present.
        state.set_field(Field::BankName, "Ellery");
        state.set_field(Field::Beneficiary, "Penhallow Morgan");
        state.set_field(Field::Iban, REFERENCE_IBAN_GROUPED);
    }

    /// Extracts text from a PDF via `pdftotext` (Poppler). Used only in tests.
    fn pdf_text(path: &Path) -> String {
        let output = Command::new("pdftotext")
            .args(["-layout", &path.to_string_lossy(), "-"])
            .output()
            .expect("pdftotext must be installed to verify exported PDF text");
        assert!(
            output.status.success(),
            "pdftotext failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    #[gpui::test]
    fn reference_invoice_end_to_end_export_and_identity_persist(cx: &mut TestAppContext) {
        // --- 1. Launch (empty identity, scratch config dir) ---
        let scratch = ScratchDir::new("flow");
        let out_dir = scratch.path().join("exports");
        fs::create_dir_all(&out_dir).expect("mkdir exports");
        let dest = out_dir.join("Ostrander Quennell invoice 4.pdf");

        let (view, cx) =
            editor_with_prefs(cx, scratch.path().to_path_buf(), Identity::default());

        // --- 2. Enter the reference invoice values ---
        edit(&view, cx, fill_reference_invoice);

        // --- 3. Live preview reflects them after debounce ---
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            let p = &view.preview;
            assert_eq!(p, &project(&view.state.invoice));
            assert_eq!(p.sender.return_line, REFERENCE_RETURN_LINE);
            assert_eq!(
                p.billed_to,
                vec![
                    "Ostrander Quennell".to_string(),
                    "Rayburn Str. 42".to_string(),
                    "80331 Musterstadt".to_string(),
                    "Germany".to_string(),
                ]
            );
            assert_eq!(
                p.meta
                    .iter()
                    .find(|(l, _)| *l == "Invoice number")
                    .map(|(_, v)| v.as_str()),
                Some("4")
            );
            assert_eq!(p.line_items.len(), 1);
            assert_eq!(p.line_items[0].description, REFERENCE_LINE_DESCRIPTION);
            assert_eq!(p.line_items[0].total, "200.00 €");
            assert_eq!(p.standing_note, Some(SECTION_19_NOTE));
            assert!(
                p.bank
                    .contains(&("IBAN", REFERENCE_IBAN_GROUPED.to_string()))
            );
            assert!(p.bank.contains(&("Bank", "Ellery".to_string())));
            assert!(
                p.bank
                    .contains(&("Beneficiary", "Penhallow Morgan".to_string()))
            );
        });

        // Identity must already be on disk (save-on-edit) before export / relaunch.
        let stored_mid = identity_store::load_from(scratch.path()).expect("mid-flow load");
        assert_eq!(stored_mid.sender.name, "Alexandra Morgan");
        assert_eq!(stored_mid.bank.iban, REFERENCE_IBAN_GROUPED);

        // --- 4. Export to a file ---
        export::save_last_export_dir(scratch.path(), &out_dir).expect("seed last folder");
        view.update(cx, |view, cx| view.on_export_pdf(cx));
        assert!(
            cx.cx.did_prompt_for_new_path(),
            "valid reference invoice must open the save dialog"
        );
        cx.cx.simulate_new_path_selection({
            let dest = dest.clone();
            let out_dir = out_dir.clone();
            move |directory| {
                assert_eq!(directory, out_dir.as_path());
                Some(dest)
            }
        });
        cx.run_until_parked();
        assert!(dest.is_file(), "PDF must exist at {}", dest.display());

        // --- 5. Reopen that PDF; every field group is present ---
        let text = pdf_text(&dest);
        assert!(
            text.contains(REFERENCE_RETURN_LINE),
            "return line missing:\n{text}"
        );
        assert!(
            text.contains("Ostrander Quennell"),
            "billed-to missing:\n{text}"
        );
        assert!(
            text.contains("Rayburn Str. 42"),
            "billed-to street missing:\n{text}"
        );
        assert!(
            text.contains("Invoice number")
                && text.contains("Invoice date")
                && text.contains("Service date")
                && text.contains("Due date"),
            "invoice meta labels missing:\n{text}"
        );
        assert!(
            text.contains("03/10/2026")
                && text.contains("11/09/2026")
                && text.contains("15/10/2026"),
            "dates missing:\n{text}"
        );
        assert!(
            text.contains("Gorham Bank") && text.contains("200.00"),
            "line items missing:\n{text}"
        );
        assert!(
            text.contains("Total amount (Net)")
                && text.contains("VAT (0%)")
                && text.contains("Total amount (Gross)"),
            "totals missing:\n{text}"
        );
        assert!(
            text.contains(SECTION_19_NOTE),
            "§19 notes group missing:\n{text}"
        );
        assert!(
            text.contains("Bank")
                && text.contains("Ellery")
                && text.contains("Beneficiary")
                && text.contains("Penhallow Morgan")
                && text.contains(REFERENCE_IBAN_GROUPED),
            "bank details missing:\n{text}"
        );

        // --- 6. Relaunch and confirm Sender/Bank identity persisted ---
        let reloaded_identity =
            identity_store::load_from(scratch.path()).expect("relaunch load");
        assert_eq!(reloaded_identity.sender.name, "Alexandra Morgan");
        assert_eq!(reloaded_identity.sender.street, "Whitfield. 42");
        assert_eq!(
            reloaded_identity.sender.postal_code_city,
            "10115 Musterstadt"
        );
        assert_eq!(reloaded_identity.sender.country, "Germany");
        assert_eq!(reloaded_identity.bank.bank_name, "Ellery");
        assert_eq!(reloaded_identity.bank.beneficiary, "Penhallow Morgan");
        assert_eq!(reloaded_identity.bank.iban, REFERENCE_IBAN_GROUPED);

        let (relaunch, cx) =
            editor_with_prefs(cx, scratch.path().to_path_buf(), reloaded_identity);
        relaunch.read_with(cx, |view, _| {
            assert_eq!(
                view.state.field_value(Field::SenderName),
                "Alexandra Morgan"
            );
            assert_eq!(view.state.field_value(Field::SenderStreet), "Whitfield. 42");
            assert_eq!(
                view.state.field_value(Field::SenderPostalCodeCity),
                "10115 Musterstadt"
            );
            assert_eq!(view.state.field_value(Field::SenderCountry), "Germany");
            assert_eq!(view.state.field_value(Field::BankName), "Ellery");
            assert_eq!(
                view.state.field_value(Field::Beneficiary),
                "Penhallow Morgan"
            );
            assert_eq!(view.state.field_value(Field::Iban), REFERENCE_IBAN_GROUPED);
            // Per-invoice content is not persisted (design D5).
            assert!(view.state.field_value(Field::ClientName).is_empty());
            assert!(view.state.field_value(Field::InvoiceNumber).is_empty());
        });
    }
}

/// Full-screen preview checks (task 5.2). They drive the overlay through the same
/// actions, keys, and clicks a user does, and check focus and scroll on each exit path.
/// Compiled with `--features gpui-mcp`, the only feature that enables GPUI's test support.
#[cfg(all(test, feature = "gpui-mcp"))]
mod fullscreen_tests {
    use super::*;
    use billing::identity_store::Identity;
    use gpui::{FocusHandle, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, point};

    fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
        cx.update(gpui_component::init);
        cx.update(bind_preview_keys);
        cx.add_window_view(|window, cx| {
            EditorView::new(AppState::new(Identity::default()), window, cx)
        })
    }

    fn is_open(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
        view.read_with(cx, |view, _| view.fullscreen.is_some())
    }

    fn focused(cx: &mut VisualTestContext) -> Option<FocusHandle> {
        cx.update(|window, cx| window.focused(cx))
    }

    fn form_offset(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
        view.read_with(cx, |view, _| view.form_scroll.offset())
    }

    /// Lays out the window, then scrolls the form to a non-zero offset. The check
    /// fails loudly if the form is too short to scroll, since a zero offset proves nothing.
    fn scroll_form_down(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
        cx.run_until_parked();
        view.update(cx, |view, _| {
            view.form_scroll.set_offset(point(px(0.), px(-40.)));
        });
        cx.run_until_parked();
        let offset = form_offset(view, cx);
        assert!(
            offset.y < px(0.),
            "the form must be scrollable for the restore check"
        );
        offset
    }

    /// Focuses the Client name field and scrolls the form, then opens the overlay with
    /// the action. Returns the focused field and the scroll offset to expect on exit.
    fn open_from_client_name(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
    ) -> (FocusHandle, Point<Pixels>) {
        let field = view.read_with(cx, |view, cx| {
            view.input_for(Field::ClientName).focus_handle(cx)
        });
        cx.update(|window, cx| window.focus(&field, cx));
        let offset = scroll_form_down(view, cx);
        cx.dispatch_action(ToggleFullscreen);
        assert!(is_open(view, cx), "the toggle must open the overlay");
        (field, offset)
    }

    /// Checks the state an exit must restore: overlay closed, the field focused again,
    /// and the form at the offset it had before the overlay opened.
    fn assert_restored(
        view: &Entity<EditorView>,
        cx: &mut VisualTestContext,
        field: &FocusHandle,
        offset: Point<Pixels>,
    ) {
        assert!(!is_open(view, cx), "the exit must close the overlay");
        cx.run_until_parked();
        assert_eq!(
            focused(cx).as_ref(),
            Some(field),
            "focus returns to the field"
        );
        assert_eq!(form_offset(view, cx), offset, "scroll offset is restored");
    }

    /// Fills the model the way the export tests do, so the gate passes.
    fn fill_valid_invoice(state: &mut AppState) {
        state.set_field(Field::SenderTaxNumber, "21/815/08150");
        state.set_field(Field::InvoiceNumber, "4");
        state.set_field(Field::InvoiceDate, "01/11/2026");
        state.set_field(Field::ServiceDate, "31/10/2026");
        state.set_field(Field::DueDate, "15/11/2026");
        state.set_field(Field::Iban, "DE89 3704 0044 0532 0130 00");
        state.add_line_item();
        state.set_line_item_text(0, LineItemField::Quantity, "1");
        state.set_line_item_text(0, LineItemField::Rate, "200");
    }

    #[gpui::test]
    fn toggle_action_opens_at_fit_width_and_closes(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.dispatch_action(ToggleFullscreen);
        assert!(is_open(&view, cx));
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.fullscreen.as_ref().map(|s| s.zoom),
                Some(Zoom::FitWidth)
            );
        });
        cx.dispatch_action(ToggleFullscreen);
        assert!(!is_open(&view, cx));
    }

    #[gpui::test]
    fn cmd_shift_f_keystroke_toggles_the_overlay(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_keystrokes("cmd-shift-f");
        assert!(is_open(&view, cx));
        cx.simulate_keystrokes("cmd-shift-f");
        assert!(!is_open(&view, cx));
    }

    #[gpui::test]
    fn preview_button_click_opens_the_overlay(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.run_until_parked();
        let center = cx
            .debug_bounds("preview.fullscreen")
            .expect("the Full-screen preview button is laid out")
            .center();
        cx.simulate_click(center, Modifiers::none());
        assert!(is_open(&view, cx));
    }

    #[gpui::test]
    fn escape_exits_and_restores_focus_and_scroll(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let (field, offset) = open_from_client_name(&view, cx);
        cx.simulate_keystrokes("escape");
        assert_restored(&view, cx, &field, offset);
    }

    #[gpui::test]
    fn cmd_shift_f_exits_and_restores_focus_and_scroll(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let (field, offset) = open_from_client_name(&view, cx);
        cx.simulate_keystrokes("cmd-shift-f");
        assert_restored(&view, cx, &field, offset);
    }

    #[gpui::test]
    fn close_button_exits_and_restores_focus_and_scroll(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        let (field, offset) = open_from_client_name(&view, cx);
        cx.run_until_parked();
        let center = cx
            .debug_bounds("fullscreen.close")
            .expect("the Close button is laid out")
            .center();
        cx.simulate_click(center, Modifiers::none());
        assert_restored(&view, cx, &field, offset);
    }

    #[gpui::test]
    fn blocked_export_from_overlay_closes_it_and_shows_errors(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.dispatch_action(ToggleFullscreen);
        cx.run_until_parked();
        let center = cx
            .debug_bounds("fullscreen.export-pdf")
            .expect("the toolbar's Export PDF is laid out")
            .center();
        cx.simulate_click(center, Modifiers::none());

        assert!(!is_open(&view, cx), "a blocked export closes the overlay");
        assert!(
            !cx.cx.did_prompt_for_new_path(),
            "no save dialog for a blocked export"
        );
        view.read_with(cx, |view, _| {
            assert!(view.export_attempted);
            assert!(export_gate::summary(&view.export_issues()).is_some());
            assert!(view.field_error(Field::InvoiceNumber).is_some());
        });
    }

    #[gpui::test]
    fn valid_export_from_overlay_opens_the_save_dialog_and_stays_open(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        view.update(cx, |view, cx| {
            fill_valid_invoice(&mut view.state);
            view.model_changed(cx);
        });
        cx.dispatch_action(ToggleFullscreen);
        cx.run_until_parked();
        let center = cx
            .debug_bounds("fullscreen.export-pdf")
            .expect("the toolbar's Export PDF is laid out")
            .center();
        cx.simulate_click(center, Modifiers::none());

        assert!(
            cx.cx.did_prompt_for_new_path(),
            "a valid export opens the save dialog"
        );
        assert!(is_open(&view, cx), "a valid export keeps the overlay open");
    }

    #[test]
    fn stepped_zoom_walks_the_steps_and_clamps_at_the_ends() {
        let up = |percent: f32| match stepped_zoom(percent, true) {
            Zoom::Percent(step) => step,
            Zoom::FitWidth => panic!("a step is never fit width"),
        };
        let down = |percent: f32| match stepped_zoom(percent, false) {
            Zoom::Percent(step) => step,
            Zoom::FitWidth => panic!("a step is never fit width"),
        };
        assert_eq!(up(100.), 125);
        assert_eq!(up(125.), 150);
        assert_eq!(up(300.), 300, "zoom in clamps at 300 %");
        assert_eq!(down(100.), 75);
        assert_eq!(down(50.), 50, "zoom out clamps at 50 %");
        // A fit-width percentage that rounds to a step (99.6 % is the 100 % step) steps
        // past that step, never onto it.
        assert_eq!(up(99.6), 125);
        assert_eq!(down(99.6), 75);
    }

    #[test]
    fn page_in_view_picks_the_page_with_the_most_overlap() {
        let pages = [(0., 100.), (110., 210.), (220., 320.)];
        assert_eq!(page_in_view((0., 100.), &pages), 0);
        assert_eq!(page_in_view((100., 200.), &pages), 1);
        assert_eq!(page_in_view((200., 300.), &pages), 2);
        // Equal overlap keeps the earlier page.
        assert_eq!(page_in_view((50., 160.), &pages), 0);
        // Before the first layout no page overlaps, so the indicator reads page 1.
        assert_eq!(page_in_view((0., 0.), &[]), 0);
    }

    /// Settles the debounced preview and the background renders the overlay waits on.
    fn settle(cx: &mut VisualTestContext) {
        cx.executor().advance_clock(PREVIEW_DEBOUNCE);
        cx.run_until_parked();
    }

    /// The page count and "Page n of N" text the overlay shows right now.
    fn page_text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, _| {
            let state = view.fullscreen.as_ref().expect("the overlay is open");
            fullscreen_page_text(state, view.fullscreen_shown_pages().len())
        })
    }

    /// Adds line items so the invoice runs to several pages. 50 rows make 5 pages with
    /// the padded table (see the raster tests).
    fn fill_many_items(view: &Entity<EditorView>, cx: &mut VisualTestContext, count: usize) {
        view.update(cx, |view, cx| {
            for _ in 1..count {
                view.state.add_line_item();
            }
            for index in 0..count {
                view.state.set_line_item_text(
                    index,
                    LineItemField::Description,
                    &format!(
                        "Workshop session {index} - Straße, Größe, Äpfel € § ä ß, a second line"
                    ),
                );
                view.state
                    .set_line_item_text(index, LineItemField::Quantity, "1");
                view.state
                    .set_line_item_text(index, LineItemField::Rate, "200");
            }
            view.model_changed(cx);
        });
    }

    /// Zoom change, sharp render (task 5.3): a 1000 px window has a 952 px fit width,
    /// about 160 %, so one step up is 200 % and one step down is 150 %.
    #[gpui::test]
    fn zoom_change_requests_a_sharp_raster_at_the_new_scale(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_resize(size(px(1000.), px(700.)));
        settle(cx);
        cx.dispatch_action(ToggleFullscreen);
        settle(cx);
        let scale = cx.update(|window, _| window.scale_factor());

        cx.simulate_keystrokes("cmd-=");
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.fullscreen.as_ref().map(|s| s.zoom),
                Some(Zoom::Percent(200))
            );
        });
        let requested = view.read_with(cx, |view, _| view.fullscreen_requested_px_per_pt);
        assert!(
            (requested - 2.0 * scale).abs() < 1e-3,
            "the request is at 200 % times the scale, got {requested}"
        );
        // The shown pages are the sharp render, so their pixel width is the A4 width at
        // that scale. A stretched column raster would be about 292 px times the scale.
        let width = view.read_with(cx, |view, _| {
            assert!(!view.fullscreen_pages.is_empty(), "the sharp render arrived");
            view.fullscreen_pages[0].size(0).width.0 as f32
        });
        let expected = A4_WIDTH_PT * 2.0 * scale;
        assert!(
            (width - expected).abs() <= 2.0,
            "the sharp page is {width} px wide, expected about {expected}"
        );

        cx.simulate_keystrokes("cmd--");
        settle(cx);
        let requested = view.read_with(cx, |view, _| view.fullscreen_requested_px_per_pt);
        assert!(
            (requested - 1.5 * scale).abs() < 1e-3,
            "one step down is 150 %, got {requested}"
        );
    }

    /// Cmd+0 returns to Fit width and re-renders at the fit-width scale (task 5.3).
    #[gpui::test]
    fn cmd_zero_returns_to_fit_width_and_rerenders(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_resize(size(px(1000.), px(700.)));
        settle(cx);
        cx.dispatch_action(ToggleFullscreen);
        settle(cx);
        cx.simulate_keystrokes("cmd-=");
        settle(cx);
        cx.simulate_keystrokes("cmd-0");
        settle(cx);
        let scale = cx.update(|window, _| window.scale_factor());
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.fullscreen.as_ref().map(|s| s.zoom),
                Some(Zoom::FitWidth)
            );
        });
        let expected = fullscreen_px_per_pt(Zoom::FitWidth, fullscreen_fit_width(1000.), scale);
        let requested = view.read_with(cx, |view, _| view.fullscreen_requested_px_per_pt);
        assert!((requested - expected).abs() < 1e-3, "got {requested}");
    }

    /// Page Down and Page Up move one page and clamp at the ends. The indicator tracks
    /// the page in view (task 5.3).
    #[gpui::test]
    fn page_keys_move_one_page_and_the_indicator_follows(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_resize(size(px(1000.), px(700.)));
        fill_many_items(&view, cx, 50);
        settle(cx);
        let count = view.read_with(cx, |view, _| view.preview_pages.len());
        assert!(count >= 3, "the invoice must run to several pages, got {count}");
        cx.dispatch_action(ToggleFullscreen);
        settle(cx);
        assert_eq!(page_text(&view, cx), format!("Page 1 of {count}"));

        cx.simulate_keystrokes("pagedown");
        settle(cx);
        assert_eq!(page_text(&view, cx), format!("Page 2 of {count}"));

        // More Page Downs than pages: the view stops on the last page.
        for _ in 0..count {
            cx.simulate_keystrokes("pagedown");
            settle(cx);
        }
        assert_eq!(
            page_text(&view, cx),
            format!("Page {count} of {count}"),
            "Page Down clamps at the last page"
        );

        cx.simulate_keystrokes("pageup");
        settle(cx);
        assert_eq!(page_text(&view, cx), format!("Page {} of {count}", count - 1));
    }

    /// A renderer that always fails, for the failed full-screen render path (task 5.3).
    fn failing_render(_: &Projection, _: f32) -> Result<Vec<Arc<RenderImage>>, String> {
        Err("forced failure".to_string())
    }

    /// A failed full-screen render clears the sharp pages of the older zoom, so the overlay
    /// falls back to the column pages instead of showing stale sharp ones (task 5.3).
    #[gpui::test]
    fn failed_fullscreen_render_falls_back_to_the_column_pages(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_resize(size(px(1000.), px(700.)));
        settle(cx);
        cx.dispatch_action(ToggleFullscreen);
        settle(cx);
        view.read_with(cx, |view, _| {
            assert!(!view.fullscreen_pages.is_empty(), "the sharp render arrived");
        });

        view.update(cx, |view, _| view.preview_renderer = failing_render);
        cx.simulate_keystrokes("cmd-=");
        settle(cx);
        view.read_with(cx, |view, _| {
            assert!(
                view.fullscreen_pages.is_empty(),
                "a failed render clears the older sharp pages"
            );
            assert!(!view.preview_pages.is_empty());
            let shown = view.fullscreen_shown_pages();
            assert_eq!(shown.len(), view.preview_pages.len());
            assert!(
                shown
                    .iter()
                    .zip(&view.preview_pages)
                    .all(|(now, column)| Arc::ptr_eq(now, column)),
                "the overlay shows the column pages"
            );
        });
    }

    /// A width-only refresh keeps the projection, so it must not re-render an open overlay
    /// whose scale is within `FULLSCREEN_RERENDER_TOLERANCE`. A 9 px resize moves the
    /// column past its threshold, but the fit-width scale by under 1 % (task 5.3).
    #[gpui::test]
    fn width_only_refresh_does_not_rerender_the_open_overlay(cx: &mut TestAppContext) {
        let (view, cx) = editor(cx);
        cx.simulate_resize(size(px(1000.), px(700.)));
        settle(cx);
        cx.dispatch_action(ToggleFullscreen);
        settle(cx);
        let before = view.read_with(cx, |view, _| view.fullscreen_generation);

        cx.simulate_resize(size(px(1009.), px(700.)));
        settle(cx);
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.fullscreen_generation, before,
                "a width-only refresh must not start a second full-screen render"
            );
        });
    }
}

/// Pure checks for the zoom, paging, and render-scale helpers (task 5.3). They run
/// without `--features gpui-mcp`.
#[cfg(test)]
mod zoom_paging_unit_tests {
    use super::*;

    fn percent_of(zoom: Zoom, fit: f32) -> u16 {
        match zoom {
            Zoom::Percent(step) => step,
            Zoom::FitWidth => panic!("expected a step, got fit width (fit {fit})"),
        }
    }

    #[test]
    fn zoom_steps_walk_all_seven_stops_up_and_down() {
        let mut stops = vec![50_u16];
        let mut current = 50.0_f32;
        for _ in 0..6 {
            let next = percent_of(stepped_zoom(current, true), 0.);
            stops.push(next);
            current = f32::from(next);
        }
        assert_eq!(stops, vec![50, 75, 100, 125, 150, 200, 300]);

        let mut down = vec![300_u16];
        let mut current = 300.0_f32;
        for _ in 0..6 {
            let next = percent_of(stepped_zoom(current, false), 0.);
            down.push(next);
            current = f32::from(next);
        }
        assert_eq!(down, vec![300, 200, 150, 125, 100, 75, 50]);
    }

    #[test]
    fn zoom_clamps_at_both_ends() {
        assert_eq!(percent_of(stepped_zoom(300., true), 0.), 300);
        assert_eq!(percent_of(stepped_zoom(50., false), 0.), 50);
        // Below the first stop, zoom in goes to 50, not further down.
        assert_eq!(percent_of(stepped_zoom(20., true), 0.), 50);
    }

    #[test]
    fn fit_width_percent_steps_from_its_own_value() {
        // A 1000 px window has a 952 px fit width, about 160 %: up is 200, down is 150.
        let fit = fullscreen_fit_width(1000.);
        let percent = zoom_percent(Zoom::FitWidth, fit);
        assert!((percent - 159.9).abs() < 0.1, "fit is {percent} %");
        assert_eq!(percent_of(stepped_zoom(percent, true), fit), 200);
        assert_eq!(percent_of(stepped_zoom(percent, false), fit), 150);
    }

    #[test]
    fn fit_width_is_clamped_to_the_narrowest_column() {
        assert_eq!(fullscreen_fit_width(1000.), 1000. - FULLSCREEN_PADDING_PX);
        assert_eq!(fullscreen_fit_width(10.), PREVIEW_MIN_COLUMN_PX);
    }

    #[test]
    fn render_scale_is_zoom_times_the_window_scale_factor() {
        // 100 % is one point per logical pixel, so the scale is the window's factor.
        let at_100 = fullscreen_px_per_pt(Zoom::Percent(100), 952., 2.0);
        assert!((at_100 - 2.0).abs() < 1e-4, "got {at_100}");
        let at_200 = fullscreen_px_per_pt(Zoom::Percent(200), 952., 2.0);
        assert!((at_200 - 4.0).abs() < 1e-4, "got {at_200}");
        // Fit width renders at the fit width's own scale.
        let fit = fullscreen_px_per_pt(Zoom::FitWidth, A4_WIDTH_PT, 1.0);
        assert!((fit - 1.0).abs() < 1e-4, "got {fit}");
    }

    #[test]
    fn paging_moves_one_page_and_clamps_at_both_ends() {
        assert_eq!(paged_index(0, 3, true), 1);
        assert_eq!(paged_index(1, 3, true), 2);
        assert_eq!(paged_index(2, 3, true), 2, "Page Down stops at the last page");
        assert_eq!(paged_index(2, 3, false), 1);
        assert_eq!(paged_index(0, 3, false), 0, "Page Up stops at the first page");
        assert_eq!(paged_index(0, 0, true), 0, "no pages, no movement");
    }
}

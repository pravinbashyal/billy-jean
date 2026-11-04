# Design

## Context

See proposal.md (Why). This change builds on the `add-invoice-builder` code and changes some of its decisions. The current state that shapes the approach:

- **Preview.**
  - `editor.rs` `preview_body` is a separate GPUI layout of the debounced `Projection` (300 ms `PREVIEW_DEBOUNCE`).
  - It shares content with `pdf::render_projection` but no layout. The designer listed seven visible differences between them: the headings, the table frame, the Notes placement, the totals alignment, the sender weight, and the missing A4 page.
  - Tests assert `view.preview == project(...)`. The export tests compare `pdftotext` output against `"{label}: {value}"` needles.
- **PDF (genpdf 0.2).** I checked these limits in `~/.cargo/registry/src/*/genpdf-0.2.0/src`:
  - Font sizes are `u8`, so only whole points are possible.
  - `Area::draw_line(points, Style)` uses only the style's colour. Line thickness can't be set, so every rule is the default ~1 pt stroke.
  - There are no filled shapes and no curves.
  - `SimplePageDecorator` has `set_header` but no footer slot.
  - Layout is flow-only, with no absolute positioning.
  - These are available: a public `CellDecorator` trait, `Element::padded(Margins)`, `Style::bold()`, `Style::with_color(Color::Rgb)`, and nested elements inside `TableLayout` cells.
  - Only `NotoSans-Regular.ttf` is bundled, and `font_family()` maps the bold slots to Regular.
- **Dates.**
  - The three dates are plain `Input`s (`invoice.date`, `invoice.service_date`, `invoice.due_date`). The model stores free-text `String`s, and `date.rs` parses strict `DD/MM/YYYY`.
  - gpui-component 0.9 `time::DatePickerState` provides `date_format`, `first_day_of_week` (Sunday by default), `set_date` (which also updates its internal `CalendarState`), and `DatePickerEvent::Change`.
  - It binds only `enter` (open/confirm), `escape`, and `delete`/`backspace` in the `"DatePicker"` key context. The calendar has no arrow-key day navigation.
- **Identity storage.**
  - `identity_store` writes `schema_version: 1` plus the flattened `Identity { sender, bank }`, and rejects any other version.
  - `Sender` has four `String` fields and no `deny_unknown_fields`, so older builds ignore extra keys.
- **GPUI images.** `gpui::RenderImage::new(frames)` and `ImageSource::Render(Arc<RenderImage>)` are available in gpui-pre 0.3.11. Frames are `image::Frame` buffers; GPUI expects BGRA byte order.
- **Memory constraints.**
  - The totals wording stays "Total amount (Net)" / "Total amount (Gross)".
  - Any new font weight must be bundled and registered as a real face.
  - The `gpui_platform` `font-kit` feature must stay on. Without it, quads render but glyphs don't, which shows up as invisible text.

## Goals / Non-Goals

**Goals:**
- One rendering path. The preview is a raster of exactly the bytes Export PDF writes, so the two cannot drift again.
- Keep the existing content contract (`Projection`) as the single thing published to gpui-mcp and asserted in tests.
- Make the date fields fully usable from the keyboard even though they are calendar-only.

**Non-Goals:**
- Exact DIN 5008 address-window positioning (45 mm from the top). genpdf can't position absolutely. The return line sits at the natural top of the client block.
- Repeating the line-items header row on page 2, and pinning footers to the bottom of the page. genpdf 0.2 can do neither, so footers flow after the content.
- Selectable text in the preview, an in-app print path, and pinch-zoom.
- Any change to the VAT label (`VAT (n%)`), EUR, or the date validation rules in the export gate.

## Decisions

**D1: The preview rasterizes the real PDF in-process with `hayro`.** Pipeline:

```
 edit --> debounce 300ms --> project(invoice) --> Projection (published to gpui-mcp, tests)
                                   |
                                   v   background executor, generation-tagged
                        render_projection(&p) -> pdf bytes   (same fn Export PDF uses)
                                   |
                                   v
                     raster::rasterize(&bytes, px_per_pt) -> Vec<PageImage RGBA>
                                   |   RGBA -> BGRA, wrap in RenderImage
                                   v
           latest generation wins; on Err keep last good pages + notice
```

- **New module `src/raster.rs`.** A thin wrapper over `hayro`: `rasterize(bytes, scale) -> Result<Vec<PageImage>, RasterError>`. Nothing outside it touches `hayro` types.
- **Threading.** Rendering runs on `cx.background_executor()`. A monotonically increasing `preview_generation` discards stale results.
- **Last good render.** The editor keeps `preview_pages: Vec<Arc<RenderImage>>` from the last good render plus `preview_error: bool`.
- **Scale.** The preview column renders at `px_per_pt = column_width_px * window.scale_factor() / 595.28`.
  - A column-width change of more than 8 px schedules a re-render through the same debounce.
  - Full-screen mode renders at `zoom * scale_factor` (D8).
- **Export snapshot.** Export keeps rendering from the projection snapshotted at click (current behaviour), so preview and export share code and inputs. The D3 claim from `add-invoice-builder` becomes literally true.
- **Alternatives rejected.**
  - Keep a GPUI-drawn sheet driven by shared layout tokens (designer F3 option B). It's never exact, and every PDF tweak would have to be made twice.
  - Use `pdfium`. That's a C library loaded as a dylib, which breaks "no external tools".
  - Shell out to `pdftoppm`. Same problem: it's an external tool.
  - Use the `pdf-render` fork. It needs a commercial licence.
- **Spike gate (task 1.1).** Raster preview is only adopted if both of these hold on the reference invoice:
  - (a) Rendering one page at preview scale (about 1.3-2 px/pt) takes 150 ms or less end-to-end (genpdf + hayro) in a release build on the dev Mac, and 3-page invoices stay under 400 ms.
  - (b) A side-by-side with `pdftoppm -r 144` shows no missing or misplaced glyphs. Check €, §, ä/ß, and bold vs regular.
- **Fallback.**
  - If (a) fails, render the pages at the column scale only after edits settle, and keep the full-screen re-render on demand.
  - If (b) fails, or (a) fails badly (over 500 ms), stop and report to the PO. The fallback is designer option B: a GPUI A4 sheet driven by a shared `pdf::tokens` module. That weakens the "Preview shows the exact exported page" scenario, and changing a spec scenario needs the PO's agreement.

**D2: The date fields are calendar-only `DatePicker`s, with a small keyboard wrapper.** This supersedes `add-invoice-builder` D6 for the picker. Per the PO, there is no typed entry.

- **Picker setup.** Each date gets a `DatePickerState` with `.date_format("%d/%m/%Y")`, `.first_day_of_week(Weekday::Mon)`, a `cleanable` picker, and a `DD/MM/YYYY` placeholder.
- **Writing the model.** On `DatePickerEvent::Change(dt)` the editor writes `dt.format("%d/%m/%Y")`, or `""` when the picker is cleared, through `AppState::set_field`. So `InvoiceMeta`, `date.rs`, `export_gate`, and `project` don't change.
- **Initialising the picker.** On construction, a model string that parses with `date::parse` is converted to `NaiveDate` and passed to `set_date`.
- **Malformed dates.** The `MalformedDate` gate path stays as a model-level guard, even though the UI can no longer produce malformed text.
- **Keyboard wrapper.**
  - Each picker is wrapped in a `div().key_context("InvoiceDateField")`.
  - Our own actions are bound in that context: `left`/`right` move one day, `up`/`down` move seven days, `pageup`/`pagedown` move one month, and `shift-pageup`/`shift-pagedown` move one year.
  - The handler computes the new `NaiveDate` with chrono (`checked_add_days`/`checked_add_months`, starting from today when the field is empty), calls `set_date`, and writes the model. This works because `update_date(emit=false)` doesn't emit `Change`.
  - Actions dispatched from the focused picker bubble up to the wrapper's `on_action`, so the stock Enter/Escape/Delete bindings keep working underneath.
  - Task 4.3 found that in gpui-component 0.9 `set_date` closes an open popover (`update_date` sets `open = false`) and the public API cannot reopen it conditionally. PO waiver: a keyboard step closes an open calendar; the value and `CalendarState` still update, so the next open shows the new month.
- **Rejected options.**
  - Typed entry (designer F1 option B): the PO chose calendar-only.
  - Arrow keys inside the calendar grid: that would mean patching gpui-component.
- **gpui-mcp.** The three date ids change role from `textbox` to `datepicker`, and `type_text` into them reports an error. That is consistent with calendar-only. The interaction gate drives them through click and the D2 keys.

**D3: Bundle Bold and SemiBold Noto Sans with the bold styling.**
- **Font files.**
  - Add `assets/fonts/NotoSans-Bold.ttf` and `NotoSans-SemiBold.ttf`, the static instances from the same Noto Sans release as the bundled Regular. They are covered by the existing `OFL.txt`.
  - In genpdf, `FontFamily { regular: Regular, bold: Bold, italic: Regular, bold_italic: Bold }`. SemiBold is UI-only, because genpdf has a single bold flag.
  - In `main.rs`, register all three faces with `add_fonts`. The theme `font_family` stays `"Noto Sans"`.
- **Font resolution risk.** The static SemiBold's name table reports the legacy family (nameID 1) as `Noto Sans SemiBold`, with `Noto Sans` only in the typographic family (nameID 16). font-kit may therefore fail to resolve `FontWeight::SEMIBOLD` under `"Noto Sans"`, which is the invisible-text failure mode from memory.
- **Verification and fallback.** Task 1.2 checks in the running app, not just in tests, that semibold panel titles render visible glyphs. If SemiBold doesn't resolve, map the UI's semibold uses to `FontWeight::BOLD` and drop the SemiBold file rather than shipping invisible text.
- **The `font-kit` feature stays on** in `Cargo.toml`.

**D4: PDF layout within genpdf 0.2.** A new private `pdf::style` section holds the tokens. The renderer is rebuilt from a few pieces:

- **Tokens (whole points only, because sizes are `u8`; the designer's half-point sizes are rounded):**

  | Role | pt | Weight / colour |
  |---|---|---|
  | Return line | 8 | regular, muted |
  | Labels (Billed to, meta labels, Notes, table header) | 8 | regular, muted |
  | Sender address, tax and contact lines | 9 | regular, muted (value ink) |
  | Body, table cells, client address | 10 | regular, ink |
  | Meta values | 10 | bold, ink |
  | Gross row | 11 | bold, ink |
  | Sender name: Letter / Header / Footer column | 13 / 16 / 8 | bold, ink |
  | Client name | 14 | bold, ink |
  | Bank footer | 8 | regular, muted |

  Colours: ink `#111827`, muted `#6B7280`, row rule `#E5E7EB`, closing rule `#D1D5DB`, accent `#00B8D9`. Margins: `Margins::trbl(18, 20, 18, 20)` mm. Line spacing 1.3 inside table cells and 1.25 elsewhere. Spacing comes from `padded` margins in mm (1 / 2 / 4 / 8 / 12), not from `Break` lines.
- **`InvoiceTableDecorator: CellDecorator`.**
  - It draws only horizontal rules: accent under row 0, `#E5E7EB` between body rows, and `#D1D5DB` under the last row. No vertical lines and no frame.
  - Every cell is `padded(trbl(2.5, 2.0, 2.5, 2.0))`. The first column has 0 left padding and the last column has 0 right padding.
  - Column weights are 10:2:3:3.
  - Because stroke width can't be set, rules differ by colour only. The accent rule may be drawn as two adjacent strokes about 0.3 mm apart, if one stroke reads too faint in the spike render.
- **`Hairline` element.** A custom `Element` that draws one full-width `#E5E7EB` line with vertical padding. It's used before the bank footer, under the return line, and after the Bold header block.
- **Two-column bands.** These are `TableLayout`s without a decorator, holding nested `LinearLayout`s: Billed to + meta grid, and the Letter-layout top band. The 2x2 meta grid is a nested 2-column table of label-over-value cells.
- **Totals.** A `TableLayout` with weights about 55:25:20 (empty spacer, label, amount), so the block takes about 45 % of the width on the right. A `Hairline` sits above Gross, and Gross is bold.
- **Notes.** Rendered only when `standing_note` or `custom_notes` is present (unchanged rule).
- **No document title on the page.** The PDF metadata title stays "Invoice".

**D5: Sender model and storage.**
- `Sender` gains `tax_number`, `vat_id`, `email: String`, and `layout: SenderLayout`. Each new field is `#[serde(default)]`.
- `SenderLayout` is `{ Letter (default), Header, Footer }`, with `#[serde(rename_all = "lowercase")]`.
- `SCHEMA_VERSION` stays 1. The change is purely additive: old files load with defaults, and old builds ignore the new keys.
- **Rejected:** bumping to 2 with a migration. It adds code for no benefit, and a careless bump would reject current users' files and empty the editor.
- The layout setting lives on `Sender`, not in a separate preferences struct, so it flows into `Invoice` → `Projection` and into the identity store without new plumbing.

**D6: Projection shape.**
- `sender_header: String` is replaced by `sender: SenderBlock { name, address_lines, tax_lines: Vec<(&'static str, String)>, email: Option<String>, return_line }` and `sender_layout: SenderLayout`.
- `pdf::return_line(&Sender)` replaces `sender_header`. It's pure, joins with ` · `, omits Country, and skips empty segments. `AppState::sender_header` becomes `return_line`.
- **Meta order.** `meta` keeps its `(label, value)` pairs. The renderer places them in the source's grid order: Invoice date | Service date / Invoice number | Due date.
- **Beneficiary fallback.** `bank` uses the effective beneficiary: `beneficiary.trim()`, or `sender.name.trim()` when that is empty. Only the projection does this; the stored Beneficiary stays empty, so changing the Name later still flows through. The Beneficiary input's placeholder is updated whenever the sender Name changes.

**D7: Export gate.**
- Add `ExportIssue::MissingTaxNumber`, bound to `Field::SenderTaxNumber`.
- Inline text: "Enter your tax number (Steuernummer)." Summary item: "Tax number".
- It is checked first, because Sender is the first panel and `check` returns issues in display order.
- VAT ID and Email are never checked.

**D8: Full-screen preview as an in-window overlay.**
- **State.** `EditorView.fullscreen: Option<FullscreenState { zoom: Zoom::{FitWidth, Percent(u16)}, scroll: ScrollHandle, restore_focus: Option<FocusHandle> }>`.
- **Form scroll position.** The form column switches to an explicit, persistent `ScrollHandle`, so its offset survives while it's unmounted.
- **Rendering.** When the overlay is active, `render` returns the overlay in place of the two-column layout.
- **Key bindings.** `cmd-shift-f` (ToggleFullscreen) is bound in the editor's root key context. `escape`, `cmd-=`, `cmd--`, `cmd-0`, `pageup`, and `pagedown` are bound in a `"FullscreenPreview"` context, which only exists while the overlay is open, so the DatePicker's own Escape binding is never shadowed.
- **Zoom.** Zoom steps are `[50, 75, 100, 125, 150, 200, 300]`. Each zoom change re-rasterizes at `zoom/100 * scale_factor` through D1. The old images stay on screen, scaled, until the sharp ones arrive.
- **Page indicator.** "Page n of N" comes from the scroll offset divided by page height plus gap, rounded to the page that is mostly in view.
- **Export.** The toolbar's Export PDF calls the same `on_export_pdf`. If the gate blocks, the overlay closes so the errors are visible.
- **Rejected:** a separate window (it breaks the single-window spec) and native macOS full screen (users can already do that themselves).

**D9: Editor styling.**
- Hard-coded hex values are replaced with `cx.theme()` tokens (`muted_foreground`, `border`, `danger`, `success`, `background`/`secondary` for the canvas).
- A 4 px grid: panel padding 16, gap between panels 12, gap between fields 12, gap between label and input 4.
- Panel titles are 14 px SemiBold (D3), and field labels are 13 px muted.
- `Button::new("export-pdf").primary()`.
- Invoice details become a 2x2 grid: Invoice number | Invoice date / Service date | Due date.
- The Sender layout control is a horizontal gpui-component `RadioGroup` (`sender.layout`) labelled "German letter", "Bold header", and "Footer columns".

**D10: gpui-mcp surface.**
- New nodes:
  - `sender.tax_number`, `sender.vat_id`, `sender.email` (textbox);
  - `sender.layout` (radiogroup, with its value);
  - `preview.fullscreen`, `fullscreen.close`, `fullscreen.zoom-in`, `fullscreen.zoom-out`, `fullscreen.fit-width`, `fullscreen.export-pdf` (button);
  - `fullscreen.zoom` and `fullscreen.page` (text);
  - `preview.pages` (text: page count plus `ok` / `rendering` / `error`).
- `preview.sender` now publishes the return line and the sender-block lines.
- The existing `preview.*` content nodes keep publishing the `Projection`, so the MCP checks and the `view.preview == project(...)` tests still hold.

## Risks / Trade-offs

- **hayro is young and the author calls it incomplete** → genpdf output uses only TrueType text and stroked lines, none of the missing features (blending, knockout, encryption). The spike gate in D1 checks speed and fidelity before anything is built on it, and the fallback is named.
- **A CPU render on every debounce** → it runs off the UI thread, stale generations are dropped, and the last good image stays on screen. The budget is enforced in the spike.
- **Preview text can't be selected** → accepted; the PDF is the artifact.
- **Rule thickness can't be set in genpdf 0.2** → rules are told apart by colour. The accent rule can be doubled. Forking genpdf for thinner lines is out of scope.
- **SemiBold may not resolve under "Noto Sans" (invisible text)** → verified visually in task 1.2, with a fallback to Bold.
- **Calendar-only dates are slow for far-off dates with a mouse** → the D2 keyboard wrapper gives month and year jumps. The PO accepted calendar-only.
- **The tests' `"{label}: {value}"` PDF text needles break** → task 3.1 replaces them with label-and-value checks that tolerate the grid's reading order in `pdftotext`. Tests keep using `pdftotext` (a dev-time tool, not a runtime dependency).
- **The MODIFIED deltas target requirements that so far exist only in the unarchived `add-invoice-builder`** → archive `add-invoice-builder` (its task 10.1 is still open) before archiving this change.
- **Footers flow after the content instead of sitting at the page bottom** → accepted (Non-Goals). Footer-column layout C reads as a closing block rather than a true page footer.

## Migration Plan

- There is no data migration. `schema_version` stays 1, and identity files from the earlier version load with empty tax and contact values and the Letter layout. The first save writes the new keys.
- Rollback: an older build ignores the new keys and keeps working on the same file. Only its tax, contact, and layout values are lost from view.

## Open Questions

- The exact rounding of the designer's half-point sizes (D4 table) may be retuned after the spike renders, but rendering stays whole-point only. This changes neither the specs nor the task breakdown.
- Whether the accent rule needs the doubled stroke is decided from the spike render.

## Spike results

Task 1.1 (`hayro` raster preview, D1). Measured on the dev Mac in a release build with `cargo test --release --test raster_spike -- --ignored --nocapture`. Each time is the median of 5 runs after one warm-up, and covers `render_invoice` (genpdf) plus `raster::rasterize` (hayro).

| Case | Scale | Median | Budget | Verdict |
|---|---|---|---|---|
| Reference invoice, 1 page | 1.5 px/pt | 65.4 ms | 150 ms or less | PASS |
| Reference invoice, 1 page | 3.0 px/pt | 66.5 ms | not gated | info |
| 50-item invoice, 3 pages | 1.5 px/pt | 71.0 ms | under 400 ms | PASS |
| 50-item invoice, 3 pages | 3.0 px/pt | 78.0 ms | not gated | info |

- **Re-measured 2026-11-03 (after task 3.2).** The 50-item fixture now renders 5 pages, not 3, because the task 3.2 table padding adds height. The re-measured 5-page median at 1.5 px/pt is 74.4 ms against the under 400 ms budget: PASS. The 3-page rows above are the original measurement and are kept as written.

- **Gate (a): PASS.** Both budgets hold at preview scale with a wide margin. The 3.0 px/pt numbers stay well under the 500 ms fallback threshold as well.
- **Fixture note.** The 3-page case uses 50 line items, not about 60. With the descriptions used here, 60 rows came out at 4 to 5 pages, and 50 gives exactly 3.
- **Gate (b): fidelity check, side by side at 144 dpi.** The hayro render matches `pdftoppm -r 144` in glyph shapes and text positions, on both the reference invoice and the 3-page invoice. The euro sign, section sign, and ä/ß/ö characters all render correctly.
- **Gate (b), bold half: PASS.** Added after task 1.2 bundled the Bold face. A single-line sample was rendered with the real `pdf::font_family()` (`Style::bold()` against regular, 14 pt, text "Straße Größe Äpfel € § ä ß ö 0123456789"). It was rasterized at 2.0 px/pt and compared with `pdftoppm -r 144`. Bold is visibly heavier in both renderers: 4920 dark pixels against 3011 for regular in the hayro render, about 63 % more ink. Glyph shapes and positions match between hayro and pdftoppm, and € § ä ß ö render in both weights. The sample PDF embeds the Bold face (the `pdf` test `bold_text_embeds_the_bold_face` checks this). The regenerating test is `pdf::tests::spike_bold_vs_regular_sample` (ignored).
- **Artifacts** (in the gitignored `output-test/spike/`): `reference.pdf`, `hayro-reference-144dpi.png`, `pdftoppm-reference-144dpi-1.png`, `three-page.pdf`, `hayro-three-page-p1-144dpi.png`, `pdftoppm-three-page-p1-144dpi-1.png`, `bold-vs-regular-regular.pdf`, `bold-vs-regular-bold.pdf`, the matching hayro and pdftoppm PNGs (`hayro-bold-vs-regular-*-144dpi.png`, `pdftoppm-bold-vs-regular-*-144dpi.png`), and `side-by-side-bold-vs-regular-144dpi.png`.
- **Decision.** Both halves of gate (b) pass, and the gate passes overall, so the D1 raster preview is adopted. The D1 fallback is not used.

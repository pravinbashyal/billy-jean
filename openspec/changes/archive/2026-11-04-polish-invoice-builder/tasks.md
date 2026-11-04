# Tasks

## 1. Spike and font foundations

- [x] 1.1 Spike `hayro` raster preview (design D1).
  - Add the `hayro` dependency (0.9).
  - Create `src/raster.rs` with `rasterize(pdf_bytes, px_per_pt) -> Result<Vec<PageImage>, RasterError>`. `PageImage` holds width, height, and RGBA bytes. No `hayro` types leak out of the module.
  - Add an ignored-by-default timing test (or a small `examples/` binary) that renders the reference invoice to PDF bytes with `pdf::render_invoice` and rasterizes it at 1.5 and 3.0 px/pt. Time a 1-page invoice and a 3-page one (about 60 line items) in a release build.
  - Verify:
    - A unit test shows `rasterize` returns one image per page with the expected A4 aspect (about 1:1.414).
    - Record the measured genpdf+hayro times and the gate verdict in a "Spike results" section appended to `design.md`. The budget is 150 ms or less for one page and under 400 ms for three pages.
    - Write PNGs of the hayro render and of `pdftoppm -r 144` to `output-test/spike/` for a side-by-side check that €, §, ä/ß and the text positions match.
  - If the gate fails, stop. Report to the PO with the D1 fallback; do not continue to group 5.
- [x] 1.2 Bundle Bold and SemiBold Noto Sans (design D3).
  - Add `assets/fonts/NotoSans-Bold.ttf` and `NotoSans-SemiBold.ttf` from the same Noto Sans release as the bundled Regular. Note them in `assets/fonts/` beside `OFL.txt`.
  - Map genpdf `FontFamily` bold and bold_italic to Bold.
  - Register all three faces in `main.rs` `add_fonts`.
  - Keep the `gpui_platform` `font-kit` feature and the `"Noto Sans"` theme family unchanged.
  - Verify:
    - A unit test renders a PDF whose bold text uses the Bold face. Run `pdffonts` on the output and confirm it lists a Bold Noto Sans font as well as Regular.
    - Launch the running app (`cargo run --features gpui-mcp`) and screenshot it via gpui-mcp to confirm the existing semibold panel titles render visible glyphs, not blank space.
    - If SemiBold does not resolve, apply the D3 fallback (semibold uses become Bold, drop the SemiBold file) and show the screenshot again.

## 2. Sender model, projection, and export gate

- [x] 2.1 Extend `Sender` and the identity store (design D5).
  - Add `tax_number`, `vat_id`, and `email` (String) and `layout: SenderLayout` (Letter by default, then Header and Footer; serialised lowercase) to `Sender`. Every new field gets `#[serde(default)]`; `SCHEMA_VERSION` stays 1.
  - Add `Field::SenderTaxNumber`, `SenderVatId`, and `SenderEmail` to `AppState` `field_value`/`set_field`. Add a setter for the layout. Make sure the new sender values and the layout are included in `identity()` and saved.
  - Verify with unit tests that:
    - a fixture JSON in the old four-field sender shape loads with empty tax/VAT/email and `SenderLayout::Letter`;
    - a full identity with every new field and each layout round-trips through `save_to`/`load_from`;
    - setting each new field through `AppState` updates `Invoice.sender` and `identity()`.
- [x] 2.2 Update the projection and the export gate (design D6, D7).
  - Replace `Projection.sender_header` with `sender: SenderBlock` (name, address lines, tax lines, optional email, return line) plus `sender_layout`.
  - Replace `sender_header()` with `return_line()`: joined with ` · `, Country omitted, empty segments skipped. Rename `AppState::sender_header` to match.
  - Make the bank projection use the effective beneficiary (Beneficiary, or the sender Name when Beneficiary is blank).
  - Add `ExportIssue::MissingTaxNumber`: message "Enter your tax number (Steuernummer).", summary item "Tax number", checked first. Never check VAT ID or Email.
  - Verify with unit tests that:
    - the return line for `Alexandra Sharma / Musterstraße 12 / 10115 Musterstadt / Germany` is exactly `Alexandra Sharma · Musterstraße 12 · 10115 Musterstadt`, and missing segments leave no stray separator;
    - VAT ID and Email lines are absent from `SenderBlock` when blank;
    - the beneficiary falls back to the Name only when Beneficiary is blank;
    - `check` reports `MissingTaxNumber` for an empty or whitespace-only tax number and not otherwise, and `summary` names "Tax number";
    - the existing gate and projection tests still pass with their expectations updated (`cargo test`).

## 3. PDF redesign

- [x] 3.1 Rebuild the PDF page skeleton (design D4; spec "Invoice page layout", "Invoice typography and colour").
  - Add style tokens: whole-point sizes, ink/muted/rule/accent colours, and 20/18 mm margins.
  - Add a `Hairline` element and the mm spacing helpers.
  - Lay out the Billed to + 2x2 meta grid band (Invoice date | Service date / Invoice number | Due date; labels muted, values bold). Then the totals block (right-aligned, about 45 % wide, hairline above a bold Gross, wording "Total amount (Net)" / "VAT (n%)" / "Total amount (Gross)"). Then Notes, only when there is content. Then the muted bank footer below a full-width hairline.
  - No on-page title, wave, or band. The sender treatment is a temporary Letter-layout stub that 3.3 replaces.
  - Rewrite the test helpers in `src/editor.rs` and `src/pdf.rs` that look for `"{label}: {value}"` needles so they check labels and values separately.
  - Verify:
    - `cargo test` passes, including a test that the extracted `pdftotext` text has no `Invoice date:`-style `Label: value` metadata line, and a test that the Notes label is absent at a non-zero VAT rate with empty notes.
    - Rasterize the reference invoice with `raster::rasterize` to `output-test/pdf-3-1.png` for visual review.
- [x] 3.2 Pad the line-items table (design D4; spec "Padded line-items table").
  - Replace `FrameCellDecorator` with `InvoiceTableDecorator`, which draws horizontal rules only: accent under the header, `#E5E7EB` between rows, `#D1D5DB` under the last row.
  - Pad every cell with `trbl(2.5, 2.0, 2.5, 2.0)` mm, with 0 on the outer side of the first and last columns.
  - Use column weights 10:2:3:3, a muted 8 pt header with right-aligned numeric headers, a 10 pt ink body with 1.3 line spacing, and 6 mm before and after the table.
  - Verify:
    - A unit test covers the decorator's rule plan: for N rows it emits one accent rule, N-2 row hairlines, one closing rule, and no vertical lines.
    - Render a 3-row invoice, including one long wrapped description, to `output-test/pdf-3-2.png`. It must show padded cells, no vertical grid, a cyan header rule, and numeric cells right-aligned on the first line of the wrapped row.
- [x] 3.3 Render the three sender layouts (spec "Selectable sender layouts").
  - **Letter:** a right-aligned top-right block (13 pt bold name, then 9 pt muted address lines and the tax/VAT/email lines), then the 8 pt muted return line with a hairline directly above "Billed to"; the bank footer as in 3.1.
  - **Header:** a 16 pt bold name at the top left, the street, "Postal City, Country", and one ` · `-joined tax/VAT/email line, then a hairline; no return line.
  - **Footer:** the return line at the top; after the content, a hairline and three equal muted columns (sender with bold name / tax and contact / bank), which replace the standalone bank footer.
  - Verify:
    - Unit tests show the `pdftotext` text for each layout contains the expected sender lines.
    - Only Footer has the three-column labels; only Letter and Footer contain the return line; blank VAT ID and Email produce no line.
    - Render one PNG per layout to `output-test/pdf-layout-{letter,header,footer}.png` and compare them with the PO-approved mockup `/private/tmp/claude-501/-Users-pravinbashyal-project-billing/907ef5e9-eebb-4c2b-9f86-31571ccdd7ce/scratchpad/sender-layouts.html`.

## 4. Editor form

- [x] 4.1 Update the Sender and Bank panels (spec "Editing sender identity", "Editing bank details with IBAN validation", "Persisting sender and bank identity between runs").
  - Add the Tax number, VAT ID, and Email inputs (`sender.tax_number`, `sender.vat_id`, `sender.email`) with the tax-number inline error.
  - Add a horizontal "Sender layout" RadioGroup (`sender.layout`): German letter / Bold header / Footer columns.
  - Replace "Rendered header: …" with the muted return-line caption.
  - Set the Beneficiary input placeholder to the current sender Name and keep it updated.
  - Save-on-edit covers the new fields and the layout.
  - Publish the new gpui-mcp nodes (design D10).
  - Verify:
    - GPUI tests show that editing each new field and switching the layout updates `view.preview` after the debounce and writes the identity store, and that a reload restores them.
    - Export with an empty Tax number shows the inline error and the "Tax number" summary item.
    - The caption reads `Name · Street · Postal City`.
- [x] 4.2 Replace the three date inputs with calendar pickers (spec "Editing invoice number and dates"; design D2).
  - Use `DatePickerState` with `date_format("%d/%m/%Y")`, Monday as the first day of the week, cleanable, and a `DD/MM/YYYY` placeholder.
  - `Change` writes `DD/MM/YYYY`, or `""` on clear, to the model. The pickers initialise from parseable model strings.
  - Labels lose the "(DD/MM/YYYY)" suffix. Inline export errors still show beneath each field.
  - The gpui-mcp role becomes `datepicker`, and `type_text` into it returns an error.
  - Verify:
    - GPUI tests show that setting a picker's date writes `03/10/2026` to the model and preview after the debounce, and that clearing it empties the model and, after an export attempt, shows "Enter a date in DD/MM/YYYY format.".
    - In the running app, via gpui-mcp, clicking `invoice.date` opens a Monday-first calendar and clicking a day closes it with the field showing the DD/MM/YYYY value.
- [x] 4.3 Add the date-field keyboard wrapper (spec "Keyboard operation of date fields"; design D2).
  - Add an `InvoiceDateField` key context with actions for ±1 day (left/right), ±7 days (up/down), ±1 month (pageup/pagedown), and ±1 year (shift-pageup/shift-pagedown). Each action starts from today when the field is empty, calls `set_date`, and writes the model. The stock Enter/Escape/Delete still work.
  - Verify:
    - Unit tests for the date arithmetic cover month-end clamping (31/01 + 1 month = 28/02 or 29/02) and the year step.
    - A GPUI test shows that Right, then Down, then Page Down from `03/10/2026` yields `04/10/2026`, `11/10/2026`, then `11/11/2026` in the model.
    - In the running app via gpui-mcp: Tab into `invoice.date`, press Enter to open, Escape to close without changing the value, and confirm that stepping with the calendar open closes it and the next open shows the value's month (PO waiver: close-on-step, no reopen).
- [x] 4.4 Apply the editor styling (spec "Consistent editor styling"; design D9).
  - Replace the hard-coded hex colours in `editor.rs` with `cx.theme()` tokens.
  - Use the 4 px spacing grid, 14 px SemiBold panel titles, and 13 px muted labels.
  - Make Export PDF `.primary()`.
  - Lay out Invoice details as a 2x2 grid (Invoice number | Invoice date / Service date | Due date).
  - Verify:
    - `grep -n 'rgb(0x' src/editor.rs` returns no editor-chrome colours. The preview canvas is allowed only if it is themed.
    - A gpui-mcp screenshot of the running app shows consistent panels, the 2x2 details grid, a single primary Export PDF button, and visible text in every weight used.

## 5. Raster preview and full-screen

- [x] 5.1 Make the preview a raster of the exported PDF (spec "Live preview of the current invoice"; design D1).
  - After the debounce, run `render_projection` and then `raster::rasterize` on the background executor. Each result is tagged with a generation, and only the latest one is applied.
  - Convert the pages to BGRA `RenderImage`s and show them as A4 sheets with a shadow on the themed canvas, scaled to the column width. Re-render when the width changes by more than 8 px.
  - Keep the last good pages on failure and show "Preview could not be updated"; clear the notice on the next success.
  - Stack multiple pages with "Page n of N" captions.
  - Delete `preview_body`. Keep `view.preview: Projection` and the `preview.*` nodes, and add `preview.pages`.
  - Verify:
    - The existing debounce tests pass.
    - A GPUI test shows a failed render keeps the previous pages and sets the notice, and a stale generation result is dropped.
    - A test shows the preview's rasterised pixels equal the rasterised `render_projection(&view.preview)` (PDF bytes are not deterministic: the trailer ID and Info stamps vary).
    - A gpui-mcp screenshot of the running app shows the preview matching `output-test/pdf-layout-letter.png`.
- [x] 5.2 Add full-screen preview enter/exit/export (spec "Full-screen preview"; design D8).
  - Add a `preview.fullscreen` button and a `cmd-shift-f` toggle.
  - The overlay replaces the two-column layout. Its toolbar has Close, the zoom controls, the zoom %, Fit width, "Page n of N", and Export PDF (`fullscreen.*` ids).
  - Escape, Cmd+Shift+F, and Close exit and restore the form scroll offset (via a persistent `ScrollHandle`) and the previously focused field.
  - Export from the toolbar goes through the same gate; when blocked, it closes the overlay.
  - Verify:
    - GPUI tests show the toggle via action and button, the three exit paths restoring focus and scroll offset, and a blocked export closing the overlay with errors shown.
    - In the running app via gpui-mcp: Cmd+Shift+F opens the overlay at Fit width, and Escape returns.
- [x] 5.3 Add full-screen zoom and paging (spec "Full-screen preview zoom and paging"; design D8).
  - Zoom steps are 50/75/100/125/150/200/300 % via Cmd+=/Cmd+- and the buttons, with Cmd+0 or Fit width to reset.
  - Each zoom change re-rasterizes at `zoom * scale_factor`, with the old image shown scaled until the new one arrives.
  - Scroll horizontally when the page is wider than the window.
  - Page Up/Page Down move one page, and "Page n of N" tracks the page that is mostly in view.
  - Verify:
    - Unit tests cover the zoom step sequence and clamping, and the page-in-view computation.
    - A GPUI test shows a zoom change requests a raster at the new scale.
    - In the running app via gpui-mcp: on a 3-page invoice, 200 % is sharp (screenshot), Page Down advances the indicator to "Page 2 of 3", and Cmd+0 returns to Fit width.

## 6. Integration

- [x] 6.1 Run an end-to-end interaction check of this change.
  - With `cargo run --features gpui-mcp`, drive every scenario returned by `grep -rn '^#### Scenario: \[interaction\]' openspec/changes/polish-invoice-builder/specs/`.
  - In particular: enter the reference invoice values with a Tax number, pick dates by calendar and by keyboard, switch through all three sender layouts while watching the raster preview, use full screen (zoom, paging, export), and export a PDF.
  - Then relaunch and confirm the tax fields and layout persisted.
  - Also confirm that an identity file copied from the pre-change build loads.
  - Verify: each scenario is reported as passed, failed, or undetermined with screenshot evidence, and the exported PDF opened in Preview.app matches the in-app preview.

## Workflow follow-up

- `add-invoice-builder` must be completed (its task 10.1 is open) and archived before this change is archived: this change's MODIFIED deltas target requirements that only exist once `add-invoice-builder` is merged into `openspec/specs/`.
- Archive this change after review.

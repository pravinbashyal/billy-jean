# Proposal

## Why

The first invoice builder (`add-invoice-builder`) covers every field, but it does not feel finished. Dates are free-text boxes. The preview is a separate GPUI layout, so it shares content with the exported file but not its look. The PDF's line-items table has zero cell padding. The sender is one 14 pt `Name | Street | Postal City, Country` line that reads as the page headline. And the invoice has no tax number, which German invoice rules (§14 Abs. 4 UStG) require even on §19 small-business invoices. The Product Owner asked to polish all of this: a real date picker, a preview that *is* the PDF, a full-screen preview, a padded and more beautiful PDF, and a better sender treatment.

## What Changes

- **Dates become calendar pickers.** The Invoice, Service, and Due date fields become gpui-component `DatePicker`s: calendar only, no typed entry, weeks start on Monday, and a clear control. A small keyboard wrapper lets you move the date with the arrow and Page keys. Picks still write `DD/MM/YYYY` strings, so the model and the export gate's date checks don't change. **BREAKING** (spec): this reverses `add-invoice-builder` design D6 ("no calendar picker this version").
- **The preview is the PDF.** On each debounce the app renders the same `render_projection` bytes that Export PDF writes, then rasterizes them in-process with the pure-Rust `hayro` crate. The pages appear as A4 sheets on a neutral canvas. The last good render stays visible while a new one is prepared or when one fails. Multi-page invoices stack with "Page n of N" captions. The old GPUI-drawn preview layout is removed.
- **Full-screen preview.** A new in-window overlay opens from a toolbar button or Cmd+Shift+F. Its toolbar has Close, zoom out and zoom in, the zoom %, Fit width, "Page n of N", and Export PDF. Esc, Cmd+Shift+F, or Close exits.
- **PDF visual redesign, following the source invoice.**
  - Margins grow to 20 mm at the sides and 18 mm at the top.
  - A type scale with muted labels.
  - "Billed to" sits on the left with the 2x2 invoice-meta grid on the right, so no metadata line reads `Label: value`.
  - The line-items table gets cell padding, no vertical grid, a cyan (~`#00B8D9`) rule under the header, and light hairlines between rows.
  - The totals block is right-aligned and ends with a bold Gross row, keeping the "Total amount (Net)/(Gross)" wording.
  - Notes appear only when there is something to show.
  - The bank details become a muted footer.
  - No wave graphic, no top band, and no document title.
- **Bold and SemiBold Noto Sans are bundled** for both the PDF and the UI. Today "bold" in the PDF falls back to Regular, and the UI's semibold weight has no matching face.
- **Sender identity gains Tax number (required to export), VAT ID (optional), and Email (optional).** Saved identity files from the earlier version still load unchanged (serde defaults, `schema_version` stays 1).
- **Three user-selectable sender layouts**, set by a new "Sender layout" control in the Sender panel and saved with the identity. The choice drives the preview and the PDF alike.
  - **A, German letter (default):** a sender block at the top right, plus a small return line directly above the client.
  - **B, Bold header:** a bold name at the top left, the address and tax line below it, then a hairline.
  - **C, Footer columns:** a small return line at the top, with the sender, tax/contact, and bank details in three footer columns.
  - **BREAKING** (spec): the `Name | Street | Postal City, Country` header rule is replaced. The return line becomes `Name · Street · Postal City`.
- **Beneficiary falls back to the sender Name** when the Beneficiary field is left empty. The field stays editable. This is a default still awaiting PO confirmation.
- **Editor polish.**
  - Theme colour tokens replace hard-coded hex values.
  - A consistent 4 px spacing grid and calmer panel titles.
  - Export PDF becomes the primary button.
  - The Invoice details fields sit in a 2x2 grid in PDF order.

## Capabilities

### New Capabilities
<!-- None. All behaviour belongs to the two capabilities introduced by add-invoice-builder. -->

### Modified Capabilities
- `invoice-editing`: changes four requirements and adds two.
  - Changed: "Editing sender identity" (new fields, the layout setting, and a return-line caption in place of the pipe header), "Editing invoice number and dates" (calendar pickers instead of free text), "Editing bank details with IBAN validation" (Beneficiary fallback), and "Persisting sender and bank identity between runs" (new fields, the layout setting, backward-compatible load).
  - Added: date-picker keyboard operation, and consistent editor styling.
- `pdf-export`: changes two requirements and adds four.
  - Changed: "Live preview of the current invoice" (the preview is the rasterized exported PDF) and "Export blocked by validation" (a missing Tax number blocks export).
  - Added: full-screen preview, PDF page layout and typography, the padded line-items table, and the three sender layouts.

Note: both capabilities exist only as deltas inside the unarchived `add-invoice-builder` change (`openspec/specs/` is empty). This change's MODIFIED deltas target those requirement names, so `add-invoice-builder` must be archived first.

## Impact

- **Code**:
  - `src/pdf.rs`: projection gains a structured sender, the return line, and the layout. The renderer is rewritten with a custom cell decorator and hairline element, plus the Bold font slots.
  - `src/model.rs`: new `Sender` fields and a `SenderLayout` enum.
  - `src/identity_store.rs`: serde defaults and an old-file fixture test.
  - `src/export_gate.rs`: a new missing-tax-number issue.
  - `src/app_state.rs`: new `Field` variants.
  - `src/editor.rs`: date pickers, the Sender panel, the raster preview, the full-screen overlay, styling, and the gpui-mcp nodes.
  - `src/main.rs`: registers the Bold and SemiBold faces.
  - A new raster module wraps `hayro`.
- **Dependencies**: adds `hayro` (0.9, Apache-2.0 OR MIT, pure Rust, CPU-only, needs rustc 1.92; this machine has 1.99.1). `chrono` is already in the lock file through gpui-component.
- **Assets**: adds `assets/fonts/NotoSans-Bold.ttf` and `NotoSans-SemiBold.ttf` (OFL 1.1, same family as the existing Regular).
- **Tests**: PDF text assertions that look for `Label: value` needles must change. The `view.preview == project(...)` debounce tests and the `preview.*` gpui-mcp nodes stay valid, because the projection is still published.
- **Unchanged**:
  - "No external tools" still holds, because rasterizing happens in-process.
  - The VAT label stays `VAT (n%)`.
  - The totals wording stays "Total amount (Net)/(Gross)".
  - EUR stays fixed.

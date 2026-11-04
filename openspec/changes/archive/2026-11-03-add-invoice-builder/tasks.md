# Tasks

## 1. Project setup

- [x] 1.1 Create the Cargo project and add dependencies (GPUI, `gpui-component`, `genpdf`/`printpdf`, a bundled font with § and € coverage); verify `cargo build` succeeds and a minimal GPUI window with `gpui_component::init(cx)` opens.

## 2. Invoice domain model and calculations

- [x] 2.1 Define the `Invoice`, `LineItem`, and identity (`Sender`, `Bank`) types and pure total functions (per-row Total = Qty x Rate, Net = sum of row totals, VAT = Net x rate, Gross = Net + VAT), all EUR; verify unit tests cover the Totals arithmetic contract for 0% and a non-zero rate including the reference row (1 x 200 EUR).
- [x] 2.2 Implement the VAT/§19-note rule as a pure function (0% -> §19 UStG note present; non-zero rate -> note replaced/hidden); verify unit tests assert the note is present at 0% and absent above 0%.
- [x] 2.3 Implement IBAN validation (format + ISO 7064 mod-97 checksum) and four-character-block grouping; verify unit tests pass the reference IBAN `DE89 3704 0044 0532 0130 00` and reject malformed/bad-checksum inputs, and that grouping renders in 4-char blocks.
- [x] 2.4 Implement DD/MM/YYYY date parse/validate helper; verify unit tests accept well-formed dates and reject malformed ones.

## 3. Persistence

- [x] 3.1 Implement the versioned identity store (Sender + Bank) saving to and loading from a per-user config dir, with a schema-version field and the content/identity separation seam from design D5; verify a unit test round-trips identity and that an absent/first-run file loads as empty without error.

## 4. PDF generation

- [x] 4.1 Implement the `genpdf` projection that renders all field groups (sender header `Name | Street | Postal City, Country`, billed-to block, invoice metadata, line-items table, Net/VAT/Gross totals, notes, bank block) from an `Invoice`, using only in-process code (no external process or network); verify a test generates a PDF to a temp path and that extracted text contains the sender, client name, each total, and the IBAN.

## 5. Window shell and simple form panels

- [x] 5.1 Build the single-window shell: two regions (left vertically scrolling form, right preview), panels ordered Sender, Bill to, Invoice details, Line items, Totals, Notes, Bank details, with Sender and Bank pre-filled from the identity store (empty on first run); verify launching shows all seven panels in order and pre-filled identity on a second run (Launching + Resuming scenarios).
- [x] 5.2 Wire the simple text panels — Sender (Name/Street/Postal City/Country), Bill to (same four), Invoice details (manual Invoice number + Invoice/Service/Due dates as free-text DD/MM/YYYY), Notes — two-way to the `Invoice` model; verify editing each field updates the model and the matching preview block, sender rendering as `Name | Street | Postal City, Country` (sender/bill-to/meta/notes scenarios).

## 6. Line items, totals, and VAT

- [x] 6.1 Build the Line items table with Add item and per-row Remove controls and a read-only Total cell = Qty x Rate; verify Add yields a new editable row, Remove drops it from table and preview, and the Total cell cannot be edited and recomputes on Qty/Rate change (Adding/Removing/Line-total scenarios).
- [x] 6.2 Build the Totals panel with a VAT-rate control (default 0%) driving Net/VAT/Gross recompute and the §19-note behavior from task 2.2; verify the VAT default scenario (0%, note shown, Gross = Net), the non-zero scenario (VAT/Gross recompute, note hidden), and recompute on any line change.

## 7. Bank details panel

- [x] 7.1 Build the Bank details panel (Bank, Beneficiary, IBAN) showing the IBAN in four-char blocks and an inline error beneath the IBAN field on invalid input (using task 2.3), with no hard-coded names; verify a valid IBAN shows grouped with no error and is retained as a default, and an invalid IBAN shows the inline error (bank scenarios).

- [x] 7.2 Wire save-on-edit of the identity (Sender + Bank) to `identity_store::save`, so edits in the Sender and Bank details panels persist to the per-user config dir and are reloaded on the next launch; do not persist an IBAN that fails validation (use `iban_error()`); verify a test that editing Sender and Bank fields writes the store and a reload restores them, and that an invalid IBAN is not written. (Added after 7.1 review: no UI path called `save`, so "retained as a default" and the 10.1 relaunch check were unmet.)

## 8. Live preview

- [x] 8.1 Wire the debounced live preview so the right region re-renders from current `Invoice` state after a short debounce on any edit, reusing the task 4.1 projection so preview and export match; verify the preview updates without a manual trigger after an edit settles (preview scenario).

## 9. Export

- [x] 9.1 Implement the export validation gate (blocks on no line items, missing invoice number, missing/malformed date, or invalid IBAN) with inline field errors and a summary message; verify export is blocked and the correct errors surface for each invalid case (blocked-from-export scenario).
- [x] 9.2 Implement Export PDF: on valid state open a native save dialog defaulting to a filename from client name + invoice number (e.g. `Ostrander Quennell invoice 4.pdf`) and the last-used folder, writing the PDF on confirm; verify a valid invoice writes a file at the chosen path whose content matches the preview projection (export scenario).

## 10. Integration verification

- [x] 10.1 End-to-end check: launch, enter the reference invoice values, confirm the live preview reflects them, export to a file, reopen that PDF and confirm it contains every field group, then relaunch and confirm Sender/Bank identity persisted; verify the full flow succeeds end to end.

## 11. Defect fixes

- [x] 11.1 Fix the invisible ("white on white" / unrendered) text defect — field labels and input text do not render in the running window although panels and input borders do. Root cause: the gpui-component theme's default `font_family` (`.SystemUIFont`) does not resolve in this `gpui-pre` / `runtime_shaders` build, so no glyphs are drawn; set the theme `font_family` to the bundled font family ("Noto Sans", registered in `main.rs`) right after `gpui_component::init(cx)`. Verify text renders visibly in the running app (not just that tests pass) — this task gates the 10.1 visual check and must land before it.

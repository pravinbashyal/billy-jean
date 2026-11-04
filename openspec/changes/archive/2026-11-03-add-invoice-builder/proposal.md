# Proposal

## Why

The user has a single flat PDF invoice (a German Kleinunternehmer / §19 UStG freelancer invoice) whose form fields are broken and whose fonts cannot be reused, so there is no way to regenerate it with new values. They want a native desktop application where every value on that invoice can be configured and a fresh PDF produced on demand. This first version targets the one invoice the user has today, but it is the seed of a reusable invoice tool, so the data model is designed to grow.

## What Changes

- A new GPUI (Zed's Rust GPU-accelerated GUI framework) single-window desktop app for macOS that edits one invoice and exports it to PDF.
- A left-hand scrolling form with panels for every field group on the source invoice — Sender, Bill to, Invoice details, Line items, Totals, Notes, Bank details — and a right-hand live PDF preview.
- A line-items table with add/remove rows and auto-calculated per-row and invoice totals (Net / VAT / Gross).
- A configurable VAT rate (default 0%): a non-zero rate recomputes VAT and Gross and replaces the standing §19 UStG note; 0% keeps the §19 note.
- Live, debounced preview that re-renders from current field values as the user edits.
- PDF generation entirely in pure Rust (genpdf/printpdf) bundled in the binary — **no external tools to install** (no Ghostscript, no LaTeX, no Typst). The output is a clean redesign that carries all the same fields rather than a pixel-match of the original.
- Export to a chosen file via a native save dialog, gated by validation (required fields present, at least one line item, valid IBAN).
- Persistence of the user's Sender and Bank identity between runs so they are pre-filled on next launch. Invoice number is typed manually in this version.
- The source invoice contains data typos (beneficiary `Penhallow Morgan`, line text `Gorham Bank`). These are editable field values, **not** hard-coded — the app ships no invoice content baked in.

Deliberate non-goals for this version (explicit extension points, see design.md): client address book, invoice history/storage of past invoices, auto-incrementing invoice number, multi-currency (EUR is fixed), and a calendar date-picker (dates are free-text DD/MM/YYYY). The persistence and data model are architected so these can be added later without reshaping the core.

## Capabilities

### New Capabilities
- `invoice-editing`: The editor surface and invoice data model — the form panels, line-items table with add/remove, auto-calculation of line and invoice totals, the configurable VAT rate and its effect on the §19 note, field validation, and persistence of Sender/Bank identity between runs.
- `pdf-export`: Turning the current invoice state into a PDF — the live debounced preview, the pure-Rust PDF generation, and the validated export-to-file flow with a native save dialog.

### Modified Capabilities
<!-- None. This is a greenfield project with no existing specs. -->

## Impact

- **New project**: greenfield Rust/GPUI app in `/Users/pravinbashyal/project/billing` (no app code exists yet). A Cargo project will be introduced during implementation.
- **Dependencies**: GPUI itself; a third-party GPUI component library — `gpui-component` (Longbridge) — because GPUI core ships no public text-input/table/form primitives (Zed's own input lives in unextracted GPL editor crates); and a pure-Rust PDF crate (genpdf on top of printpdf). No system-level tool installs.
- **Platform**: macOS desktop (the user's environment).
- **Source file**: `Ostrander Quennell  invoice .pdf` is used only as the reference for field inventory and default-value shape during design; the app does not read or depend on it at runtime.
- **Local state**: a small per-user config file (e.g. under the OS app-support/config dir) to persist Sender and Bank identity.

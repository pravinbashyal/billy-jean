# Spec Delta

## Purpose

Turns the current invoice state into a PDF — a live preview that re-renders as the user edits, generation performed entirely in-process with no external tools, and a validated export to a file the user chooses.

## ADDED Requirements

### Requirement: Live preview of the current invoice

The application SHALL render a preview of the invoice in the window's right region and re-render it automatically, debounced, as the user edits any field; a manual refresh affordance MAY be offered but SHALL NOT be required.

#### Scenario: [interaction] Preview updates automatically on edit
- **WHEN** the user edits any field in the left-hand form of the invoice editor window
- **THEN** the right-hand preview re-renders from the current field values after a short debounce, without the user having to trigger it manually

### Requirement: In-process PDF generation

The application SHALL generate the PDF entirely within its own binary using a pure-Rust engine, requiring no external tools (no Ghostscript, LaTeX, or Typst) and no network access, and the generated document SHALL carry all invoice field groups: sender header, billed-to block, invoice metadata, line items, totals, notes, and bank details.

#### Scenario: Generation uses no external tools
- **WHEN** the application generates a PDF from the current invoice state
- **THEN** it produces the PDF using only in-process code with no external process, binary, or network dependency
- **AND** the resulting PDF contains the sender header, billed-to block, invoice metadata, line items, totals, notes, and bank details

### Requirement: Export blocked by validation

The application SHALL block export when any required field is missing or invalid — no line items, missing invoice number, a missing or malformed date, or an invalid IBAN — marking each offending field with an inline error and showing a summary message naming what must be fixed.

#### Scenario: [interaction] Being blocked from export by validation
- **WHEN** the user clicks "Export PDF" in the invoice editor window while there are no line items, a missing invoice number, a missing or malformed date, or an invalid IBAN
- **THEN** export does not proceed, the offending fields are marked with inline errors, and a summary message names what must be fixed

### Requirement: Export the invoice to a chosen file

The application SHALL, when "Export PDF" is clicked and all required fields are valid, open a native save dialog defaulting to a filename derived from the client name and invoice number and to the last-used folder, and on confirmation write a PDF matching the preview to the chosen path.

#### Scenario: [interaction] Exporting the finished invoice to a PDF file
- **WHEN** the user clicks "Export PDF" in the invoice editor window with all required fields valid
- **THEN** a native save dialog opens with a default filename derived from the client name and invoice number (for example `Ostrander Quennell invoice 4.pdf`) and defaulting to the last-used folder
- **AND** on confirmation a PDF matching the preview is written to the chosen path

# pdf-export Specification

## Purpose
Turns the current invoice state into a PDF — a live preview that re-renders as the user edits, generation performed entirely in-process with no external tools, and a validated export to a file the user chooses.

## Requirements

### Requirement: Live preview of the current invoice

The application SHALL render a preview of the invoice in the window's right region and re-render it automatically, debounced, as the user edits any field; a manual refresh affordance MAY be offered but SHALL NOT be required. The preview SHALL show the page images of the very PDF document that Export PDF would write for the same invoice state — same layout, fonts, padding, colours, and page breaks — rendered in-process, drawn as A4 sheets on a neutral canvas and scaled to the column width.

#### Scenario: [interaction] Preview updates automatically on edit
- **WHEN** the user edits any field in the left-hand form of the invoice editor window
- **THEN** the right-hand preview re-renders from the current field values after a short debounce, without the user having to trigger it manually

#### Scenario: [interaction] Preview shows the exact exported page
- **WHEN** the user edits any field in the left form of the invoice editor window and the preview debounce elapses
- **THEN** the preview column (`preview-column`) shows the A4 page(s) of the PDF that Export PDF would write for that state, as white sheets with a soft shadow on the neutral canvas, scaled to fit the column width
- **AND** layout, fonts, padding, colours, the line-items table, and the totals block look the same as in the exported file

#### Scenario: [interaction] No flicker while re-rendering
- **WHEN** a preview refresh is in progress in the preview column (`preview-column`) of the invoice editor window
- **THEN** the previous page image stays visible until the new one is ready, so the preview never goes blank

#### Scenario: [interaction] Preview render failure
- **WHEN** rendering the preview fails in the preview column (`preview-column`) of the invoice editor window
- **THEN** the last good page image stays visible, a small muted notice "Preview could not be updated" appears above it, and the notice disappears after the next successful render

#### Scenario: [interaction] Multi-page invoice in the preview
- **WHEN** the invoice has enough line items to produce more than one PDF page and the user scrolls the preview column (`preview-column`) of the invoice editor window
- **THEN** pages are stacked vertically with a gap between them and a muted "Page n of N" caption under each

#### Scenario: Preview and export render the same document
- **WHEN** the preview has settled for a given invoice state and Export PDF is then confirmed for that same state
- **THEN** the written file and the document the preview was rasterized from are produced from the same projection by the same rendering code, so their page count and text content are identical

### Requirement: In-process PDF generation

The application SHALL generate the PDF entirely within its own binary using a pure-Rust engine, requiring no external tools (no Ghostscript, LaTeX, or Typst) and no network access, and the generated document SHALL carry all invoice field groups: sender header, billed-to block, invoice metadata, line items, totals, notes, and bank details.

#### Scenario: Generation uses no external tools
- **WHEN** the application generates a PDF from the current invoice state
- **THEN** it produces the PDF using only in-process code with no external process, binary, or network dependency
- **AND** the resulting PDF contains the sender header, billed-to block, invoice metadata, line items, totals, notes, and bank details

### Requirement: Export blocked by validation

The application SHALL block export when any required field is missing or invalid — no line items, missing invoice number, a missing or malformed date, a missing sender Tax number, or an invalid IBAN — marking each offending field with an inline error and showing a summary message naming what must be fixed.

#### Scenario: [interaction] Being blocked from export by validation
- **WHEN** the user clicks "Export PDF" in the invoice editor window while there are no line items, a missing invoice number, a missing or malformed date, a missing Tax number, or an invalid IBAN
- **THEN** export does not proceed, the offending fields are marked with inline errors, and a summary message names what must be fixed

#### Scenario: [interaction] Missing tax number blocks export
- **WHEN** the user clicks "Export PDF" (`export-pdf`) in the export bar of the invoice editor window with the Tax number field (`sender.tax_number`) in the Sender panel empty or whitespace-only
- **THEN** export does not proceed, the inline error "Enter your tax number (Steuernummer)." appears beneath the Tax number field, and the summary message lists "Tax number"

#### Scenario: [interaction] VAT ID and Email are not required
- **WHEN** the user clicks "Export PDF" (`export-pdf`) in the invoice editor window with every required field valid and the VAT ID and Email fields in the Sender panel empty
- **THEN** no inline error appears beneath VAT ID or Email and export proceeds to the save dialog

### Requirement: Export the invoice to a chosen file

The application SHALL, when "Export PDF" is clicked and all required fields are valid, open a native save dialog defaulting to a filename derived from the client name and invoice number and to the last-used folder, and on confirmation write a PDF matching the preview to the chosen path.

#### Scenario: [interaction] Exporting the finished invoice to a PDF file
- **WHEN** the user clicks "Export PDF" in the invoice editor window with all required fields valid
- **THEN** a native save dialog opens with a default filename derived from the client name and invoice number (for example `Ostrander Quennell invoice 4.pdf`) and defaulting to the last-used folder
- **AND** on confirmation a PDF matching the preview is written to the chosen path

### Requirement: Full-screen preview

The application SHALL offer a full-screen preview inside the editor window, entered from a "Full-screen preview" button at the top of the preview column or with Cmd+Shift+F, that hides the form and export bar and shows the preview pages with a toolbar of Close, zoom out, zoom percentage, zoom in, Fit width, a "Page n of N" indicator, and Export PDF; Escape, Cmd+Shift+F, or Close exits it.

#### Scenario: [interaction] Entering full-screen preview
- **WHEN** the user clicks the "Full-screen preview" icon button (`preview.fullscreen`) at the top-right of the preview column, or presses Cmd+Shift+F anywhere in the invoice editor window
- **THEN** the preview fills the whole window with the form and export bar hidden, and a slim toolbar shows Close, zoom out, the zoom percentage, zoom in, Fit width, "Page n of N", and Export PDF
- **AND** the page opens at Fit width

#### Scenario: [interaction] Exiting full-screen preview
- **WHEN** the user presses Escape, presses Cmd+Shift+F again, or clicks Close (`fullscreen.close`) in the full-screen preview toolbar of the invoice editor window
- **THEN** the editor layout returns with the form at the same scroll position and focus back on the field that had it before

#### Scenario: [interaction] Exporting from full-screen preview
- **WHEN** the user clicks Export PDF (`fullscreen.export-pdf`) in the full-screen preview toolbar of the invoice editor window
- **THEN** export runs through the same validation gate and save dialog as the export bar's "Export PDF"
- **AND** if validation blocks it, the full-screen preview closes so the inline errors and summary message are visible in the form

### Requirement: Full-screen preview zoom and paging

The full-screen preview SHALL zoom in steps of 50, 75, 100, 125, 150, 200, and 300 % (100 % = A4 at one point per logical pixel) and re-render the page sharply at the chosen scale rather than stretching it, return to Fit width on Cmd+0, scroll horizontally when the page is wider than the window, and move one page at a time with Page Down / Page Up.

#### Scenario: [interaction] Zooming the full-screen preview
- **WHEN** the user presses Cmd+= or Cmd+-, or clicks the zoom buttons (`fullscreen.zoom-in`, `fullscreen.zoom-out`) in the full-screen preview toolbar of the invoice editor window
- **THEN** the zoom steps through 50, 75, 100, 125, 150, 200, and 300 %, the toolbar shows the current percentage, and the page is re-rendered sharply at the new scale rather than stretched
- **AND** Cmd+0 or the Fit width button (`fullscreen.fit-width`) returns to Fit width, and when the page is wider than the window the canvas scrolls horizontally

#### Scenario: [interaction] Paging in full-screen preview
- **WHEN** the user presses Page Down / Page Up, or scrolls, in the full-screen preview of the invoice editor window
- **THEN** Page Down / Page Up moves the view by one page, and the toolbar's "Page n of N" shows the page that is mostly in view

### Requirement: Invoice page layout

The generated PDF SHALL lay out an A4 page with no document title, wave, or colour band: the sender treatment of the selected Sender layout; a "Billed to" block on the left with a 2x2 grid of labelled invoice metadata on the right; the line-items table; a right-aligned totals block; a "Notes" section only when there is a note to show; and the bank details as a muted footer below a full-width hairline.

#### Scenario: [interaction] Exported PDF has clear hierarchy
- **WHEN** the user exports a valid invoice via "Export PDF" (`export-pdf`) in the invoice editor window and opens the file
- **THEN** the page shows, below the sender treatment: on the left a muted "Billed to" label over the client name in bold, larger type and then the client address; on the right a 2x2 grid of muted labels over bold values (Invoice date and Service date on the first row, Invoice number and Due date on the second); the padded line-items table; a right-aligned totals block ending in a bold "Total amount (Gross)" row; a muted "Notes" label over the §19 note and/or custom notes; and the bank details (Bank, Beneficiary, IBAN in four-character blocks) in small muted text below a full-width hairline
- **AND** no invoice metadata is printed as `Label: value` on one line

#### Scenario: [interaction] Notes section only when there is something to show
- **WHEN** the VAT rate in the Totals panel of the invoice editor window is above 0 % and the Notes field in the Notes panel is empty
- **THEN** neither the exported PDF nor the preview shows a "Notes" label

#### Scenario: Totals keep the source wording
- **WHEN** the application generates a PDF for any invoice
- **THEN** the totals block labels its rows "Total amount (Net)", "VAT (n%)" with the selected rate, and "Total amount (Gross)", with labels left-aligned and amounts right-aligned within the block

### Requirement: Invoice typography and colour

The generated PDF SHALL use the bundled Noto Sans family with real bold glyphs wherever bold is specified, a single accent colour (cyan, about `#00B8D9`) used only for the rule under the line-items header, near-black ink for values and body text, and a muted grey for labels, the sender address lines, the return line, and the bank footer.

#### Scenario: Bold text uses a bold face
- **WHEN** the application generates a PDF for a valid invoice
- **THEN** the document embeds a bold Noto Sans face in addition to the regular face, and the client name, metadata values, and the "Total amount (Gross)" row are set in that bold face

#### Scenario: Accent colour is limited to the table header rule
- **WHEN** the application generates a PDF for a valid invoice
- **THEN** the cyan accent appears only in the rule beneath the line-items header row, and all other text and rules are in ink, muted grey, or light grey

### Requirement: Padded line-items table

The generated PDF's line-items table SHALL give every cell visible padding, draw no vertical lines or outer frame, show a muted header row (Description, Quantity, Rate, Total; numeric columns right-aligned) over an accent rule, separate body rows with light hairlines, close the last row with a slightly darker rule, give Description the widest column, and keep a wrapped description within its row's padding.

#### Scenario: [interaction] Line-items table has breathing room in the exported PDF
- **WHEN** the user exports an invoice via "Export PDF" (`export-pdf`) in the invoice editor window and opens the PDF
- **THEN** each line-items cell has visible space between its text and the rules around it (about 2.5 mm above and below, about 2 mm between columns, with the first and last columns flush with the page margins), with no vertical grid lines, a cyan rule under the header row, and light hairlines between rows
- **AND** a description that wraps onto several lines stays inside its row padding, with the numeric cells right-aligned on the row's first line

### Requirement: Selectable sender layouts

The generated PDF (and therefore the preview) SHALL render the sender according to the Sender layout chosen in the Sender panel. German letter: a right-aligned sender block at the top right plus a small return line directly above "Billed to". Bold header: a bold name at the top left with address and tax line below, then a hairline. Footer columns: a small return line at the top and the sender, tax/contact, and bank details in three footer columns.

#### Scenario: [interaction] German letter layout
- **WHEN** the Sender layout control (`sender.layout`) in the Sender panel of the invoice editor window is "German letter", Name, Street, Postal code/City, Country, and Tax number are filled, and the user exports via "Export PDF" (`export-pdf`)
- **THEN** the top right of the PDF shows a right-aligned sender block: the name in bold, then the street, postal code/city, and country each on its own line in smaller muted text, then "Tax number" followed by its value, then the VAT ID and Email lines when filled
- **AND** a single small muted return line `Name · Street · Postal City` with a hairline beneath it sits directly above the "Billed to" label, clearly smaller than the client name
- **AND** the bank details form the muted footer

#### Scenario: [interaction] Bold header layout
- **WHEN** the Sender layout control (`sender.layout`) in the Sender panel of the invoice editor window is "Bold header" and the user exports via "Export PDF" (`export-pdf`)
- **THEN** the top left of the PDF shows the sender name in large bold type, then the street, then "Postal City, Country" on one line in muted text, then one muted line joining "Tax number <value>", "VAT ID <value>" (when filled) and the email (when filled) with ` · `, followed by a full-width hairline
- **AND** no return line is printed, and the bank details form the muted footer

#### Scenario: [interaction] Footer columns layout
- **WHEN** the Sender layout control (`sender.layout`) in the Sender panel of the invoice editor window is "Footer columns" and the user exports via "Export PDF" (`export-pdf`)
- **THEN** the top of the PDF shows only the small muted return line `Name · Street · Postal City` above the "Billed to" block and the invoice-meta grid
- **AND** after a full-width hairline at the end of the content, three equal muted columns show: the sender (name in bold, street, postal code/city, country); tax and contact (Tax number, VAT ID when filled, Email when filled); and bank (Bank, Beneficiary, IBAN in four-character blocks)

#### Scenario: [interaction] Preview sender treatment matches the PDF
- **WHEN** the user edits any Sender panel field or the Sender layout control in the invoice editor window and the preview debounce elapses
- **THEN** the preview column shows the same sender treatment, order, wrapping, and emphasis as the exported PDF for that layout

#### Scenario: Return line content
- **WHEN** the sender has Name `Alexandra Sharma`, Street `Musterstraße 12`, Postal code/City `10115 Musterstadt`, and Country `Germany`
- **THEN** the return line reads exactly `Alexandra Sharma · Musterstraße 12 · 10115 Musterstadt`, and an empty segment is skipped without leaving a stray separator

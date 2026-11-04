# Spec Delta

## MODIFIED Requirements

### Requirement: Editing sender identity

The editor SHALL let the user edit the sender's Name, Street, Postal code/City, Country, Tax number, VAT ID, and Email in the Sender panel, and choose a Sender layout (German letter, Bold header, or Footer columns; default German letter) that drives both the preview and the exported PDF. The Sender panel SHALL show the sender's return line `Name · Street · Postal City` (Country omitted, empty segments skipped) as a muted caption in place of the former `Name | Street | Postal City, Country` header line.

#### Scenario: [interaction] Editing a saved sender detail
- **WHEN** the user edits Name, Street, Postal code/City, Country, Tax number, VAT ID, or Email in the Sender panel of the invoice editor window (fields `sender.name`, `sender.street`, `sender.postal_code_city`, `sender.country`, `sender.tax_number`, `sender.vat_id`, `sender.email`)
- **THEN** after the preview debounce the preview shows the edited value in the sender treatment of the selected Sender layout
- **AND** the new value is retained as the default sender for the next launch

#### Scenario: [interaction] Return-line caption replaces the pipe header
- **WHEN** the user views the Sender panel of the invoice editor window with Name `Alexandra Sharma`, Street `Musterstraße 12`, Postal code/City `10115 Musterstadt`, and Country `Germany`
- **THEN** a small muted caption beneath the fields reads `Alexandra Sharma · Musterstraße 12 · 10115 Musterstadt`
- **AND** no `Rendered header: Name | Street | …` line is shown

#### Scenario: [interaction] Choosing a sender layout
- **WHEN** the user changes the "Sender layout" control (`sender.layout`) in the Sender panel of the invoice editor window from "German letter" to "Bold header" or "Footer columns"
- **THEN** after the preview debounce the preview shows the chosen layout, and a subsequent Export PDF writes that same layout
- **AND** the chosen layout is pre-selected the next time the invoice editor window is opened

#### Scenario: [interaction] Optional sender details appear only when filled
- **WHEN** the VAT ID and Email fields in the Sender panel of the invoice editor window are empty
- **THEN** neither the preview nor the exported PDF shows a "VAT ID" or email line, and no gap is left where they would be
- **AND** filling them in makes each appear as its own line beneath the Tax number after the preview debounce

### Requirement: Editing invoice number and dates

The editor SHALL let the user set an Invoice number and the Invoice date, Service date, and Due date in the Invoice details panel. The invoice number is typed manually. Each date is chosen from a popover calendar (no typed entry), whose weeks start on Monday and whose choice is stored and displayed as `DD/MM/YYYY`; each date field offers a clear control and shows a muted `DD/MM/YYYY` placeholder when empty.

#### Scenario: [interaction] Entering invoice numbers and dates
- **WHEN** the user sets the Invoice number, Invoice date, Service date, and Due date fields in the Invoice details panel of the invoice editor window
- **THEN** the preview's invoice-meta block shows the invoice number and the three dates formatted DD/MM/YYYY

#### Scenario: [interaction] Picking an invoice date from the calendar
- **WHEN** the user clicks the Invoice date field (`invoice.date`) in the Invoice details panel of the invoice editor window and clicks 3 October 2026 in the popover calendar
- **THEN** the popover closes and the field shows `03/10/2026`
- **AND** after the preview debounce, the preview's invoice-meta block shows Invoice date `03/10/2026`

#### Scenario: [interaction] Calendar opens on the relevant month
- **WHEN** the user opens the calendar of the Due date field (`invoice.due_date`) in the Invoice details panel of the invoice editor window
- **THEN** if the field holds a date, the calendar shows that month with that day selected; otherwise it shows the current month with today marked
- **AND** the calendar's weeks start on Monday

#### Scenario: [interaction] Dates cannot be typed
- **WHEN** the user focuses the Service date field (`invoice.service_date`) in the Invoice details panel of the invoice editor window and types `11/09/2026`
- **THEN** the typed characters are not inserted and the field's value is unchanged

#### Scenario: [interaction] Empty date field
- **WHEN** a date field in the Invoice details panel of the invoice editor window is empty
- **THEN** it shows a muted `DD/MM/YYYY` placeholder, and no error until Export PDF has been clicked

#### Scenario: [interaction] Clearing a date
- **WHEN** the user clicks the clear control inside a filled Due date field (`invoice.due_date`) in the Invoice details panel of the invoice editor window
- **THEN** the field empties and the preview shows an empty Due date after the debounce
- **AND** if export has already been attempted, the inline error "Enter a date in DD/MM/YYYY format." appears beneath the field

### Requirement: Editing bank details with IBAN validation

The editor SHALL let the user edit Bank, Beneficiary, and IBAN in the Bank details panel, display the IBAN grouped in four-character blocks, and show an inline error beneath the IBAN field when it fails a basic IBAN format and checksum check. All source values, including the names as spelled on the source invoice, are editable and never hard-coded. When Beneficiary is left empty, the sender Name SHALL stand in for it in the preview and the PDF, and SHALL be shown as the Beneficiary field's placeholder.

#### Scenario: [interaction] Editing valid bank details
- **WHEN** the user edits the Bank, Beneficiary, or IBAN fields in the Bank details panel with a valid IBAN
- **THEN** the preview's bank block updates, the IBAN is shown grouped in four-character blocks, and no error is shown
- **AND** the bank details are retained as defaults for the next launch

#### Scenario: [interaction] Seeing an inline IBAN error
- **WHEN** the user enters an IBAN in the Bank details panel that fails the format or checksum check
- **THEN** an inline error message appears beneath the IBAN field and the bank details are not retained as valid defaults

#### Scenario: [interaction] Beneficiary falls back to the sender name
- **WHEN** the Beneficiary field (`bank.beneficiary`) in the Bank details panel of the invoice editor window is empty and the Sender panel's Name is `Alexandra Sharma`
- **THEN** the Beneficiary field shows `Alexandra Sharma` as a muted placeholder, and the preview and exported PDF show `Alexandra Sharma` as the beneficiary
- **AND** typing a different Beneficiary replaces the fallback in the preview after the debounce

### Requirement: Persisting sender and bank identity between runs

The application SHALL persist the Sender (including Tax number, VAT ID, Email, and the chosen Sender layout) and Bank details identity in local per-user storage and restore it when the editor window is reopened in a later session. Identity saved by the earlier version of the application, which has no tax, contact, or layout values, SHALL still load: the missing values load as empty and the layout as German letter.

#### Scenario: [interaction] Resuming with values preserved between runs
- **WHEN** the user reopens the invoice editor window after a prior session in which valid Sender and Bank details, a Tax number, VAT ID, Email, and a Sender layout were entered
- **THEN** the Sender and Bank details panels are repopulated with the saved values and the saved Sender layout is selected

#### Scenario: Identity saved by the earlier version still loads
- **WHEN** the application loads a saved identity that contains only Name, Street, Postal code/City, Country, Bank, Beneficiary, and IBAN under the existing schema version
- **THEN** loading succeeds with those values unchanged, Tax number, VAT ID, and Email empty, and the Sender layout set to German letter

## ADDED Requirements

### Requirement: Keyboard operation of date fields

Each date field in the Invoice details panel SHALL be fully operable without a mouse: it is reachable by Tab, Enter opens and closes its calendar, Escape closes the calendar without changing the value, Delete or Backspace clears it, and while it is focused the arrow and Page keys move the date (Left/Right one day, Up/Down one week, Page Up/Page Down one month, with Shift one year), starting from today when the field is empty.

#### Scenario: [interaction] Opening and dismissing the calendar from the keyboard
- **WHEN** the user tabs into the Invoice date field (`invoice.date`) in the Invoice details panel of the invoice editor window and presses Enter
- **THEN** the popover calendar opens
- **AND** pressing Escape closes it and leaves the field's value unchanged

#### Scenario: [interaction] Moving a date with the keyboard
- **WHEN** the Due date field (`invoice.due_date`) in the Invoice details panel of the invoice editor window is focused and holds `03/10/2026`, and the user presses Right, then Down, then Page Down
- **THEN** the field shows `04/10/2026`, then `11/10/2026`, then `11/11/2026`, and, if the calendar was open, it closes (gpui-component 0.9 cannot reopen it programmatically; PO waiver) and the next open shows the value's month
- **AND** after the preview debounce the preview's Due date shows `11/11/2026`

#### Scenario: [interaction] Keyboard entry into an empty date field
- **WHEN** the Service date field (`invoice.service_date`) in the Invoice details panel of the invoice editor window is empty and focused, and the user presses Right
- **THEN** the field takes today's date

### Requirement: Consistent editor styling

The editor SHALL style every panel with the same padding, title style, and muted label style on a 4 px spacing grid, show errors in the theme's danger colour and the export success message in its success colour, render "Export PDF" as the only primary-styled button in the export bar, and lay out the Invoice details panel as a 2x2 grid in the order Invoice number, Invoice date / Service date, Due date.

#### Scenario: [interaction] Editor panels read as a calm, consistent form
- **WHEN** the user opens the invoice editor window
- **THEN** every panel uses the same padding, title style, and muted label style; errors appear in the theme's danger colour and the success message in its success colour; and "Export PDF" (`export-pdf`) is the only primary-styled button in the export bar

#### Scenario: [interaction] Invoice details laid out as a 2x2 grid
- **WHEN** the user views the Invoice details panel of the invoice editor window
- **THEN** the fields are arranged in two rows of two: Invoice number and Invoice date on the first row, Service date and Due date on the second
- **AND** the date field labels read "Invoice date", "Service date", and "Due date" without a format suffix

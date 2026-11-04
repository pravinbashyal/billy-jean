# Spec Delta

## Purpose

Lets a person configure every value of an invoice through a form editor — sender, client, invoice metadata, line items, VAT, notes, and bank details — with auto-calculated totals and identity that is remembered between sessions.

## ADDED Requirements

### Requirement: Invoice editor window layout

The application SHALL present a single-window editor whose left region is a vertically scrolling form divided into labelled panels in the order Sender, Bill to, Invoice details, Line items, Totals, Notes, Bank details, and whose right region shows a preview of the current invoice.

#### Scenario: [interaction] Launching into the invoice editor window
- **WHEN** the user opens the application and the invoice editor window appears
- **THEN** the left form shows the Sender, Bill to, Invoice details, Line items, Totals, Notes, and Bank details panels in that order
- **AND** the Sender and Bank details panels are pre-filled with previously saved issuer/bank values, or are empty on the first ever run
- **AND** the right region shows a preview of the current invoice

### Requirement: Editing sender identity

The editor SHALL let the user edit the sender's Name, Street, Postal code/City, and Country in the Sender panel, and SHALL render the sender in the preview header as `Name | Street | Postal City, Country`.

#### Scenario: [interaction] Editing a saved sender detail
- **WHEN** the user edits Name, Street, Postal code/City, or Country in the Sender panel of the invoice editor window
- **THEN** the preview header line updates to the rendered form `Name | Street | Postal City, Country`
- **AND** the new value is retained as the default sender for the next launch

### Requirement: Editing the billed-to client

The editor SHALL let the user enter the client's Name, Street, Postal code/City, and Country in the Bill to panel, reflected in the preview's "Billed to" block.

#### Scenario: [interaction] Filling in the client being billed
- **WHEN** the user types into the Name, Street, Postal code/City, or Country fields of the Bill to panel in the invoice editor window
- **THEN** the "Billed to" block of the preview updates to show those lines

### Requirement: Editing invoice number and dates

The editor SHALL let the user set an Invoice number and the Invoice date, Service date, and Due date in the Invoice details panel, where each date is a free-text field in DD/MM/YYYY format and the invoice number is typed manually.

#### Scenario: [interaction] Entering invoice numbers and dates
- **WHEN** the user sets the Invoice number, Invoice date, Service date, and Due date fields in the Invoice details panel of the invoice editor window
- **THEN** the preview's invoice-meta block shows the invoice number and the three dates formatted DD/MM/YYYY

### Requirement: Managing line items

The editor SHALL let the user add and remove rows in the Line items table, where each row has editable Description, Quantity, and Rate fields, a read-only Total cell, and a Remove control.

#### Scenario: [interaction] Adding a line item
- **WHEN** the user clicks "Add item" in the Line items panel of the invoice editor window
- **THEN** a new empty row appears with editable Description, Quantity, and Rate fields, a read-only Total cell, and a Remove control

#### Scenario: [interaction] Removing a line item
- **WHEN** the user clicks the Remove control on a row in the Line items panel
- **THEN** that row disappears from the table and from the preview
- **AND** the Net, VAT, and Gross totals recompute

### Requirement: Auto-calculating a line total

The editor SHALL compute each row's Total as Quantity multiplied by Rate, display it as a read-only EUR amount, and prevent direct editing of the Total cell.

#### Scenario: [interaction] Auto-calculating a line total
- **WHEN** the user enters or changes the Quantity or Rate of a row in the Line items panel
- **THEN** that row's Total cell updates to Quantity x Rate shown as a read-only EUR amount
- **AND** the Total cell cannot be edited directly

### Requirement: Computing invoice totals

The editor SHALL compute the Net as the sum of all row totals, the VAT amount as Net multiplied by the selected VAT rate, and the Gross as Net plus VAT amount, all in EUR, and show them in the Totals panel and the preview.

#### Scenario: [interaction] Recomputing invoice totals
- **WHEN** the user changes any Quantity, Rate, or line item in the Line items panel
- **THEN** the Totals panel and the preview show Net = sum of row totals, VAT amount = Net x the selected VAT rate, and Gross = Net + VAT amount, all in EUR

#### Scenario: Totals arithmetic contract
- **WHEN** the invoice has rows with totals summing to a Net value N and the selected VAT rate is R
- **THEN** the computed VAT amount MUST equal N x R and the computed Gross MUST equal N + (N x R)

### Requirement: Configurable VAT rate and §19 note

The editor SHALL provide a VAT-rate control defaulting to 0%; at 0% the VAT amount is 0 EUR, Gross equals Net, and the Notes default contains "According to § 19 UStG no VAT is charged"; a non-zero rate recomputes VAT and Gross and replaces/hides the §19 UStG note, because a non-zero rate alongside that note is contradictory.

#### Scenario: [interaction] Keeping the Kleinunternehmer VAT default
- **WHEN** the user views the Totals panel without changing the VAT rate in the invoice editor window
- **THEN** the VAT rate is 0%, the VAT amount is 0 EUR, and Gross equals Net
- **AND** the Notes panel contains the default "According to § 19 UStG no VAT is charged"

#### Scenario: [interaction] Switching to a non-zero VAT rate
- **WHEN** the user sets the VAT-rate control in the Totals panel above 0%
- **THEN** the VAT amount and Gross recompute and the preview's VAT line shows the chosen percentage
- **AND** the §19 UStG note is no longer shown as the standing note

### Requirement: Editing notes

The editor SHALL let the user edit free-form Notes text in the Notes panel, shown verbatim in the preview's Notes section.

#### Scenario: [interaction] Editing the notes text
- **WHEN** the user edits the text in the Notes panel of the invoice editor window
- **THEN** the preview's Notes section shows the edited text verbatim

### Requirement: Editing bank details with IBAN validation

The editor SHALL let the user edit Bank, Beneficiary, and IBAN in the Bank details panel, display the IBAN grouped in four-character blocks, and show an inline error beneath the IBAN field when it fails a basic IBAN format and checksum check. All source values, including the names as spelled on the source invoice, are editable and never hard-coded.

#### Scenario: [interaction] Editing valid bank details
- **WHEN** the user edits the Bank, Beneficiary, or IBAN fields in the Bank details panel with a valid IBAN
- **THEN** the preview's bank block updates, the IBAN is shown grouped in four-character blocks, and no error is shown
- **AND** the bank details are retained as defaults for the next launch

#### Scenario: [interaction] Seeing an inline IBAN error
- **WHEN** the user enters an IBAN in the Bank details panel that fails the format or checksum check
- **THEN** an inline error message appears beneath the IBAN field and the bank details are not retained as valid defaults

### Requirement: Persisting sender and bank identity between runs

The application SHALL persist the Sender and Bank details identity in local per-user storage and restore it when the editor window is reopened in a later session.

#### Scenario: [interaction] Resuming with values preserved between runs
- **WHEN** the user reopens the invoice editor window after a prior session in which valid Sender and Bank details were entered
- **THEN** the Sender and Bank details panels are repopulated with the saved values

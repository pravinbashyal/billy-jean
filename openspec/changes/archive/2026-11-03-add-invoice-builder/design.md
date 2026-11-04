# Design

## Context

See proposal.md - Why. Greenfield macOS project; the only existing artifact is the reference PDF, whose AcroForm fields are broken (`pdfinfo` errors `Can't get Fields array`) and whose embedded fonts cannot be reliably reused — so regeneration must re-typeset from scratch, it cannot fill the original. GPUI is Zed's Rust GPU-accelerated GUI framework; its core ships no public text-input, table, or form primitives (Zed's own input lives in unextracted GPL editor crates), which forces a component-library dependency for a form-heavy app. The PO confirmed a pure-Rust, zero-install PDF path and an architecture that can later grow into a fuller invoice manager.

## Goals / Non-Goals

**Goals:**
- A single-window GPUI app that edits one invoice with live debounced preview and exports a clean-redesign PDF with no external tools.
- A data model and persistence boundary shaped so a future invoice manager (address book, history, auto-numbering, multi-currency) can be added without reshaping the core.
- No invoice content hard-coded; every value (including the source's `Penhallow`/`Gorham` spellings) is editable field data.

**Non-Goals (this version):**
- Pixel-matching the original PDF (clean redesign only).
- Building the manager features now — only leaving clean seams for them.
- Reading or depending on the source PDF at runtime.

## Decisions

**D1 — GUI: GPUI + `gpui-component` (Longbridge).** GPUI core has no text input; `gpui-component` supplies `Input`/`InputState`, table, and form primitives and is initialized via `gpui_component::init(cx)`. Alternatives (`gpui-ui-kit`, `guise-ui`) are less complete. Trade-off: a third-party dependency, but unavoidable for "configure every value".

**D2 — PDF engine: pure-Rust `genpdf` on `printpdf`.** `genpdf` gives auto layout and text wrapping (needed for multi-line descriptions and notes) over the low-level `printpdf`. Bundled in the binary; no Ghostscript/LaTeX/Typst, no network. Alternatives rejected per PO decision #1: LaTeX (install burden), bundled Typst (heavier dep), low-level `printpdf` alone (manual layout). Fonts are substitutes, not the originals.

**D3 — Live debounced preview.** The pure-Rust engine is fast enough to regenerate on edit; the preview re-renders from current state after a short debounce (PO decision #4). The same generation code backs both preview and export, so the preview is a faithful proxy for the exported file. A manual refresh control is optional, not required.

**D4 — Invoice domain model as the single source of truth.** A plain Rust `Invoice` value type holds all fields — sender, client, metadata, `Vec<LineItem>`, VAT rate, notes, bank — with derived totals computed as pure functions (Net = sum of Qty x Rate, VAT = Net x rate, Gross = Net + VAT). The UI edits this model; preview and PDF are both projections of it. This keeps arithmetic and the VAT/§19-note rule testable without the GUI.

**D5 — Extension-point split of persisted state (PO decision #2).** Separate two concerns in storage so the manager can grow in later:
- *Identity* (Sender + Bank details) — persisted now, restored on launch.
- *Invoice content* (client, metadata, line items, VAT, notes) — not persisted across runs now.

```
  +-------------------+        +------------------------------+
  |  Identity store   |        |  (future) Invoice store      |
  |  sender, bank     |        |  clients[], invoices[],      |
  |  [persisted now]  |        |  next_number  [seam only]    |
  +---------+---------+        +---------------+--------------+
            |                                  |
            v                                  v
        +-------------------------------------------+
        |            Invoice (in-memory)            |
        |  identity + content + derived totals      |
        +-------------------------------------------+
                         |
             +-----------+-----------+
             v                       v
        [live preview]          [PDF export]
```

The config file is versioned (a schema version field) so a future migration to a manager store is additive. Invoice number stays a manual text field now; auto-increment is a future reader of the invoice store.

**D6 — EUR fixed; dates free-text DD/MM/YYYY.** *(Date-picker part SUPERSEDED by `polish-invoice-builder` design D2: dates become calendar-only pickers that still store DD/MM/YYYY strings. The EUR-fixed part stands.)* No currency selector and no calendar picker this version (PO decisions #5, #6). Dates are validated as well-formed DD/MM/YYYY strings at export; a currency field is a future per-invoice manager enhancement.

**D7 — IBAN validation.** Basic format + ISO 7064 mod-97 checksum check, surfaced as an inline field error; display groups the IBAN in four-character blocks. Does not require a bank-directory lookup.

## Risks / Trade-offs

- **`gpui-component` maturity / API churn** → pin a known-good version; isolate component usage behind thin wrapper widgets so a future swap is localized.
- **Clean redesign will not look like the original** → accepted per PO decision #1; the layout aims to be professional and carry every field, not to mimic.
- **Substitute fonts may render § and € glyphs differently** → choose a bundled font with full Latin + currency/section-sign coverage; verify the §19 note and EUR amounts render during implementation.
- **Live regeneration on every keystroke could stutter** → debounce (D3) and regenerate off the critical input path; if genpdf proves too slow for full regeneration, cache static chrome and re-render only changed regions.
- **Over-building the extension seams** → keep D5 to a versioned config shape and a clean `Invoice` model boundary; do not implement manager storage now.

## Open Questions

- Exact bundled font family (must cover § and €) — a finishing choice during implementation; does not affect specs, approach, or task breakdown.
- Whether the optional manual "Refresh preview" control is shown at all — cosmetic; the spec makes it optional.

//! In-process rasterizer for the preview (design D1). A thin wrapper over the
//! pure-Rust `hayro` crate: it turns the bytes that Export PDF writes into one
//! RGBA image per page. No `hayro` types appear in this module's public API.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::vello_cpu::peniko::ImageAlphaType;
use hayro::{PixmapSettings, RenderCache, RenderSettings};
use std::fmt;

/// One rendered page: straight (non-premultiplied) RGBA8, row-major, no padding.
/// `rgba.len()` is always `width * height * 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Why a PDF could not be rasterized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RasterError {
    /// The bytes are not a PDF that the parser accepts.
    Load(String),
    /// The page count or a page's geometry is unusable.
    Render(String),
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RasterError::Load(msg) => write!(f, "could not load PDF for rasterizing: {msg}"),
            RasterError::Render(msg) => write!(f, "could not rasterize PDF page: {msg}"),
        }
    }
}

impl std::error::Error for RasterError {}

/// Rasterizes every page of `pdf_bytes` at `px_per_pt` pixels per PDF point
/// (1 pt = 1/72 in). The background is opaque white, so the output is what a
/// reader would see on paper.
pub fn rasterize(pdf_bytes: &[u8], px_per_pt: f32) -> Result<Vec<PageImage>, RasterError> {
    if !(px_per_pt.is_finite() && px_per_pt > 0.0) {
        return Err(RasterError::Render(format!(
            "scale must be a positive number, got {px_per_pt}"
        )));
    }

    let pdf = Pdf::new(pdf_bytes.to_vec()).map_err(|err| RasterError::Load(format!("{err:?}")))?;

    // One cache per call: the glyph and outline caches are reused across the
    // pages of this document.
    let cache = RenderCache::new();
    let interpreter = InterpreterSettings::default();
    let render_settings = RenderSettings::default();
    let pixmap_settings = PixmapSettings {
        x_scale: px_per_pt,
        y_scale: px_per_pt,
        bg_color: WHITE,
    };

    let mut pages = Vec::new();
    for page in pdf.pages().iter() {
        let pixmap = hayro::render(
            page,
            &cache,
            &interpreter,
            &render_settings,
            &pixmap_settings,
        );
        let width = u32::from(pixmap.width());
        let height = u32::from(pixmap.height());
        if width == 0 || height == 0 {
            return Err(RasterError::Render(
                "page rendered to zero size".to_string(),
            ));
        }
        let rgba = pixmap.take_rgba8(ImageAlphaType::Alpha);
        pages.push(PageImage {
            width,
            height,
            rgba,
        });
    }

    if pages.is_empty() {
        return Err(RasterError::Render("PDF has no pages".to_string()));
    }
    Ok(pages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        Bank, Client, Invoice, InvoiceMeta, LineItem, Money, Quantity, Sender, VatRate,
    };
    use crate::pdf::render_invoice;

    fn invoice_with_items(count: usize) -> Invoice {
        Invoice {
            sender: Sender {
                name: "Alexandra Morgan".to_string(),
                street: "Whitfield. 42".to_string(),
                postal_code_city: "10115 Musterstadt".to_string(),
                country: "Germany".to_string(),
                ..Sender::default()
            },
            client: Client {
                name: "Ostrander Quennell".to_string(),
                street: "Rayburn Str. 42".to_string(),
                postal_code_city: "80331 Musterstadt".to_string(),
                country: "Germany".to_string(),
            },
            meta: InvoiceMeta {
                number: "4".to_string(),
                invoice_date: "03/10/2026".to_string(),
                service_date: "11/09/2026".to_string(),
                due_date: "15/10/2026".to_string(),
            },
            line_items: (0..count)
                .map(|i| LineItem {
                    description: format!(
                        "Workshop session {i} - Straße, Größe, Äpfel € § ä ß, a second line"
                    ),
                    quantity: Quantity::whole(1),
                    rate: Money::from_euros(200),
                })
                .collect(),
            vat_rate: VatRate::ZERO,
            notes: String::new(),
            bank: Bank {
                bank_name: "Ellery".to_string(),
                beneficiary: "Penhallow Morgan".to_string(),
                iban: "DE89370400440532013000".to_string(),
            },
        }
    }

    #[test]
    fn rasterize_returns_one_a4_image_per_page() {
        let bytes = render_invoice(&invoice_with_items(1)).expect("render");
        let pages = rasterize(&bytes, 1.5).expect("rasterize");

        assert_eq!(pages.len(), 1);
        let page = &pages[0];
        assert_eq!(page.rgba.len(), (page.width * page.height * 4) as usize);
        let aspect = page.height as f64 / page.width as f64;
        assert!(
            (aspect - std::f64::consts::SQRT_2).abs() < 0.01,
            "A4 aspect should be about 1:1.414, got {aspect}"
        );
    }

    #[test]
    fn rasterize_returns_one_image_per_page_for_multi_page_invoice() {
        let bytes = render_invoice(&invoice_with_items(50)).expect("render");
        let pages = rasterize(&bytes, 1.0).expect("rasterize");

        // 50 rows span 5 pages with the padded line-items table (task 3.2). They
        // spanned 3 before that change.
        assert_eq!(pages.len(), 5);
        for page in &pages {
            let aspect = page.height as f64 / page.width as f64;
            assert!((aspect - std::f64::consts::SQRT_2).abs() < 0.01);
        }
    }

    #[test]
    fn rasterize_rejects_bytes_that_are_not_a_pdf() {
        let err = rasterize(b"not a pdf", 1.5).expect_err("should fail");
        assert!(matches!(err, RasterError::Load(_)), "got {err:?}");
    }

    #[test]
    fn rasterize_rejects_non_positive_scale() {
        let bytes = render_invoice(&invoice_with_items(1)).expect("render");
        assert!(matches!(
            rasterize(&bytes, 0.0),
            Err(RasterError::Render(_))
        ));
        assert!(matches!(
            rasterize(&bytes, f32::NAN),
            Err(RasterError::Render(_))
        ));
    }
}

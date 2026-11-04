//! Spike for design D1: times genpdf + hayro on the reference invoice and
//! writes PNGs for a side-by-side check against `pdftoppm -r 144`.
//!
//! Ignored by default because it is a timing gate. Run it in release:
//!
//! ```text
//! cargo test --release --test raster_spike -- --ignored --nocapture
//! ```

use billing::model::{
    Bank, Client, Invoice, InvoiceMeta, LineItem, Money, Quantity, Sender, VatRate,
};
use billing::pdf::render_invoice;
use billing::raster::{PageImage, rasterize};
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Budget from design D1: one page at preview scale in 150 ms or less, and a
/// three-page invoice in under 400 ms.
const ONE_PAGE_BUDGET: Duration = Duration::from_millis(150);
const THREE_PAGE_BUDGET: Duration = Duration::from_millis(400);

/// Preview scale (1.3-2 px/pt range) used for the gate.
const PREVIEW_SCALE: f32 = 1.5;
/// Scale of the larger preview/full-screen case, reported but not gated.
const LARGE_SCALE: f32 = 3.0;
/// `pdftoppm -r 144` is 2 px/pt, the comparison scale for the PNGs.
const COMPARE_SCALE: f32 = 2.0;

const TIMED_RUNS: usize = 5;

fn reference_invoice(line_items: usize) -> Invoice {
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
        line_items: (0..line_items)
            .map(|i| {
                if i == 0 {
                    LineItem {
                        description: "Live musician performance - Tallis Vestry, Gorham Bank"
                            .to_string(),
                        quantity: Quantity::whole(1),
                        rate: Money::from_euros(200),
                    }
                } else {
                    LineItem {
                        description: format!(
                            "Workshop session {i} - Straße, Größe, Äpfel € § ä ß, a second line"
                        ),
                        quantity: Quantity::whole(2),
                        rate: Money::from_euros(75),
                    }
                }
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

fn output_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("output-test/spike");
    fs::create_dir_all(&dir).expect("create output-test/spike");
    dir
}

fn write_png(path: &PathBuf, page: &PageImage) {
    let file = File::create(path).expect("create png");
    let mut encoder = png::Encoder::new(BufWriter::new(file), page.width, page.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(&page.rgba).expect("png data");
}

/// Runs genpdf + hayro once per iteration and returns the median wall time.
/// One warm-up run is discarded so font loading and first-use costs are not
/// attributed to the steady state.
fn median_time(invoice: &Invoice, scale: f32) -> (Duration, usize) {
    let warm = render_invoice(invoice).expect("render");
    rasterize(&warm, scale).expect("rasterize");

    let mut samples = Vec::with_capacity(TIMED_RUNS);
    let mut pages = 0;
    for _ in 0..TIMED_RUNS {
        let start = Instant::now();
        let bytes = render_invoice(invoice).expect("render");
        let images = rasterize(&bytes, scale).expect("rasterize");
        samples.push(start.elapsed());
        pages = images.len();
    }
    samples.sort();
    (samples[samples.len() / 2], pages)
}

#[test]
#[ignore = "timing gate; run in release with --ignored --nocapture"]
fn spike_timing_gate_and_png_output() {
    let one_page = reference_invoice(1);
    let three_pages = reference_invoice(50);

    let (one_at_preview, one_pages) = median_time(&one_page, PREVIEW_SCALE);
    let (one_at_large, _) = median_time(&one_page, LARGE_SCALE);
    let (three_at_preview, three_pages_count) = median_time(&three_pages, PREVIEW_SCALE);
    let (three_at_large, _) = median_time(&three_pages, LARGE_SCALE);

    println!("spike timings (median of {TIMED_RUNS}, genpdf + hayro, release):");
    println!(
        "  1-page  @ {PREVIEW_SCALE} px/pt: {:>7.1} ms  ({one_pages} page)",
        one_at_preview.as_secs_f64() * 1000.0
    );
    println!(
        "  1-page  @ {LARGE_SCALE} px/pt: {:>7.1} ms  (not gated)",
        one_at_large.as_secs_f64() * 1000.0
    );
    println!(
        "  3-page  @ {PREVIEW_SCALE} px/pt: {:>7.1} ms  ({three_pages_count} pages)",
        three_at_preview.as_secs_f64() * 1000.0
    );
    println!(
        "  3-page  @ {LARGE_SCALE} px/pt: {:>7.1} ms  (not gated)",
        three_at_large.as_secs_f64() * 1000.0
    );

    let one_ok = one_at_preview <= ONE_PAGE_BUDGET;
    let three_ok = three_at_preview < THREE_PAGE_BUDGET;
    println!(
        "  gate (a) 1-page <= 150 ms: {}",
        if one_ok { "PASS" } else { "FAIL" }
    );
    println!(
        "  gate (a) 3-page < 400 ms:  {}",
        if three_ok { "PASS" } else { "FAIL" }
    );

    // PNG output for the side-by-side check (design D1 gate (b)).
    let dir = output_dir();
    let pdf_bytes = render_invoice(&one_page).expect("render");
    fs::write(dir.join("reference.pdf"), &pdf_bytes).expect("write pdf");
    let images = rasterize(&pdf_bytes, COMPARE_SCALE).expect("rasterize");
    assert_eq!(images.len(), 1);
    write_png(&dir.join("hayro-reference-144dpi.png"), &images[0]);
    // Three-page fixture with the umlaut, eszett, and euro/section characters.
    let three_pdf = render_invoice(&three_pages).expect("render");
    fs::write(dir.join("three-page.pdf"), &three_pdf).expect("write pdf");
    let three_images = rasterize(&three_pdf, COMPARE_SCALE).expect("rasterize");
    write_png(
        &dir.join("hayro-three-page-p1-144dpi.png"),
        &three_images[0],
    );
    println!(
        "  wrote {} and {}",
        dir.join("reference.pdf").display(),
        dir.join("hayro-reference-144dpi.png").display()
    );

    assert!(
        one_ok,
        "gate (a) failed: one page took {one_at_preview:?}, budget {ONE_PAGE_BUDGET:?}"
    );
    assert!(
        three_ok,
        "gate (a) failed: three pages took {three_at_preview:?}, budget {THREE_PAGE_BUDGET:?}"
    );
}

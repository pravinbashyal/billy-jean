//! The bundled Noto Sans faces (OFL 1.1, see `assets/fonts/OFL.txt`). This is the
//! single source of truth for the font bytes and the family name, shared by the
//! PDF renderer ([`crate::pdf`]) and the GPUI app (`main.rs`).

/// Regular face. Covers Latin plus the § (U+00A7) and € (U+20AC) glyphs.
pub const REGULAR: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
/// Bold face of the same Noto Sans release (design D3). Backs the PDF bold slots.
pub const BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");
/// SemiBold face, used by the UI only: genpdf has a single bold flag (design D3).
pub const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-SemiBold.ttf");

/// Family name stored in the bundled TTF's name table (nameID 1). The UI theme
/// must use this exact string so GPUI resolves the registered font.
pub const FAMILY: &str = "Noto Sans";

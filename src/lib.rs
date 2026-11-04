//! Core library for the invoice builder. Pure domain logic only: no GPUI,
//! no file I/O, no PDF code. Later UI, PDF, and persistence tasks depend on it.

pub mod app_state;
pub mod date;
pub mod export;
pub mod export_gate;
pub mod fonts;
pub mod identity_store;
pub mod iban;
pub mod model;
pub mod pdf;
pub mod raster;

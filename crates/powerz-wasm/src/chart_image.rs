//! The chart's Save image as PDF in the browser (FR EAKFEC): the desktop
//! app's writer ([`powerz_record::chart_image::pdf_from_png`]), so the page
//! downloads the same one-page PDF the desktop app saves, and refuses the
//! same pictures in the same words.

use powerz_record::chart_image;
use wasm_bindgen::prelude::*;

use crate::BindError;

/// The one-page PDF of the chart PNG `png`; a PNG this writer does not take
/// (or out of 320 to 7680 px) is refused as the desktop refuses it.
#[wasm_bindgen(js_name = chartPdfFromPng)]
pub fn chart_pdf_from_png(png: &[u8]) -> Result<Vec<u8>, BindError> {
    chart_image::pdf_from_png(png)
        .map_err(|e| BindError::new("usage", format!("The image was not saved: {e}.")))
}

/// The desktop's check of a chart PNG before it is saved.
#[wasm_bindgen(js_name = chartPngCheck)]
pub fn chart_png_check(png: &[u8]) -> Result<(), BindError> {
    chart_image::check_chart_image(chart_image::ChartImageFormat::Png, png)
        .map_err(|e| BindError::new("usage", format!("The image was not saved: {e}.")))
}

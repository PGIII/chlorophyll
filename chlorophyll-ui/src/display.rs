use core::fmt::Write;

use embedded_graphics::{
    geometry::Point,
    mono_font::{MonoTextStyle, ascii::FONT_10X20},
    pixelcolor::BinaryColor,
    prelude::*,
    text::Text,
};
use heapless::String as HeaplessString;

/// Pre-averaged sensor values for one display frame.
pub struct DisplayState {
    pub temperature_f: Option<f32>,
    pub humidity_pct: Option<f32>,
    pub lux: Option<f32>,
}

/// Draw one frame onto any `BinaryColor` [`DrawTarget`].
///
/// Fills the display white, then draws two rows of text:
/// - Row 1: temperature + humidity (or "No Temp Data")
/// - Row 2: lux (or "No Data lux")
pub fn render_frame<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    state: &DisplayState,
) -> Result<(), D::Error> {
    let font = FONT_10X20;
    let style = MonoTextStyle::new(&font, BinaryColor::Off);
    let row_h = font.character_size.height as i32;

    display.fill_solid(&display.bounding_box(), BinaryColor::On)?;

    let mut msg: HeaplessString<64> = HeaplessString::new();
    match (state.temperature_f, state.humidity_pct) {
        (Some(t), Some(h)) => {
            let _ = write!(msg, "{:.2}F {:.2}%", t, h);
        }
        _ => {
            let _ = write!(msg, "No Temp Data");
        }
    }
    Text::new(&msg, Point::new(5, row_h), style).draw(display)?;

    msg.clear();
    match state.lux {
        Some(l) => {
            let _ = write!(msg, "{:.2}lux", l);
        }
        None => {
            let _ = write!(msg, "No Data lux");
        }
    }
    Text::new(&msg, Point::new(5, row_h * 2), style).draw(display)?;

    Ok(())
}

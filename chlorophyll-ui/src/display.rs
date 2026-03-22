use core::fmt::Write;

use embedded_graphics::{
    geometry::Point,
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, PrimitiveStyleBuilder, Rectangle, Triangle},
};
use heapless::String as HeaplessString;
use u8g2_fonts::{
    FontRenderer,
    fonts,
    types::{FontColor, HorizontalAlignment, VerticalPosition},
};

/// Pre-averaged sensor values for one display frame.
pub struct DisplayState {
    pub temperature_f: Option<f32>,
    pub humidity_pct: Option<f32>,
    pub lux: Option<f32>,
}

// Row layout: 3 rows of 40px each within the 122px-tall display.
// Icons occupy the left 30px; text starts at x=35.
const ROW_HEIGHT: i32 = 40;
const ICON_WIDTH: i32 = 30;
const TEXT_X: i32 = 35;
const ROW_BASELINES: [i32; 3] = [35, 75, 115];

/// Draw one frame onto any `BinaryColor` [`DrawTarget`].
///
/// Three rows: temperature, humidity, lux — each with an icon on the left.
pub fn render_frame<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    state: &DisplayState,
) -> Result<(), D::Error> {
    display.fill_solid(&display.bounding_box(), BinaryColor::On)?;

    let font = FontRenderer::new::<fonts::u8g2_font_helvB24_tf>();
    let color = FontColor::Transparent(BinaryColor::Off);

    // Row 0 — temperature
    let row0_top = ROW_BASELINES[0] - ROW_HEIGHT + 2;
    draw_thermometer(display, Point::new(3, row0_top))?;
    let mut msg: HeaplessString<32> = HeaplessString::new();
    match state.temperature_f {
        Some(t) => { let _ = write!(msg, "{:.1}F", t); }
        None    => { let _ = write!(msg, "--F"); }
    }
    font.render_aligned(
        msg.as_str(),
        Point::new(TEXT_X, ROW_BASELINES[0]),
        VerticalPosition::Baseline,
        HorizontalAlignment::Left,
        color,
        display,
    ).ok();

    // Row 1 — humidity
    let row1_top = ROW_BASELINES[1] - ROW_HEIGHT + 2;
    draw_droplet(display, Point::new(3, row1_top))?;
    msg.clear();
    match state.humidity_pct {
        Some(h) => { let _ = write!(msg, "{:.1}%", h); }
        None    => { let _ = write!(msg, "--%"); }
    }
    font.render_aligned(
        msg.as_str(),
        Point::new(TEXT_X, ROW_BASELINES[1]),
        VerticalPosition::Baseline,
        HorizontalAlignment::Left,
        color,
        display,
    ).ok();

    // Row 2 — lux
    let row2_top = ROW_BASELINES[2] - ROW_HEIGHT + 2;
    draw_sun(display, Point::new(3, row2_top))?;
    msg.clear();
    match state.lux {
        Some(l) => { let _ = write!(msg, "{:.0}lx", l); }
        None    => { let _ = write!(msg, "--lx"); }
    }
    font.render_aligned(
        msg.as_str(),
        Point::new(TEXT_X, ROW_BASELINES[2]),
        VerticalPosition::Baseline,
        HorizontalAlignment::Left,
        color,
        display,
    ).ok();

    Ok(())
}

// ── Icons ─────────────────────────────────────────────────────────────────────

const FILLED: PrimitiveStyle<BinaryColor> =
    PrimitiveStyleBuilder::new().fill_color(BinaryColor::Off).build();
const STROKE1: PrimitiveStyle<BinaryColor> = PrimitiveStyleBuilder::new()
    .stroke_color(BinaryColor::Off)
    .stroke_width(1)
    .build();

/// Thermometer: thin tube (rect) + bulb (circle) at the bottom.
fn draw_thermometer<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    top_left: Point,
) -> Result<(), D::Error> {
    let x = top_left.x + (ICON_WIDTH / 2) - 3;
    let y = top_left.y;
    // Tube
    Rectangle::new(Point::new(x, y), Size::new(6, 22))
        .into_styled(STROKE1)
        .draw(display)?;
    // Fill indicator (bottom third of tube)
    Rectangle::new(Point::new(x + 1, y + 15), Size::new(4, 7))
        .into_styled(FILLED)
        .draw(display)?;
    // Bulb
    Circle::new(Point::new(x - 3, y + 20), 12)
        .into_styled(FILLED)
        .draw(display)?;
    Ok(())
}

/// Water droplet: filled teardrop (triangle tip-up + circle base).
fn draw_droplet<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    top_left: Point,
) -> Result<(), D::Error> {
    let cx = top_left.x + ICON_WIDTH / 2;
    let y = top_left.y;
    // Triangle pointing up
    Triangle::new(
        Point::new(cx, y),
        Point::new(cx - 8, y + 18),
        Point::new(cx + 8, y + 18),
    )
    .into_styled(FILLED)
    .draw(display)?;
    // Circle for the rounded base
    Circle::new(Point::new(cx - 8, y + 14), 16)
        .into_styled(FILLED)
        .draw(display)?;
    Ok(())
}

/// Sun: filled circle core + 8 short radiating lines.
fn draw_sun<D: DrawTarget<Color = BinaryColor>>(
    display: &mut D,
    top_left: Point,
) -> Result<(), D::Error> {
    let cx = top_left.x + ICON_WIDTH / 2;
    let cy = top_left.y + 16;
    // Core
    Circle::new(Point::new(cx - 6, cy - 6), 12)
        .into_styled(FILLED)
        .draw(display)?;
    // 8 rays: N, NE, E, SE, S, SW, W, NW (inner r=9, outer r=14)
    let rays: [(i32, i32, i32, i32); 8] = [
        (0, -9, 0, -14),    // N
        (6, -6, 10, -10),   // NE
        (9, 0, 14, 0),      // E
        (6, 6, 10, 10),     // SE
        (0, 9, 0, 14),      // S
        (-6, 6, -10, 10),   // SW
        (-9, 0, -14, 0),    // W
        (-6, -6, -10, -10), // NW
    ];
    for (dx0, dy0, dx1, dy1) in rays {
        Line::new(
            Point::new(cx + dx0, cy + dy0),
            Point::new(cx + dx1, cy + dy1),
        )
        .into_styled(STROKE1)
        .draw(display)?;
    }
    Ok(())
}

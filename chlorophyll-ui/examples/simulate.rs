use chlorophyll_ui::display::{DisplayState, SensorDisplay};
use chlorophyll_ui::displays::binary_250x122::Display250x122Binary;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

fn main() {
    // Display is 250 wide × 122 tall (rotated 270°: physical 122×250 → logical 250×122)
    let mut display = Display250x122Binary::new(
        SimulatorDisplay::<BinaryColor>::new(Size::new(250, 122)),
    );

    let state = DisplayState {
        temperature_f: Some(72.53),
        humidity_pct: Some(45.12),
        lux: Some(847.3),
        watchdog_reset: false,
    };

    display.render(&state).unwrap();

    let settings = OutputSettingsBuilder::new().build();
    display
        .inner
        .to_rgb_output_image(&settings)
        .save_png("simulate.png")
        .expect("failed to save PNG");

    println!("Saved simulate.png");
}

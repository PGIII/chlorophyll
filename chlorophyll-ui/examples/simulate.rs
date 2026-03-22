use chlorophyll_ui::display::{DisplayState, render_frame};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics_simulator::{OutputSettingsBuilder, SimulatorDisplay};

fn main() {
    // Display is 250 wide × 122 tall (rotated 270°: physical 122×250 → logical 250×122)
    let mut display = SimulatorDisplay::<BinaryColor>::new(Size::new(250, 122));

    let state = DisplayState {
        temperature_f: Some(72.53),
        humidity_pct: Some(45.12),
        lux: Some(847.3),
    };

    render_frame(&mut display, &state).unwrap();

    let settings = OutputSettingsBuilder::new().build();
    display
        .to_rgb_output_image(&settings)
        .save_png("simulate.png")
        .expect("failed to save PNG");

    println!("Saved simulate.png");
}

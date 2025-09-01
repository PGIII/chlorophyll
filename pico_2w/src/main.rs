//! This example tests the RP Pico 2 W onboard LED.
//!
//! It does not work with the RP Pico 2 board. See `blinky.rs`.

#![no_std]
#![no_main]

mod temp_humidity_sensor;

use core::fmt::Write;
use core::str::from_utf8;
use cyw43::JoinOptions;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::{self, info, println, unwrap, warn};
use embassy_executor::Spawner;
use embassy_net::StackResources;
use embassy_net::tcp::TcpSocket;
use embassy_rp::block::ImageDef;
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output};
use embassy_rp::peripherals::{DMA_CH0, I2C1, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::spi;
use embassy_rp::spi::{Blocking, Spi};
use embassy_rp::{
    bind_interrupts,
    i2c::{self, InterruptHandler as I2CInterruptHandler},
};
use embassy_time::{Delay, Duration, Timer};
use embedded_graphics::geometry::Point;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_5::{FONT_6X9, FONT_9X15_BOLD};
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_hal_bus::spi::ExclusiveDevice;
use embedded_io_async::Write as _;
use heapless::String;
use rand::RngCore;
use ssd1680::driver::Ssd1680;
use ssd1680::graphics::{Display, Display2in13, DisplayRotation};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

static_toml::static_toml! {
    static CONFIG = include_toml!("config.toml");
}

#[unsafe(link_section = ".start_block")]
#[used]
pub static IMAGE_DEF: ImageDef = ImageDef::secure_exe();

// Program metadata for `picotool info`.
// This isn't needed, but it's recommended to have these minimal entries.
#[unsafe(link_section = ".bi_entries")]
#[used]
pub static PICOTOOL_ENTRIES: [embassy_rp::binary_info::EntryAddr; 4] = [
    embassy_rp::binary_info::rp_program_name!(c"Blinky Example"),
    embassy_rp::binary_info::rp_program_description!(
        c"This example tests the RP Pico 2 W's onboard LED, connected to GPIO 0 of the cyw43 \
        (WiFi chip) via PIO 0 over the SPI bus."
    ),
    embassy_rp::binary_info::rp_cargo_version!(),
    embassy_rp::binary_info::rp_program_build_attribute!(),
];

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
    I2C1_IRQ => I2CInterruptHandler<I2C1>;
});

#[embassy_executor::task]
async fn cyw43_task(
    runner: cyw43::Runner<'static, Output<'static>, PioSpi<'static, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, cyw43::NetDriver<'static>>) -> ! {
    runner.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut rng = RoscRng;
    let fw = include_bytes!("../cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    // To make flashing faster for development, you may want to flash the firmwares independently
    // at hardcoded addresses, instead of baking them into the program with `include_bytes!`:
    //     probe-rs download ../../cyw43-firmware/43439A0.bin --binary-format bin --chip RP2040 --base-address 0x10100000
    //     probe-rs download ../../cyw43-firmware/43439A0_clm.bin --binary-format bin --chip RP2040 --base-address 0x10140000
    //let fw = unsafe { core::slice::from_raw_parts(0x10100000 as *const u8, 230321) };
    //let clm = unsafe { core::slice::from_raw_parts(0x10140000 as *const u8, 4752) };

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        RM2_CLOCK_DIVIDER,
        pio.irq0,
        cs,
        p.PIN_24,
        p.PIN_29,
        p.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    defmt::unwrap!(spawner.spawn(cyw43_task(runner)));

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let net_config = embassy_net::Config::dhcpv4(Default::default());
    let seed = rng.next_u64();

    info!("Setting up Network");
    static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();
    let (stack, runner) = embassy_net::new(
        net_device,
        net_config,
        RESOURCES.init(StackResources::new()),
        seed,
    );
    unwrap!(spawner.spawn(net_task(runner)));
    loop {
        match control
            .join(
                CONFIG.wifi.ssid,
                JoinOptions::new(CONFIG.wifi.password.as_bytes()),
            )
            .await
        {
            Ok(_) => break,
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
    }

    // Wait for DHCP, not necessary when using static IP
    info!("waiting for DHCP...");
    while !stack.is_config_up() {
        Timer::after_millis(100).await;
    }
    info!("DHCP is now up!");

    info!("Building display");
    let spi = p.SPI0;
    let rst = Output::new(p.PIN_0, Level::Low);
    let dc = Output::new(p.PIN_4, Level::Low);
    let busy = Input::new(p.PIN_1, embassy_rp::gpio::Pull::Up);
    let sclk = p.PIN_2;
    let mosi = p.PIN_3;
    let cs = Output::new(p.PIN_5, Level::High);

    let spi: Spi<'_, _, Blocking> =
        Spi::new_blocking_txonly(spi, sclk, mosi, spi::Config::default());

    let spi_device = ExclusiveDevice::new(spi, cs, Delay);
    let disp_interface = display_interface_spi::SPIInterface::new(spi_device, dc);
    let mut delay = Delay;
    let mut ssd1680 = Ssd1680::new(disp_interface, busy, rst, &mut delay).unwrap();
    ssd1680.clear_bw_frame().unwrap();
    let mut display_bw = Display2in13::bw();
    display_bw.set_rotation(DisplayRotation::Rotate90);
    println!("drawing display");
    // background fill
    display_bw
        .fill_solid(&display_bw.bounding_box(), BinaryColor::On)
        .unwrap();

    info!("set up i2c ");
    let sda = p.PIN_14;
    let scl = p.PIN_15;
    let i2c = i2c::I2c::new_async(p.I2C1, scl, sda, Irqs, i2c::Config::default());
    let timer = &mut Delay;
    let mut aht20_uninit = aht20_driver::AHT20::new(i2c, aht20_driver::SENSOR_ADDRESS);
    let mut aht20 = aht20_uninit.init(timer).unwrap();

    let delay = Duration::from_millis(5000);

    /*
    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut buf = [0; 4096];
    */

    loop {
        /*
        let mut socket = TcpSocket::new(stack, &mut rx_buffer, &mut tx_buffer);
        socket.set_timeout(Some(Duration::from_secs(10)));

        control.gpio_set(0, false).await;

        info!("Listening on TCP:1234...");
        if let Err(e) = socket.accept(1234).await {
            warn!("accept error: {:?}", e);
            continue;
        }
        info!("Received connection from {:?}", socket.remote_endpoint());
        control.gpio_set(0, true).await;

        loop {
            let n = match socket.read(&mut buf).await {
                Ok(0) => {
                    warn!("read EOF");
                    break;
                }
                Ok(n) => n,
                Err(e) => {
                    warn!("read error: {:?}", e);
                    break;
                }
            };

            info!("rxd {}", from_utf8(&buf[..n]).unwrap());

            match socket.write_all(&buf[..n]).await {
                Ok(()) => {}
                Err(e) => {
                    warn!("write error: {:?}", e);
                    break;
                }
            };
        }
        */

        control.gpio_set(0, true).await;
        Timer::after(delay).await;
        let measure = aht20.measure(timer).unwrap();
        let mut msg: String<200> = String::new();
        write!(
            msg,
            "{:.2}F {:.2}%",
            measure.temperature * 9.0 / 5.0 + 32.0,
            measure.humidity
        )
        .unwrap();
        display_bw
            .fill_solid(&display_bw.bounding_box(), BinaryColor::On)
            .unwrap();
        Text::new(
            &msg,
            Point::new(5, 10),
            MonoTextStyle::new(&FONT_9X15_BOLD, BinaryColor::Off),
        )
        .draw(&mut display_bw)
        .unwrap();
        ssd1680.update_bw_frame(display_bw.buffer()).unwrap();
        ssd1680.display_frame(timer).unwrap();
        control.gpio_set(0, false).await;
        Timer::after(delay).await;
    }
}

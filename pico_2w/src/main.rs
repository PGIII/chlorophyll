//! This example tests the RP Pico 2 W onboard LED.
//!
//! It does not work with the RP Pico 2 board. See `blinky.rs`.

#![no_std]
#![no_main]
extern crate alloc;

mod temp_humidity_sensor;

use chlorophyll_protocol::light::Light;
use chlorophyll_protocol::postcard::to_allocvec;
use chlorophyll_protocol::temperature::Temperature;
use chlorophyll_protocol::*;
use core::cell::RefCell;
use core::fmt::Write;
use core::net::Ipv4Addr;
use core::ops::{Add, AddAssign, Deref, Div};
use cyw43::JoinOptions;
use cyw43_pio::{PioSpi, RM2_CLOCK_DIVIDER};
use defmt::{info, unwrap, warn};
use embassy_embedded_hal::shared_bus::blocking::i2c::I2cDevice;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{IpAddress, Stack};
use embassy_net::{
    IpEndpoint, StackResources,
    udp::{PacketMetadata, UdpSocket},
};
use embassy_rp::gpio::{Input, Level, Output};
use embassy_rp::peripherals::{DMA_CH0, I2C1, PIO0, SPI0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::spi;
use embassy_rp::spi::{Async, Spi};
use embassy_rp::{
    bind_interrupts,
    i2c::{self, InterruptHandler as I2CInterruptHandler},
};
use embassy_rp::{block::ImageDef, clocks::RoscRng};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::channel::{Channel, Receiver, Sender};
use embassy_time::{Delay, Duration, Timer};
use embedded_alloc::LlffHeap as Heap;
use embedded_graphics::geometry::Point;
use embedded_graphics::mono_font::MonoTextStyle;
use embedded_graphics::mono_font::iso_8859_5::FONT_9X15_BOLD;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::text::Text;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal_async::spi::SpiDevice as AsyncSpiDeviceTrait;
use embedded_hal_bus::spi::ExclusiveDevice;
use heapless::String as HeaplessString;
use ssd1680::async_driver::Ssd1680Async;
use ssd1680::graphics::{Display, Display2in13, DisplayRotation};
use static_cell::StaticCell;
use tsl2591_eh_driver::Driver as Tsl2591Driver;
use {defmt_rtt as _, panic_probe as _};

// Type defs
type I2c1Bus = Mutex<NoopRawMutex, RefCell<i2c::I2c<'static, I2C1, i2c::Blocking>>>;

const SENSOR_DATA_CHANNEL_DEPTH: usize = 32;
type SensorDataChannel = Channel<CriticalSectionRawMutex, DataType, SENSOR_DATA_CHANNEL_DEPTH>;
type SensorDataReceiver =
    Receiver<'static, CriticalSectionRawMutex, DataType, SENSOR_DATA_CHANNEL_DEPTH>;
type SensorDataSender =
    Sender<'static, CriticalSectionRawMutex, DataType, SENSOR_DATA_CHANNEL_DEPTH>;
type DisplaySpiDevice = ExclusiveDevice<Spi<'static, SPI0, Async>, Output<'static>, Delay>;

// Static vars
#[global_allocator]
static HEAP: Heap = Heap::empty();

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

static SENSOR_DATA_CHANNEL: SensorDataChannel = Channel::new();

// Funcs
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

/// Handles all sensors that read from the i2c1 shared bus
#[embassy_executor::task]
async fn i2c1_sensor_task(i2c_bus: &'static I2c1Bus, tx: SensorDataSender) {
    info!("Init I2c Shared bus");
    let i2c_device = I2cDevice::new(i2c_bus);
    let timer = &mut Delay;
    let mut aht20_uninit = aht20_driver::AHT20::new(i2c_device, aht20_driver::SENSOR_ADDRESS);
    let mut aht20 = aht20_uninit.init(timer).unwrap();

    // TSL2591
    let tsl_device = I2cDevice::new(i2c_bus);
    let mut tsl2591 = Tsl2591Driver::new(tsl_device).unwrap();
    tsl2591.enable().unwrap();

    loop {
        let measure = aht20.measure(timer).unwrap();
        tx.send(DataType::Temperature(temperature::Celsius::new(
            measure.temperature,
        )))
        .await;
        tx.send(DataType::RelativeHumidity(humidity::RelativeHumidity::new(
            measure.humidity,
        )))
        .await;

        let lux_value = match tsl2591.get_channel_data() {
            Err(_) => {
                warn!("Light sensor I2C error reading channel data");
                None
            }
            Ok((ch0, ch1)) => match tsl2591.calculate_lux(ch0, ch1) {
                Ok(lux) => Some(light::Lux::new(lux)),
                Err(tsl2591_eh_driver::Error::SignalOverflow()) => {
                    warn!("Light sensor saturated (signal overflow)");
                    None
                }
                Err(tsl2591_eh_driver::Error::InfraredOverflow()) => {
                    warn!("Light sensor infrared overflow");
                    None
                }
                Err(tsl2591_eh_driver::Error::IdMismatch(id)) => {
                    warn!("Light sensor ID mismatch: {}", id);
                    None
                }
                Err(tsl2591_eh_driver::Error::I2cError(_)) => {
                    warn!("Light sensor I2C error during lux calculation");
                    None
                }
            },
        };
        if let Some(lux_value) = lux_value {
            tx.send(DataType::Light(lux_value)).await;
        }

        Timer::after(Duration::from_millis(100)).await;
    }
}

fn get_unique_id() -> u128 {
    embassy_rp::otp::get_chipid().expect("error fetching chip ID") as u128
}

/// Handles all network I/O: responds to discovery, then streams sensor data unicast.
///
/// Protocol flow:
///   Server → multicast Discover → we reply DiscoverResponse (unicast, our chip ID in header)
///   Server → unicast StartStreaming → we send DataReading to server address
///   Server → unicast StopStreaming  → we stop
#[embassy_executor::task]
async fn network_task(stack: Stack<'static>, rx: SensorDataReceiver) {
    info!("network_task: waiting for DHCP");
    while !stack.is_config_up() {
        Timer::after_millis(100).await;
    }
    info!("DHCP is up");

    let multicast_ip = IpAddress::Ipv4(Ipv4Addr::new(239, 0, 0, 1));
    stack
        .join_multicast_group(multicast_ip)
        .expect("Unable to join multicast group");

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];

    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buffer,
        &mut tx_meta,
        &mut tx_buffer,
    );
    socket.bind(5000).expect("Error binding to socket");

    let packet_builder = PacketBuilder::new(get_unique_id());
    // recv_from returns UdpMetadata; we only need the IpEndpoint inside it.
    let mut server_addr: Option<IpEndpoint> = None;
    let mut recv_buf = [0u8; 1500];

    loop {
        // Wait for either a new server command or a new data reading to send to a server
        match select(socket.recv_from(&mut recv_buf), rx.receive()).await {
            Either::First(recv_result) => match recv_result {
                Ok((len, meta)) => {
                    let src = meta.endpoint;
                    match postcard::from_bytes::<Packet>(&recv_buf[..len]) {
                        Ok(packet) => match packet.command() {
                            PacketCommand::Discover => {
                                info!("Discover from {:?}, sending DiscoverResponse", src);
                                let resp = packet_builder.build(PacketCommand::DiscoverResponse);
                                match to_allocvec(&resp) {
                                    Ok(data) => {
                                        if let Err(e) = socket.send_to(&data, src).await {
                                            warn!("DiscoverResponse send error: {:?}", e);
                                        }
                                    }
                                    Err(_) => warn!("DiscoverResponse serialize error"),
                                }
                            }
                            PacketCommand::StartStreaming => {
                                info!("StartStreaming from {:?}", src);
                                server_addr = Some(src);
                            }
                            PacketCommand::StopStreaming => {
                                info!("StopStreaming");
                                server_addr = None;
                            }
                            _ => {}
                        },
                        Err(_) => warn!("packet parse error"),
                    }
                }
                Err(e) => warn!("recv_from error: {:?}", e),
            },
            Either::Second(reading) => {
                if let Some(addr) = server_addr {
                    let packet = packet_builder.build(PacketCommand::DataReading(reading));
                    match to_allocvec(&packet) {
                        Ok(data) => {
                            if let Err(e) = socket.send_to(&data, addr).await {
                                warn!("DataReading send error: {:?}", e);
                            }
                        }
                        Err(_) => warn!("DataReading serialize error"),
                    }
                }
                // if server_addr is None, discard the reading
            }
        }
    }
}

async fn run_display<SPI, DC, BSY, RST>(
    spi_device: SPI,
    dc: DC,
    busy: BSY,
    rst: RST,
    rx: SensorDataReceiver,
) where
    SPI: AsyncSpiDeviceTrait,
    DC: OutputPin,
    BSY: InputPin,
    RST: OutputPin,
{
    let disp_interface = display_interface_spi::SPIInterface::new(spi_device, dc);
    let mut delay = Delay;
    let mut ssd1680 = Ssd1680Async::new(disp_interface, busy, rst, &mut delay)
        .await
        .unwrap();
    ssd1680.clear_bw_frame().await.unwrap();
    let mut display_bw = Display2in13::bw();
    display_bw.set_rotation(DisplayRotation::Rotate270);
    display_bw
        .fill_solid(&display_bw.bounding_box(), BinaryColor::On)
        .unwrap();

    let delay_duration = Duration::from_millis(5000);
    let mut msg: HeaplessString<256> = HeaplessString::new();
    loop {
        // Wait for data
        rx.ready_to_receive().await;
        // Accumulate the avg of all available data
        let mut humidity_count = 0;
        let mut humidity = 0.0;
        let mut temp_count = 0;
        let mut temperature = 0.0;
        let mut lux_count = 0;
        let mut lux = 0.0;
        // TODO: this could probably be more robust since this expects temp and humidity to come at
        // the same time
        while let Ok(reading) = rx.try_receive() {
            match reading {
                DataType::Temperature(celsius) => {
                    temperature += celsius.get_as_f();
                    temp_count += 1;
                }
                DataType::RelativeHumidity(relative_humidity) => {
                    humidity += relative_humidity.percent();
                    humidity_count += 1;
                }
                DataType::Light(in_lux) => {
                    lux += in_lux.get_as_lux();
                    lux_count += 1;
                }
            }
        }
        display_bw
            .fill_solid(&display_bw.bounding_box(), BinaryColor::On)
            .unwrap();

        msg.clear();
        if temp_count > 0 && humidity_count > 0 {
            temperature /= temp_count as f32;
            humidity /= humidity_count as f32;
            write!(msg, "{:.2}F {:.2}%", temperature, humidity).expect("write to heapless string");
        } else {
            write!(msg, "No Temp Data").expect("write to heapless string");
        }

        Text::new(
            &msg,
            Point::new(5, 10),
            MonoTextStyle::new(&FONT_9X15_BOLD, BinaryColor::Off),
        )
        .draw(&mut display_bw)
        .unwrap();

        msg.clear();
        if lux_count > 0 {
            lux /= lux_count as f32;
            write!(msg, "{:.2}lux", lux).expect("write to heapless string");
        } else {
            write!(msg, "No Data lux").expect("write to heapless string");
        }
        Text::new(
            &msg,
            Point::new(5, 25),
            MonoTextStyle::new(&FONT_9X15_BOLD, BinaryColor::Off),
        )
        .draw(&mut display_bw)
        .unwrap();
        ssd1680.update_bw_frame(display_bw.buffer()).await.unwrap();
        ssd1680.display_frame(&mut Delay).await.unwrap();
        Timer::after(delay_duration).await;
    }
}

/// Concrete Embassy task — thin wrapper over `run_display`.
#[embassy_executor::task]
async fn display_task(
    spi_device: DisplaySpiDevice,
    dc: Output<'static>,
    busy: Input<'static>,
    rst: Output<'static>,
    rx: SensorDataReceiver,
) {
    run_display(spi_device, dc, busy, rst, rx).await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Init allocator
    {
        use core::mem::MaybeUninit;
        use core::ptr::addr_of_mut;
        const HEAP_SIZE: usize = 1024;
        static mut HEAP_MEM: [MaybeUninit<u8>; HEAP_SIZE] = [MaybeUninit::uninit(); HEAP_SIZE];
        unsafe { HEAP.init(addr_of_mut!(HEAP_MEM) as usize, HEAP_SIZE) }
    }

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
    unwrap!(spawner.spawn(cyw43_task(runner)));

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

    unwrap!(spawner.spawn(network_task(stack, SENSOR_DATA_CHANNEL.receiver())));

    info!("Building display");
    let disp_spi: Spi<'_, _, Async> =
        Spi::new_txonly(p.SPI0, p.PIN_2, p.PIN_3, p.DMA_CH1, spi::Config::default());
    let disp_cs = Output::new(p.PIN_5, Level::High);
    let disp_spi_device = ExclusiveDevice::new(disp_spi, disp_cs, Delay);
    unwrap!(spawner.spawn(display_task(
        disp_spi_device,
        Output::new(p.PIN_4, Level::Low),                // dc
        Input::new(p.PIN_1, embassy_rp::gpio::Pull::Up), // busy
        Output::new(p.PIN_0, Level::Low),                // rst
        SENSOR_DATA_CHANNEL.receiver()
    )));

    info!("Init I2c Shared bus");
    let sda = p.PIN_14;
    let scl = p.PIN_15;
    let i2c = i2c::I2c::new_blocking(p.I2C1, scl, sda, i2c::Config::default());
    static I2C_BUS: StaticCell<I2c1Bus> = StaticCell::new();
    let i2c_bus = I2C_BUS.init(Mutex::new(RefCell::new(i2c)));

    unwrap!(spawner.spawn(i2c1_sensor_task(i2c_bus, SENSOR_DATA_CHANNEL.sender())));

    // Blink LED
    let mut led_on = false;
    loop {
        control.gpio_set(0, led_on).await;
        led_on = !led_on;
        Timer::after(Duration::from_millis(1000)).await;
    }
}

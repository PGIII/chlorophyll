# pico_2w

Embassy-based firmware for the Raspberry Pi Pico 2W. Reads temperature, humidity, and light
sensors over I2C and streams readings over UDP. Displays live data on a Waveshare 2.13" e-ink
display (SSD1680, BW variant).

## Prerequisites

- `probe-rs` — flash and debug over SWD
- Rust target: `thumbv8m.main-none-eabihf`
- `cargo-bloat` — binary size reporting

```sh
cargo install cargo-bloat
rustup target add thumbv8m.main-none-eabihf
```

## Configuration

Copy `config.toml.example` to `config.toml` and fill in WiFi credentials.

## Building

```sh
cargo build
```

## Flashing

```sh
cargo run
```

Uses `probe-rs run --chip RP235x` via the runner configured in `.cargo/config.toml`.

## Binary size

```sh
cargo sz
```

Lists the largest functions by size with human-readable KiB values.

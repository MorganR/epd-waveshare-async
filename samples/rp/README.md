# RP-2040 samples

These samples run on an rp2040 microcontroller, such as a Raspberry Pi Pico or Pico W. See the code in [lib.rs](src/lib.rs) for the expected pin configuration.

## Development

### Core set up

1. Install Rust `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
2. Add the appropriate target: `rustup target add thumbv6m-none-eabi`
3. Install [probe-rs](https://probe.rs) `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/probe-rs/probe-rs/releases/latest/download/probe-rs-tools-installer.sh | sh`
4. Set up [udev rules](https://probe.rs/docs/getting-started/probe-setup/#linux%3A-udev-rules) for `probe-rs`
5. Set up your debug probe

### Debug probe

You need a debug probe to easily flash software to the Pico, to read its log messages, and to actively debug programs. We recommend using a [Raspberry Pi Debug Probe](https://www.raspberrypi.com/documentation/microcontrollers/debug-probe.html) or a Pico that [has been flashed](https://www.raspberrypi.com/documentation/microcontrollers/pico-series.html#debugging-using-another-pico-series-device) to work as a debug probe.

Follow the probe's [getting started instructions](https://www.raspberrypi.com/documentation/microcontrollers/debug-probe.html#getting-started) for more details. You should just need to set up `openocd`, and wire the probe up to your main Pico.

To verify this is working, run `rs-probe list`, and you should see your probe. Then, try `cargo run --release` and confirm the app runs. Try adding a simple LED blinking loop if you're not sure:

```rust
// Note: this only works on Pico, not a Pico W.
let mut led = Output::new(p.PIN_25, Level::Low);

loop {
    info!("led on!");
    led.set_high();
    Timer::after_secs(1).await;

    info!("led off!");
    led.set_low();
    Timer::after_secs(1).await;
}
```

### Running the samples

Samples can be run from the command line:

```shell
# This relies on the config in `samples/rp/.cargo`, which applies if you're in that directory.
cd samples/rp
cargo run --release --bin epd2in9
```

or via VS Code.
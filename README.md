# epd-waveshare-async

![build-status](https://github.com/MorganR/epd-waveshare-async/actions/workflows/build.yaml/badge.svg?branch=main&event=push)

[docs.rs](https://docs.rs/epd-waveshare-async/latest/epd_waveshare_async/)

Async drivers for Waveshare's e-paper displays.

This is inspired by both the existing (sync) [epd-waveshare](https://github.com/caemor/epd-waveshare)
crate, and the [e-Paper](https://github.com/waveshareteam/e-Paper/tree/master) code published by
Waveshare directly.

However, it diverges significantly in the public interface for the displays, with a focus on
**clarity, correctness, and flexibility**.

## [Changelog](./CHANGELOG.md)

## Drivers

This library only supports a small set of screens for which I have confirmed all functionality.
Drivers should all be tested on real displays using a sample program (see below). Each driver
should go in its own module.

## Samples

Sample code should exist for each display driver, to both demonstrate its use and to act as a test
case that can be easily run. These live in the `samples` folder, with one subfolder per
microcontroller. A sample just needs to be provided for at least one microcontroller per display
driver.

## Development

### Set up

1. Install Rust `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

We also use [Husky](https://typicode.github.io/husky/) to run formatters and linters on `git push`, which requires NPM. The suggested set up is the following:

1. Install [NVM](https://github.com/nvm-sh/nvmhttps://github.com/nvm-sh/nvm): `curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash`
2. Run `nvm install` to get a consistent version of node
3. Run `npm ci`
4. If needed, install [rustfmt](https://github.com/rust-lang/rustfmt): `rustup component add rustfmt`. You can run `cargo fmt` to see if this is already installed.

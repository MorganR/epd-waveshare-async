# epd-waveshare-async

![build-status](https://github.com/MorganR/epd-waveshare-async/actions/workflows/build.yaml/badge.svg?branch=main&event=push)

Async drivers for Waveshare's e-paper displays.

This is inspired by both the existing (sync) [epd-waveshare](https://github.com/caemor/epd-waveshare) crate, and the [e-Paper](https://github.com/waveshareteam/e-Paper/tree/master) code published by Waveshare directly. It includes modifications related to local testing.

To start, this library will only support the display(s) I am actively testing with it. Once in a stable state, contributions for more displays will be welcome.

## Developing

### Set up

1. Install Rust `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

We also use [Husky](https://typicode.github.io/husky/) to run formatters and linters on `git push`, which requires NPM. The suggested set up is the following:

1. Install [NVM](https://github.com/nvm-sh/nvmhttps://github.com/nvm-sh/nvm): `curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.1/install.sh | bash`
2. Run `nvm install` to get a consistent version of node
3. Run `npm ci`
4. Install [rustfmt](https://github.com/rust-lang/rustfmt) if needed: `rustup component add rustfmt`. You can run `cargo fmt` to see if this is already installed.

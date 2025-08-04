# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Rust library providing async drivers for Waveshare's e-paper displays, built on `embedded-hal-async` and `embedded-graphics`. The library is designed for no_std embedded environments.

## Workspace Structure

- Root workspace with two main components:
  - `epd-waveshare-async/` - Main library crate
  - `samples/` - Sample applications for different microcontrollers (currently RP2040)

## Core Architecture

The library uses a composable trait-based architecture split into hardware abstraction and functionality traits:

### Hardware Abstraction
- `EpdHw`: Hardware abstraction for SPI communication, GPIO pins (Data/Command, Reset, Busy), and delay timers. Users must implement this trait for their hardware.

### Functionality Traits
The functionality is split into composable traits for granular support and compile-time state checking:

- `Reset`: Basic hardware reset support
- `Sleep`: Displays that can be put to sleep for power saving
- `Wake`: Displays that can be woken from sleep state
- `Displayable`: Base trait for displays that can be updated separately from framebuffer data
- `DisplaySimple`: Basic support for writing and displaying a single framebuffer with configurable bit depth and frame count
- `DisplayPartial`: Support for partial refresh using diff framebuffers against a base framebuffer

The crate provides buffer utilities in the `buffer` module and display-specific modules like `epd2in9` and `epd2in9_v2`.

## Common Development Commands

### Building and Testing
```bash
# Build the main library
cargo build

# Run tests
cargo test

# Build for release
cargo build --release

# Check without building
cargo check
```

### Code Quality
```bash
# Format code (used by pre-commit hooks)
cargo fmt

# Run clippy linter
cargo clippy

# Format all files with prettier (via husky)
npx prettier --write --ignore-unknown .
```

### Sample Development (RP2040)
```bash
# Navigate to samples directory
cd samples/rp

# Add required target (if not already added)
rustup target add thumbv6m-none-eabi

# Run a specific sample (requires probe-rs and debug probe)
cargo run --release --bin epd2in9

# List available probes
rs-probe list
```

### Pre-commit Setup
The project uses Husky for pre-commit hooks:
```bash
# Install dependencies (if not done)
npm ci

# Husky is automatically set up via package.json prepare script
```

## Dependencies and Features

The library supports optional logging via:
- `defmt` feature for embedded logging
- `log` feature for standard Rust logging

Key dependencies:
- `embedded-hal-async` and `embedded-hal` for hardware abstraction
- `embedded-graphics` for drawing operations
- Embassy framework for async embedded development (dev dependencies)

## Testing

Tests are run with standard `cargo test`. The samples act as integration tests and require actual hardware (RP2040 with debug probe) to run.

## Display Support

Currently supports:
- 2.9" EPD v1 (`epd2in9` module)
- 2.9" EPD v2 (`epd2in9_v2` module)

Each display driver should have corresponding sample code in the `samples/` directory.
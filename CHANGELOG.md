# Changelog

## v0.3.0

- Split `EpdHw` into separate traits to support drivers that require different hardware configurations (e.g. the 7" display requires control of an extra power pin).
- Moved `XHw` traits into the `hw` module.
- Updated heapless version.

## v0.2.0

- Split `Epd` trait into separate, stateful traits. This enables display drivers to support only relevant features.
- Added the 2in9v2 driver.
- Added state to the 2in9 and 2in9v2 drivers, so that it's impossible to accidentally use the display while it's asleep.

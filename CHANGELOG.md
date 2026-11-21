# Changelog

## Unreleased

- **Breaking:** displays now start in a new `StateUnknown`, which only supports `reset()`. Call `reset()` before `init()`. This avoids a double reset, e.g. on display wake.
- **Breaking:** `reset()` now always returns the display in `StateUninitialized`, from any state, as a hardware reset clears the display's configuration. In practice, I've found this isn't _always_ necessary, but the behaviour is unreliable. Always re-initialising better follows Waveshare's specs and sample code.
- **Breaking:** `init()` is only available in `StateUninitialized`, and no longer performs its own hardware reset.
- The 2.9" (v1) driver now re-initialises the display when waking from sleep.
- Fixed the 7.5" V2 driver not performing a hardware reset when `reset()` is called on a sleeping display.

## v0.3.2

- Add 7.5" V2 driver (thanks to @SakiiCode)
- Re-initialise the 2.9" V2 driver when waking from sleep, as Waveshare documents you must.

## v0.3.1

- Add `FullSlow` refresh mode to `epd2in9_v2`.

## v0.3.0

- Split `EpdHw` into separate traits to support drivers that require different hardware configurations (e.g. the 7" display requires control of an extra power pin).
- Moved `XHw` traits into the `hw` module.
- Updated heapless version.

## v0.2.0

- Split `Epd` trait into separate, stateful traits. This enables display drivers to support only relevant features.
- Added the 2in9v2 driver.
- Added state to the 2in9 and 2in9v2 drivers, so that it's impossible to accidentally use the display while it's asleep.

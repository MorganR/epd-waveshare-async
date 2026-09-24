//! Mock hardware for testing drivers on the host. Records everything a driver does to the pins
//! and SPI bus, so tests can compare it against reference command sequences.
//!
//! The mock also models the controller's busy state: after any command in `busy_commands`, BUSY
//! reads as busy until the driver waits for it to go idle. Commands sent while busy are recorded
//! as violations, since controllers ignore them.

extern crate std;

use core::{
    cell::RefCell,
    convert::Infallible,
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use std::{rc::Rc, vec::Vec};

use embedded_hal::{
    digital::{ErrorType as PinErrorType, InputPin, OutputPin, PinState},
    spi::{ErrorType as SpiErrorType, Operation},
};
use embedded_hal_async::{delay::DelayNs, digital::Wait, spi::SpiDevice};

use crate::hw::{BusyHw, DcHw, DelayHw, ErrorHw, ResetHw, SpiHw};

/// Runs a future to completion. The mocks never return `Pending`, so this just polls.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
            return output;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Reset(PinState),
    Command(u8),
    Data(Vec<u8>),
    /// A command was sent while the controller was busy.
    CommandWhileBusy(u8),
    /// The driver waited for BUSY to show idle.
    WaitedForIdle,
    DelayNs(u32),
}

struct Shared {
    events: Vec<Event>,
    dc: PinState,
    busy: bool,
    busy_when: PinState,
    busy_commands: &'static [u8],
}

/// A handle onto the recorded events.
#[derive(Clone)]
pub struct Log(Rc<RefCell<Shared>>);

impl Log {
    pub fn events(&self) -> Vec<Event> {
        self.0.borrow().events.clone()
    }

    pub fn clear(&self) {
        self.0.borrow_mut().events.clear();
    }

    pub fn is_busy(&self) -> bool {
        self.0.borrow().busy
    }

    /// Commands with their data bytes, ignoring pin activity and delays.
    pub fn commands_with_data(&self) -> Vec<(u8, Vec<u8>)> {
        let mut commands: Vec<(u8, Vec<u8>)> = Vec::new();
        for event in &self.0.borrow().events {
            match event {
                Event::Command(c) => commands.push((*c, Vec::new())),
                Event::Data(d) => commands
                    .last_mut()
                    .expect("data sent before any command")
                    .1
                    .extend_from_slice(d),
                _ => {}
            }
        }
        commands
    }

    pub fn violations(&self) -> Vec<Event> {
        self.0
            .borrow()
            .events
            .iter()
            .filter(|e| matches!(e, Event::CommandWhileBusy(_)))
            .cloned()
            .collect()
    }

    fn push(&self, event: Event) {
        self.0.borrow_mut().events.push(event);
    }

    /// The level the BUSY pin currently reads.
    fn busy_level(&self) -> PinState {
        let shared = self.0.borrow();
        if shared.busy {
            shared.busy_when
        } else {
            !shared.busy_when
        }
    }

    fn wait_for(&self, level: PinState) {
        let idle = !self.0.borrow().busy_when;
        assert!(
            level == idle,
            "the mock controller only supports waiting for idle"
        );
        self.push(Event::WaitedForIdle);
        self.0.borrow_mut().busy = false;
    }
}

pub struct MockSpi(Log);

impl SpiErrorType for MockSpi {
    type Error = Infallible;
}

impl SpiDevice for MockSpi {
    async fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> Result<(), Self::Error> {
        for op in operations {
            match op {
                Operation::Write(bytes) => {
                    let mut shared = self.0 .0.borrow_mut();
                    if shared.dc == PinState::High {
                        shared.events.push(Event::Data(bytes.to_vec()));
                    } else {
                        for &command in bytes.iter() {
                            if shared.busy {
                                shared.events.push(Event::CommandWhileBusy(command));
                            }
                            shared.events.push(Event::Command(command));
                            if shared.busy_commands.contains(&command) {
                                shared.busy = true;
                            }
                        }
                    }
                }
                _ => unimplemented!("the mock only supports writes"),
            }
        }
        Ok(())
    }
}

pub struct MockDc(Log);

impl PinErrorType for MockDc {
    type Error = Infallible;
}

impl OutputPin for MockDc {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().dc = PinState::Low;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0 .0.borrow_mut().dc = PinState::High;
        Ok(())
    }
}

pub struct MockReset(Log);

impl PinErrorType for MockReset {
    type Error = Infallible;
}

impl OutputPin for MockReset {
    fn set_low(&mut self) -> Result<(), Self::Error> {
        self.0.push(Event::Reset(PinState::Low));
        // A reset aborts whatever the controller was doing.
        self.0 .0.borrow_mut().busy = false;
        Ok(())
    }

    fn set_high(&mut self) -> Result<(), Self::Error> {
        self.0.push(Event::Reset(PinState::High));
        Ok(())
    }
}

pub struct MockBusy(Log);

impl PinErrorType for MockBusy {
    type Error = Infallible;
}

impl InputPin for MockBusy {
    fn is_high(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.busy_level() == PinState::High)
    }

    fn is_low(&mut self) -> Result<bool, Self::Error> {
        Ok(self.0.busy_level() == PinState::Low)
    }
}

impl Wait for MockBusy {
    async fn wait_for_high(&mut self) -> Result<(), Self::Error> {
        self.0.wait_for(PinState::High);
        Ok(())
    }

    async fn wait_for_low(&mut self) -> Result<(), Self::Error> {
        self.0.wait_for(PinState::Low);
        Ok(())
    }

    async fn wait_for_rising_edge(&mut self) -> Result<(), Self::Error> {
        self.0.wait_for(PinState::High);
        Ok(())
    }

    async fn wait_for_falling_edge(&mut self) -> Result<(), Self::Error> {
        self.0.wait_for(PinState::Low);
        Ok(())
    }

    async fn wait_for_any_edge(&mut self) -> Result<(), Self::Error> {
        let idle = !self.0 .0.borrow().busy_when;
        self.0.wait_for(idle);
        Ok(())
    }
}

pub struct MockDelay(Log);

impl DelayNs for MockDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.0.push(Event::DelayNs(ns));
    }
}

pub struct MockHw {
    busy_when: PinState,
    dc: MockDc,
    reset: MockReset,
    busy: MockBusy,
    delay: MockDelay,
}

impl MockHw {
    /// Creates mock hardware whose controller shows `busy_when` on BUSY after each of
    /// `busy_commands`, until the driver waits for it.
    pub fn new(busy_when: PinState, busy_commands: &'static [u8]) -> (Self, MockSpi, Log) {
        let log = Log(Rc::new(RefCell::new(Shared {
            events: Vec::new(),
            dc: PinState::Low,
            busy: false,
            busy_when,
            busy_commands,
        })));
        let hw = MockHw {
            busy_when,
            dc: MockDc(log.clone()),
            reset: MockReset(log.clone()),
            busy: MockBusy(log.clone()),
            delay: MockDelay(log.clone()),
        };
        (hw, MockSpi(log.clone()), log)
    }
}

impl ErrorHw for MockHw {
    type Error = Infallible;
}

impl SpiHw for MockHw {
    type Spi = MockSpi;
}

impl DcHw for MockHw {
    type Dc = MockDc;

    fn dc(&mut self) -> &mut Self::Dc {
        &mut self.dc
    }
}

impl ResetHw for MockHw {
    type Reset = MockReset;

    fn reset(&mut self) -> &mut Self::Reset {
        &mut self.reset
    }
}

impl BusyHw for MockHw {
    type Busy = MockBusy;

    fn busy(&mut self) -> &mut Self::Busy {
        &mut self.busy
    }

    fn busy_when(&self) -> PinState {
        self.busy_when
    }
}

impl DelayHw for MockHw {
    type Delay = MockDelay;

    fn delay(&mut self) -> &mut Self::Delay {
        &mut self.delay
    }
}

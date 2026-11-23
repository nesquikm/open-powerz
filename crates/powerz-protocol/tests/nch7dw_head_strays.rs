//! FR NCH7DW, AC .6, in the flash engine over a hand-written fake old
//! bootloader: a HEAD exchange that yields only stray ACCEPTs past the cap
//! is a read error (vendor code -108), not a product error.
//!
//! Uses only API that exists before the fix. No real device is opened;
//! every container is synthetic.

mod common;
#[path = "support/fw_container.rs"]
mod fw_container;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use common::FakeClock;
use fw_container::*;
use powerz_protocol::firmware::DeviceMode;
use powerz_protocol::firmware::container::FirmwareContainer;
use powerz_protocol::firmware::flash::{
    self, FlashAck, FlashError, FlashTransport, Meter, MeterOpener,
};
use powerz_protocol::transport::discovery::Model;
use powerz_protocol::transport::{Error as TransportError, Transport};

// ---------------------------------------------------------------- AC-NCH7DW.6

const P: usize = 3 * 4080 + 100;

fn container_bytes() -> Vec<u8> {
    let payload: Vec<u8> = (0..P + 16).map(|i| (i * 37 + 11) as u8 | 0x80).collect();
    ContainerBuilder::new()
        .payload(&payload)
        .field(SIZE, &P.to_string())
        .build()
}

/// What the fake saw.
#[derive(Debug, Default)]
struct Seen {
    transacts: Vec<Vec<u8>>,
    reads: usize,
    raw: Vec<Vec<u8>>,
}

/// An old bootloader that answers every transfer with an ACCEPT carrying
/// its own counter: CONNECT is accepted, and HEAD yields only stray
/// ACCEPTs, however many are read.
#[derive(Debug, Clone)]
struct AcceptsOnly {
    seen: Rc<RefCell<Seen>>,
    own: Rc<RefCell<u8>>,
}

impl AcceptsOnly {
    fn accept(&self) -> Vec<u8> {
        let mut own = self.own.borrow_mut();
        let id = *own;
        *own = own.wrapping_add(1);
        vec![0x05, id, 0x00, 0x00]
    }
}

impl Transport for AcceptsOnly {
    fn transact(&mut self, request: &[u8], _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.seen.borrow_mut().transacts.push(request.to_vec());
        Ok(self.accept())
    }

    fn read(&mut self, _timeout: Duration) -> Result<Vec<u8>, TransportError> {
        self.seen.borrow_mut().reads += 1;
        Ok(self.accept())
    }

    fn recover(&mut self) -> Result<(), TransportError> {
        Err(TransportError::Io("simulated pipe recovery failed".into()))
    }
}

impl FlashTransport for AcceptsOnly {
    fn write_raw(&mut self, data: &[u8], _timeout: Duration) -> Result<(), TransportError> {
        self.seen.borrow_mut().raw.push(data.to_vec());
        Ok(())
    }
}

struct Opener(AcceptsOnly);

impl MeterOpener for Opener {
    type Pipe = AcceptsOnly;

    fn open(&mut self) -> Result<Option<Meter<AcceptsOnly>>, TransportError> {
        Ok(Some(Meter {
            mode: DeviceMode::LegacyBootloader,
            model: Model::Km003c,
            pipe: self.0.clone(),
        }))
    }
}

fn run_accepts_only() -> (Result<flash::FlashOutcome, FlashError>, Rc<RefCell<Seen>>) {
    let pipe = AcceptsOnly {
        seen: Rc::new(RefCell::new(Seen::default())),
        own: Rc::new(RefCell::new(0x50)),
    };
    let seen = pipe.seen.clone();
    let bytes = container_bytes();
    let c = FirmwareContainer::parse(&bytes).expect("a synthetic container parses");
    let stop = AtomicBool::new(false);
    let r = flash::flash(
        FlashAck::acknowledge().with_experimental_old_bootloader(),
        &c,
        &mut Opener(pipe),
        FakeClock::new(1_790_000_000_000),
        &stop,
        &mut |_| {},
    );
    (r, seen)
}

#[test]
fn nch7dw_old_boot_head_with_only_stray_accepts_is_a_read_error() {
    let (r, _) = run_accepts_only();
    assert!(
        matches!(r, Err(FlashError::Read { .. })),
        "only stray ACCEPTs at HEAD must be a read error, got {r:?}"
    );
}

#[test]
fn nch7dw_old_boot_head_with_only_stray_accepts_has_vendor_code_minus_108() {
    let (r, _) = run_accepts_only();
    let code = r.as_ref().err().map(FlashError::code);
    assert_eq!(code, Some(-108), "{r:?}");
}

#[test]
fn nch7dw_old_boot_head_with_only_stray_accepts_sends_no_descriptor() {
    let (r, seen) = run_accepts_only();
    assert!(r.is_err(), "{r:?}");
    let seen = seen.borrow();
    assert!(
        !seen.raw.iter().any(|d| d.first() == Some(&0x42)),
        "no descriptor after a HEAD with no HEAD reply: {:02x?}",
        seen.raw
    );
    assert!(
        seen.transacts.iter().any(|t| t.first() == Some(&0x40)),
        "HEAD was sent: {:02x?}",
        seen.transacts
    );
}

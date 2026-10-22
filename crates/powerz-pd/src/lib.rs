//! USB Power Delivery revision 3.2 message decoder (FR TGE83B).
//!
//! Pure library: no I/O, builds for `wasm32-unknown-unknown`. The capture
//! core in `powerz-protocol` feeds it PD records; the decoder turns them into
//! typed messages with a one-line summary and a field tree.

mod blocks;
mod capture;
mod extended;
mod field;
mod header;
mod pdo;
mod rdo;
mod vdm;

pub use capture::{Capture, Decoded, Input, Issue, Message};
pub use extended::{EcdbType, EprAction, EprMode, ExtendedControl, ExtendedHeader};
pub use field::{BitRange, Field, Flag};
pub use header::{
    CablePlug, DataRole, Header, MessageKind, MessageType, PowerRole, Sop, SpecRevision,
};
pub use pdo::{Battery, EprAvs, FixedSink, FixedSource, Pps, SinkPdo, SourcePdo, SprAvs, Variable};
pub use rdo::{AvsRdo, BatteryRdo, FixedRdo, PpsRdo, Rdo, Request};
pub use vdm::{
    ActiveCableVdo, ActiveCableVdo2, CommandType, DfpVdo, DiscoverIdentity, IdHeader,
    PassiveCableVdo, ProductTypeVdo, ProductVdo, StructuredVdmHeader, UfpVdo,
    UnstructuredVdmHeader, Vdm, VdmBody, VdmCommand, VdmHeader,
};

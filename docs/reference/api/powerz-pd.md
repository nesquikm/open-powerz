<!-- WARNING: This reference was generated without mechanical signature extraction for this stack. Signatures may be imprecise. Review carefully before publishing. -->

# `powerz-pd` API

`powerz-pd` decodes USB Power Delivery revision 3.2 messages. It has no
dependencies and does no I/O, so it also builds for `wasm32-unknown-unknown`.
`powerz-protocol` re-exports it as `powerz_protocol::pd`, so most users reach it
through that path.

The authoritative, always-current API reference is rustdoc:

```bash
RUSTDOCFLAGS="-D warnings" cargo doc -p powerz-pd --no-deps --locked --open
```

This page is a map of the public items. Every item is exported from the crate
root. Public enums are `#[non_exhaustive]`: match them with a wildcard arm, as
new variants may be added. Visibility rule: the `decode` of a whole wire value
(header, data object, message body, such as `Vdm::decode`,
`Request::against_caps` and `Request::with_copy`) is `pub`; a decoder of one
variant's body stays private to its module.

## Decoding a capture

Feed the records of one capture to a `Capture`, in order. It remembers what a
later message needs: the last Source_Capabilities on SOP (so a Request decodes
against the offer it points at) and the chunks of a long extended message.

| item | signature or contents |
|---|---|
| `Capture` | `Capture::new() -> Capture`; `feed(&mut self, input: Input<'_>) -> Option<Decoded>`. Messages decode to `Some`. Events update the context and decode to `None`: a Hard Reset or a Detach forgets the remembered offer and pending chunks. |
| `Input<'a>` | `Message { sop: u8, header: u16, body: &'a [u8] }` (the meter's SOP code, the 16-bit header, the bytes after it), `HardReset`, `CableReset`, `Attach`, `Detach` |
| `Decoded` | `header: Header`, `extended_header: Option<ExtendedHeader>`, `message: Message`, `issues: Vec<Issue>`; `summary(&self) -> String` (one line for the packet table, starting with the type name); `fields(&self) -> Vec<Field>` (the field tree) |
| `Message` | The typed body: `Control`, `SourceCapabilities(Vec<SourcePdo>)`, `SinkCapabilities(Vec<SinkPdo>)`, `Request(Request)`, `EprRequest(Request)`, `Vdm(Vdm)`, `EprMode(EprMode)`, `ExtendedControl(ExtendedControl)`, `EprSourceCapabilities(Vec<SourcePdo>)` and `EprSinkCapabilities(Vec<SinkPdo>)` (reassembled when chunked), `Chunk { chunk_number, data_size }`, `ChunkRequest { chunk_number }`, `Data(Vec<u32>)` and `Extended(Vec<u8>)` for types without a typed body |
| `Issue` | Implements `Display` (a readable sentence with its numbers). Problems found while decoding: `NoSourceCapabilities`, `PositionOutOfRange { position, available }`, `MissingChunk { expected, got }`, `ShortBody { expected, got }` |

The decoder never panics on any input; anything it cannot type is kept raw.

## The field tree

| item | contents |
|---|---|
| `Field` | `name`, `bits: BitRange`, `raw: u64` (the bits shifted down to bit 0), `meaning` (with units where the spec has them), `flag: Option<Flag>`, `children: Vec<Field>`; builders `Field::new(name, hi, lo, raw, meaning)`, `flagged(self, flag: Option<Flag>)`, `with_children(self, children: Vec<Field>)` |
| `BitRange` | `hi`, `lo`: inclusive. Header and data-object fields count bits inside their 16- or 32-bit word; data-block fields count from bit 0 of the block's first byte. |
| `Flag` | `Reserved` (a reserved value, or a reserved bit that is set), `Invalid` (a value the spec does not allow here) |

## Message header

| item | signature or contents |
|---|---|
| `Header` | `Header::decode(raw: u16, sop: Sop) -> Header`; `field(&self) -> Field`. Fields: `raw`, `extended`, `num_data_objects`, `message_id`, `port_power_role: Option<PowerRole>` (SOP only), `cable_plug: Option<CablePlug>` (SOP'/SOP'' only), `spec_revision`, `port_data_role: Option<DataRole>` (SOP only), `message_type`, `sop` |
| `Sop` | `Sop`, `SopPrime`, `SopDoublePrime`, `SopPrimeDebug`, `SopDoublePrimeDebug`, `Other(u8)`; `Sop::from_raw(code: u8) -> Sop` (0 SOP, 1 SOP', 2 SOP'', 3 SOP'_Debug, 4 SOP''_Debug); `is_cable(self) -> bool` |
| `MessageType` | `MessageType::new(kind: MessageKind, code: u8)`; `kind(self) -> MessageKind`; `code(self) -> u8`; `name(self) -> &'static str` (the spec name, for example `Source_Capabilities`); `is_reserved(self) -> bool` |
| `MessageKind` | `Control`, `Data`, `Extended` |
| `PowerRole`, `DataRole`, `CablePlug`, `SpecRevision` | Header bit fields: `Sink`/`Source`; `Ufp`/`Dfp`; `Port`/`CablePlug`; `Rev1`/`Rev2`/`Rev3`/`Reserved` |
| `ExtendedHeader` | `ExtendedHeader::decode(raw: u16) -> ExtendedHeader`; `field(&self) -> Field` |

## Power data objects and requests

| item | signature or contents |
|---|---|
| `SourcePdo` | `Fixed(FixedSource)`, `Battery(Battery)`, `Variable(Variable)`, `Pps(Pps)`, `SprAvs(SprAvs)`, `EprAvs(EprAvs)`, `Reserved(u32)`; `SourcePdo::decode(raw: u32) -> SourcePdo`; `field(&self, index: usize) -> Field` |
| `SinkPdo` | `Fixed(FixedSink)` and the same other variants; `SinkPdo::decode(raw: u32) -> SinkPdo`; `field(&self, index: usize) -> Field` |
| `FixedSource`, `FixedSink`, `Battery`, `Variable`, `Pps`, `SprAvs`, `EprAvs` | The decoded PDO bodies |
| `Request` | A Request or EPR_Request: `rdo: Rdo`, `pdo: Option<SourcePdo>` (the PDO it requests, when known) |
| `Rdo` | `Fixed(FixedRdo)`, `Variable(FixedRdo)`, `Battery(BatteryRdo)`, `Pps(PpsRdo)`, `Avs(AvsRdo)`, `Unresolved { position, raw }`; `Rdo::decode(raw: u32, pdo: Option<&SourcePdo>) -> Rdo`; `raw(&self) -> u32`; `position(&self) -> u8`; `field(&self) -> Field` |
| `FixedRdo`, `BatteryRdo`, `PpsRdo`, `AvsRdo` | The decoded RDO bodies |

## Vendor-defined and extended messages

| item | signature or contents |
|---|---|
| `Vdm` | `header: VdmHeader`, `body: VdmBody`; `Vdm::decode(objects: &[u32], sop: Sop) -> Option<Vdm>` (`None` without a header object) |
| `VdmHeader` | `Structured(StructuredVdmHeader)` or unstructured (`UnstructuredVdmHeader`); `VdmHeader::decode(raw: u32) -> VdmHeader` |
| `VdmBody` | `DiscoverIdentity(DiscoverIdentity)`, `DiscoverSvids(Vec<u16>)`, `DiscoverModes(Vec<u32>)`, `Objects(Vec<u32>)` |
| `VdmCommand` | `name(self) -> String` (for example `Discover Identity`) |
| `CommandType` | `Req`, `Ack`, `Nak`, `Busy`; `name(self) -> &'static str` |
| `DiscoverIdentity`, `IdHeader`, `ProductVdo`, `ProductTypeVdo`, `PassiveCableVdo`, `ActiveCableVdo`, `ActiveCableVdo2`, `UfpVdo`, `DfpVdo` | The Discover Identity ACK and its VDOs |
| `ExtendedControl` | `ExtendedControl::decode(data: &[u8]) -> Option<ExtendedControl>`; its type is an `EcdbType` (`name(self) -> &'static str`) |
| `EprMode` | `EprMode::decode(dobj: u32) -> EprMode`; its action is an `EprAction` (`name(self) -> &'static str`) |

## Example

```rust
use powerz_protocol::pd::{Capture, Input};

let mut capture = Capture::new();
let body = [0x2c, 0x91, 0x81, 0x08]; // one Fixed 5 V 3 A PDO
if let Some(decoded) = capture.feed(Input::Message { sop: 0, header: 0x11a1, body: &body }) {
    println!("{}", decoded.summary()); // Source_Capabilities: Fixed 5.00 V 3.00 A
}
```

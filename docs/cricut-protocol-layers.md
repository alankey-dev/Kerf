# How Design Space reaches the Maker 3, layer by layer

A plain-language walk through the stack, for understanding why a third-party app cannot
drive a Maker without Cricut's server. It stays at the level the existing evidence docs
already establish (`evidence-native.md`, `evidence-handshake.md`,
`real-device-feasibility.md`). It does not
reproduce any key-derivation, key-extraction or decryption procedure, because the project
does not build those. Where a step is the encrypted part, this doc names it and stops.

## The layers, top to bottom

1. **Renderer (Angular, in Electron).** The UI. It owns the material and tool catalogue and
   every cloud call, and it drives a Make by exchanging status values with the bridge. It
   never touches the serial port. It does one thing that matters for the wall: once per
   login it fetches the user's settings from Cricut's server and hands the result to the
   native layer as opaque `authData`.

2. **Electron main process.** Passes `authData` down and wraps the pipe to the bridge. The
   wrapping here protects only the local pipe between Electron and the bridge; it is not the
   machine's encryption.

3. **The bridge plugin (`CricutDevice`).** A native child process. It frames the device
   protocol, keeps the machine state (`matCutInteraction.deviceState`, which Kerf mirrors in
   `src/device.rs`), and holds the per-machine-type key material that arrived inside
   `authData`. This is where cleartext turns into the encrypted command stream.

4. **The transport plugin (`CricutDeviceIO`).** Owns the serial node and moves bytes. It does
   not understand the protocol; it is the wire.

5. **The machine.** A USB CDC-ACM serial device. Live capture confirms it: vendor id
   `0x20D3`, product id `0x0028`, one configuration, the two standard CDC interfaces
   (a control interface, class 2, and a data interface, class 10), one serial node, and no
   vendor-specific, HID or alternate interface. On a read-only listen it is silent; it
   speaks only once the host starts the conversation.

## Where the encryption sits, and where the key comes from

The transport (layers 4 and 5) is cleartext framing: a small header, a length, then a body.
The **body is encrypted** from the bridge down. The frames captured live from the bridge's
own log are `CC 01`, a 4-byte little-endian length, then that many high-entropy bytes. That
high entropy is the point: the command payload is AES-encrypted.

The session is set up by a handshake. The very first handshake message is sent in cleartext,
before any key is active, so the transport framing can be observed without a session. Every
command after the session is established is encrypted, and the machine refuses commands that
are not. Observing that the handshake exists tells you nothing you can act on, because the
reply can only be turned into a session key with the machine-type master key.

That master key is the wall. It is not generated on the client and it is not on the machine
in a form a third party can read. It descends from a secret that Cricut's server issues to
its own authenticated client, delivered inside `authData` and unwrapped by the bridge at
runtime. Reconstructing it means impersonating Cricut's authenticated client and undoing the
protections the server and bridge put around that secret. That is the device-authentication
bypass this project does not do. `real-device-feasibility.md` traces the same conclusion
from the binaries and recommends against it; this doc agrees and stops here.

## Why there is no open path for Kerf

Three facts close the loop:

- The machine exposes **one** serial pipe and no alternate interface (live capture).
- It is **silent** until the host drives it, and the only protocol it speaks on that pipe is
  the encrypted Cricut one (static analysis plus the live idle listen).
- Motion and cut commands are **encrypted and refused without the session**, whose key
  descends from the server-held secret (handshake analysis).

So there is no cleartext cut mode to drive. Kerf's honest interoperability is what spec 58
already ships: recognise the machine from its USB descriptor, drive **open** cutters that
speak cleartext HPGL or GRBL over the same kind of serial node, and tell the user plainly
that a Cricut machine needs Cricut Design Space to cut. To design for a Maker in Kerf and
cut in Design Space, use Kerf's SVG, DXF and PDF export (spec 25) and its Prepare for Design
Space handoff.

## What would change this

Only a supported route. If Cricut published or licensed a machine SDK, or gave written
authorisation to interoperate at the protocol level, Kerf could build against that without
touching the device's authentication. Absent that, the boundary stands.

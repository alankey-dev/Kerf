# Live capture: a Maker 3 attached, how Design Space reaches it over serial

Read-only observation taken on 2026-10-18 with a Cricut Maker 3 physically connected to
this Mac over USB. Nothing was sent to the machine. No encrypted traffic was decoded. This
report records only what the operating system and Design Space's own diagnostic log expose
while the official app runs. It is the live counterpart to the static analysis in
`evidence-native.md`, `evidence-handshake.md` and `real-device-feasibility.md`, which were
written with no machine present (`evidence-handshake.md` section 4 noted none was attached).

The point of this capture is the transport and the device identity, not the protocol. The
protocol body stays encrypted and stays undecoded here, for the reason the feasibility
report already gives: the key descends from a secret Cricut issues to its own authenticated
client, and this project does not pursue that path.

## 1. The machine enumerates as a USB CDC serial device

`ioreg -p IOUSB` shows one device and the OS exposes a serial node for it.

| Field | Value |
| --- | --- |
| USB product name | Cricut Maker 3 |
| USB vendor name | Cricut Inc |
| idVendor | 8403 (0x20D3) |
| idProduct | 40 (0x0028) |
| bcdDevice | 256 (0x0100) |
| USB serial number | 4402324072 |
| USB speed | 1 (full speed) |

The OS creates a CDC serial callout node for it:

```
/dev/cu.usbmodem44023240721   (USB CDC, the wire this report is about)
/dev/tty.usbmodem44023240721
/dev/cu.Maker3-B8A3           (the same machine's Bluetooth RFCOMM node)
/dev/tty.Maker3-B8A3
```

The node name embeds the USB serial number (`4402324072`) and the interface index. This is
ordinary USB CDC-ACM enumeration: no driver, no key and no Cricut software is needed to see
the device, its vendor and product ids, and its serial. That matches the feasibility
report's recommended first milestone, which is to enumerate and confirm the physical link
without a session (`real-device-feasibility.md` section 4).

## 2. The process and transport model, observed live

Design Space is Electron. With the Maker attached and the app open, two native plugin
processes run as children of the app, exactly as the static analysis described:

```
CricutDeviceIO   .../plugins/cricut-device-io/CricutDeviceIO      (the transport)
CricutDevice     .../plugins/device-common/CricutDevice --new-comm-lib   (the bridge)
```

The renderer talks to these plugins over stdio pipes (unix sockets on fd 0/1/2 in `lsof`),
never over the serial port directly. CricutDeviceIO owns the serial node; the bridge frames
the protocol. The serial node itself was not held open by any process while the app sat
idle at its home screen, which fits a transport that opens the port on demand for a Make.

## 3. The framing on the Electron-to-bridge pipe, captured live

The bridge writes a diagnostic log at
`.../plugins/device-common/logs/bridge.log`. With the machine attached it holds three
frames, captured here read-only. Each line is one base64-encoded frame. Decoding only the
header (not the body) shows a fixed structure:

```
magic  CC 01
length 4 bytes, little-endian, counts the body only
body   <length> bytes, high entropy
```

| Frame | magic | body length | body character |
| --- | --- | --- | --- |
| 0 | CC01 | 196 | ciphertext (printable ratio 0.35) |
| 1 | CC01 | 179 | ciphertext (printable ratio 0.30) |
| 2 | CC01 | 196 | ciphertext (printable ratio 0.39) |

The low printable ratio is what you expect from an AES-ECB body: near-random bytes, no
readable structure. This is the pipe between the Electron main process and the bridge, the
layer `evidence-handshake.md` section 1.3 describes, where the host-to-bridge key is a
constant the bridge also embeds. The on-wire serial framing between the bridge and the
machine (the cleartext `NewAES` request, then per-command AES-ECB with a checksum) is
documented from static analysis in `evidence-handshake.md` section 1.5 and is not
re-derived or exercised here.

## 4. What this confirms, and the wall it does not cross

Confirmed live, with zero protocol risk:

1. The Maker 3 is a standard USB CDC serial device. Its vendor id 0x20D3 and product id
   0x0028 identify it from the descriptor alone, before any software runs. A third-party
   app can enumerate it, name it and open its CDC node.
2. Design Space reaches it through a transport plugin that owns the serial node and a bridge
   that frames an encrypted protocol, over stdio pipes. The renderer never touches the port.
3. The framed traffic is encrypted. The captured frames are ciphertext.

The wall is unchanged from the feasibility report. Sending a real cut requires the encrypted
session, whose key descends from a server-issued secret tied to Cricut's authenticated
client. Observing the device and its transport is free; driving a cut is not reachable
without bypassing that authentication, which this project does not do.

Kerf's own serial support (`src/output/link.rs`, `src/output/runner.rs`) already drives
open cutters that speak cleartext HPGL or GRBL over exactly this kind of CDC node. Spec 58
adds detection of a plugged-in machine by its USB descriptor and states this boundary in
the UI: a recognised open cutter gets a ready-made profile, and a Cricut machine is named
and explained rather than pretended driveable.

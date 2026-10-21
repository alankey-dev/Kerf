# Driving a real Cricut Maker from Open Design Space: feasibility

Read-only investigation. No code was written and nothing was sent to a machine or to
Cricut's servers. The goal is interoperability with a Maker the user owns.

Sources and citation scheme follow the existing evidence docs in this repo:

- `evidence-native.md` is the static analysis of the native plugins under
  `/Applications/Cricut Design Space.app/Contents/Resources/plugins/` (arm64).
- The per-binary reports and working notes behind it are not kept in the repository, because they
  quote extracted third-party material (see `assets/PROVENANCE.md`).
- `evidence-handshake.md` traces the AES session and where the key material comes from.
- Native addresses are arm64 virtual addresses (image base `0x100000000`).

A blunt finding up front. The Maker's command protocol is encrypted, and the encryption
key descends from a secret that Cricut's server issues to the authenticated client. No
integration path sends cut data to the machine without that server-held secret. Reusing
the official plugin and reimplementing the protocol both hit the same wall. This report
explains the stack, pins the wall precisely, separates proven from unknown, and proposes
a first milestone that proves the physical link without touching the encryption or the
key material at all.

## 1. The layer stack, renderer down to the wire

Design Space is an Electron app. From top to bottom:

1. **Renderer (Angular, `main.js` / `chunk-*.js`).** Owns the UI, the material and tool
   catalogue, the VID/PID and Bluetooth-name tables, and every cloud call. It drives a
   Make by exchanging `PBInteractionStatus` values with the bridge (for example
   `riMATCUTNeedPathData=701` answered by `riMATCUTSetPathData=703`, carrying the SVG-like
   path string in `PBMatPathData.path_data`). Evidence: `evidence-native.md` section 4.

2. **Electron main process (`main.pretty.js`).** Spawns each plugin as a child process
   and relays messages. It also wraps the stdin pipe to the bridge in its own layer of
   AES before the frame reaches the plugin (`main.pretty.js:317895` onwards). The plugins
   are spawned with `spawn(path, args)` and the host's own environment
   (`main.pretty.js:317316`).

3. **The native plugins (child processes, stdin/stdout protobuf).** Four exist. Their
   roles, corrected from the original premise:
   - **`device-common/CricutDevice`** and **`device-common-next/CricutDevice`**, the C++
     "bridge" v6.12.13. This is the cutting-machine driver. It owns enumeration, open, the
     AES handshake, tool detection, mat load, vector streaming, and firmware update. It
     speaks the full Maker wire protocol. `evidence-native.md` section 0.
   - **`cricut-device-io/CricutDeviceIO`**, Swift/C++/ObjC, v3.18.0-alpha3. A generic
     multi-transport byte pipe. It has no machine knowledge, no opcodes, no crypto, and no
     Cricut identifiers. `report-deviceio.md` sections 3 and 4.
   - **`plugin-vulcan/CricutVulcanDevice`**. Not a cutter. It drives the Vulcan heat-press
     "pod + main unit" product over USB HID. Out of scope for the Maker. `notes-main.md`
     lines 11 to 16.

4. **The IPC framing (identical in all three cutter-relevant plugins).** A 4-byte
   little-endian uint32 length prefix followed by one serialised protobuf message, over the
   child's stdin/stdout. No sockets, XPC, or CFMessagePort in any plugin. For the bridge the
   message is `NativeModel.Bridge.PBCommonBridge`, reader `sub_10000814c`, writer
   `sub_100007c54`, run loop `sub_10000aa48` (stops on `method_action==13`). For
   CricutDeviceIO the message is `cricut.device_io.cli.Message`, `deserialize_message`
   `0x100003abc`, `serialize_message` `0x100003870`. Evidence: `report-devcommon.md`
   section 1, `report-deviceio.md` section 1.

5. **The transport to the machine.** CricutDevice itself opens USB CDC serial
   (`/dev/cu.usbmodem*` via `IOUSBHostDevice` to `IOSerialBSDClient`, `CSerialWorker`) and
   Bluetooth classic RFCOMM/SPP (`IOBluetoothRFCOMMChannel`, the machine's module is a
   Microchip RN4678). It links CoreBluetooth but uses no BLE here. CricutDeviceIO is the
   newer generic transport that does add BLE (CoreBluetooth, raw GATT), BLE-serial, USB CDC,
   RFCOMM, and TCP with `_cricut._tcp.local.` mDNS discovery. Evidence: `evidence-native.md`
   section 2, `report-deviceio.md` section 2.

6. **The device wire protocol.** Single-byte opcodes. Framing is family-dependent. The
   Maker family (machine type >= 11, which includes the Maker 3 = `SCAMANDER_MT` = 14) uses
   `[u16 big-endian length][opcode][payload]` (`0x10004f360`). The opcode table and payload
   layouts are in `evidence-native.md` section 3 (for example `DrawVectorFloat 0x42`,
   `MultiDrawVector 0x43`, `GetInterfaceState 0x73`, `GetMachineStatus 0x60`, `LoadMat 0x84`,
   `NewAES 0xC7`).

Which process speaks which layer. The renderer speaks `PBInteractionStatus` and path
strings. The Electron main process speaks the stdin-encryption wrapper. The CricutDevice
bridge speaks `PBCommonBridge` on its stdin/stdout and the encrypted single-byte opcode
protocol on the wire. CricutDeviceIO, when used, speaks only `cricut.device_io.cli.Message`
and moves opaque bytes. The machine firmware speaks the opcode protocol and enforces the
encryption.

## 2. Where cut path data is encrypted, and by whom. The crux.

**The machine enforces an encrypted command session.** Once a session key is installed,
the bridge AES-256-encrypts nearly every command before it goes on the wire (opcodes
0x45/0x46/0x47 and three exempt command classes bypass it), appends an 8-bit additive
checksum, and sends `[padded_len | 0x80][ciphertext]`. Responses are decrypted and the
checksum verified, or the bridge throws `CricutDevice::read_response(CRC Mismatch!)`
(`0x10037d1dc`). Encrypt routine `sub_1000394dc`. Evidence: `evidence-native.md` section 3,
`report-devcommon.md` section 2.

The firmware's own refusal codes confirm the machine, not just the host, requires this.
The firmware NACK enum includes `NO_AES_HANDSHAKE`, `UNKNOWN_ENCRYPTION_REQUIREMENT`,
`CRC_ERROR_DURING_DECRYPT`, `PREDECRYPT_SIZE_MISMATCH`, `CHECK_SUM_FAIL`
(`str-devcommon.txt` lines 4640 to 4840, cited in `evidence-native.md` section 6). A
machine that sees commands without a negotiated session answers `NO_AES_HANDSHAKE`.

**The path data itself is not separately server-encrypted end to end.** The bridge method
actions `ENCRYPT_AES_PATH=35` and `DECRYPT_AES_PATH=36` exist, and `PBMatPathData` carries
a `path_encrypted` flag, but no evidence ties them to a server-to-machine path encryption.
The only encryption that actually reaches the machine is the per-command AES wrapper above.
`report-devcommon.md` section 4. So the path string is the plaintext payload of
`DrawVectorFloat`/`MultiDrawVector` commands, which are then wrapped by the session cipher
like any other command.

**Who holds the key.** This is the wall. The per-command key is a session key the machine
chooses during the `NewAES` (0xC7) handshake. But the bridge can only interpret the
machine's handshake reply because it holds a master key, and that master key is **not**
computed locally. It comes from a per-user settings blob that the authenticated client
fetches once from Cricut's cloud (`GET https://apis.cricut.com/v4/Users/Settings`), inside
the `settings8` field. `evidence-handshake.md` sections 1.2 and 1.4. The blob is a
server-held secret, deliberately obfuscated, redistributed to the client through the user's
own login.

So, answering the crux directly and separating proven from unknown:

- **Proven.** The machine requires an AES-256 session before it accepts cut commands. The
  session key is established by the 0xC7 handshake. The bridge needs a master key to
  complete that handshake, and that master key arrives from Cricut's server in the user
  settings blob, not from local maths. There is no RSA, certificate, or signature check in
  any plugin; the barrier is a shared secret, not a public-key scheme.
- **Proven.** There is no separate per-cut server-issued token on the hot path. The
  1211 to 1214 "hello/web-request" round trip that looks like a per-open server call is dead
  plumbing in this build; the renderer never answers it and the bridge never emits it
  (`evidence-handshake.md` section 1.1). The key material is fetched once per login, and the
  settings blob is cached offline by the CricutLocalData plugin, so an already-signed-in
  install can open and drive the machine with no live network (`evidence-handshake.md`
  section 2, proven from code paths, not yet from a pulled cable).
- **Unknown.** Whether the machine answers any benign command (identity, status, interface
  state) **before** a session is negotiated. The bridge's encryption gate is skipped while
  no key is set (the `device+0x188 != 0` check in `sub_1000b4b98`), so the bridge would send
  such a command in cleartext, but the firmware may still refuse it with `NO_AES_HANDSHAKE`.
  This is the single most important thing to learn before any milestone, and it can only be
  settled with the machine in hand.
- **Unknown.** The exact derivation of the session key inside the handshake reply, the
  schema of the key table, and the per-entry version threshold. `evidence-handshake.md`
  section 5 and "Not determined".

The consequence for ODS. Driving a real Maker over its encrypted protocol requires the
Cricut server-held master key. Obtaining and using that secret means reproducing the
client's private unwrapping of data the vendor withholds on purpose. That is bypassing the
device's authentication and encryption. Per the investigation's own boundary, I will not
detail that, and I do not recommend building ODS on it. This is the decisive constraint for
every path below.

## 3. Candidate integration paths

| Path | What it is | Feasibility | Effort | Main risk | Blocked on |
|---|---|---|---|---|---|
| (a) Launch CricutDeviceIO, speak its protobuf | Reuse the generic transport pipe | Transport yes, Make no | Low for transport | Does nothing about encryption | You still must supply the whole encrypted device protocol and the server key yourself |
| (a') Launch the CricutDevice bridge as a black box | Reuse the real driver and let it do the crypto | Possible only with server key material | Medium | Depends on a server-held secret | The bridge refuses to work without the key table in `authData`; feeding it reproduces the blocked secret |
| (b) Reimplement transport + device protocol | ODS drives CDC/BLE and the opcodes itself | Not without the server key | High | Same wall, plus you implement the cipher | Same server-held master key |
| (c) Transport and enumeration only, no cut | Connect, enumerate, observe | Yes, and safe | Low | Little | Nothing; this is the recommended start |

**(a) ODS launches CricutDeviceIO and speaks its documented protobuf.** This is well
understood and the protocol is clean. The framing, the adapter-create and scan requests,
and `TransportSendBytesRequest`/`TransportReceiveBytesNotification` are all documented in
`proto-deviceio.proto` and `report-deviceio.md`. But CricutDeviceIO is only a byte pipe. It
has no opcodes, no handshake, and no crypto (`report-deviceio.md` section 3 found no
CommonCrypto, SecKey, HMAC, SHA, or AES anywhere in it). Using it gets you connected and
moving bytes, and nothing more. You would still have to implement the entire Maker opcode
protocol and the encrypted session on top. So path (a) by itself does not reach a cut; it
only solves the transport. It is worth reusing for exactly that, see the milestone.

**(a') ODS launches the real CricutDevice bridge as a black box.** This is the more
tempting version, because the bridge already contains the handshake and the cipher. In
principle ODS could spawn the installed bridge, send `STARTUP_BRIDGE`, enumerate, open, and
feed it path data, letting the bridge talk to the machine. The problem is that the bridge
will not establish a session without the server key table. It reads that table from
`PBCommonBridge.auth_data` on the first start message and holds it for the process lifetime
(`evidence-handshake.md` section 1.4). It also expects its stdin frames wrapped in the
Electron main process's own AES layer. Making ODS supply a working `auth_data` means
reproducing the client's secret handling of server-issued material. That is the blocked
path. I will not detail it, and a design that depends on it is not a sound foundation for
an open-source project: the secret is not ODS's to hold, it can be rotated by the server at
any time, and shipping it would put the user's account credentials and Cricut's secret on
the hook.

**(b) ODS reimplements the transport and the device protocol directly.** The transport is
tractable. The opcode table, framing, and many payload layouts are already mapped
(`evidence-native.md` section 3). The connectivity and the cleartext handshake request are
reproducible. But the moment you need the machine to accept a cut command, you need the
AES session, and the session needs the server master key. Same wall as (a'), plus you now
own the cipher implementation too. Higher effort, same blocker.

**(c) Transport and enumeration only.** The route that is both feasible and clean. Connect
to the machine, enumerate it, read whatever identity it advertises at the transport level,
and observe traffic, all without a session and without any key material. This is the
recommended first step and is detailed in section 4.

**Legal and terms-of-service note, high level only, not legal advice.** Interoperability
with hardware you own is the sympathetic case, and some jurisdictions carve out
reverse-engineering for interoperability. Against that: Cricut's software terms and account
terms typically prohibit reverse-engineering and unauthorised access, extracting and
redistributing a server-held secret key is a separate and more serious matter than
connecting to your own device, and driving stepper motors and a blade from unofficial code
can damage the machine and voids warranty. Paths (a') and (b) depend on the secret and
carry the most exposure. Path (c) connects to the user's own hardware and sends nothing
secret. Anyone proceeding past (c) should take their own legal advice.

## 4. Recommended first milestone: prove the physical link, no material, no session

Prove that ODS can open the transport to the user's own Maker and exchange bytes with it,
with no mat and no material loaded, without establishing an encrypted session and without
any server key material.

Concretely, the smallest safe proof:

1. **Enumerate and connect.** Over USB, open the CDC serial node the Maker exposes
   (`/dev/cu.usbmodem*`, from `IOSerialBSDClient`, `evidence-native.md` section 2). Reuse
   CricutDeviceIO as the transport (path (a)) or open the port directly; either is fine at
   this layer. Over Bluetooth the Maker 3 generation also advertises BLE; CricutDeviceIO's
   unfiltered scan and raw GATT give a path, but USB is the simpler first target.
2. **Observe, do not drive.** Watch for any unsolicited bytes or a button-press
   notification. Do not send motion commands. Reading the machine's advertised USB
   descriptor (vendor id, product id, serial) proves enumeration with zero protocol risk.
3. **At most, send the cleartext handshake request and observe the reply exists.** The
   `NewAES` request (opcode 0xC7, framed `00 31 C7` then 48 bytes) is sent before any key is
   set, so it goes in cleartext (`evidence-handshake.md` section 5). Seeing a reply of the
   expected shape proves bidirectional framing works. Do not attempt to decrypt the reply:
   that needs the master key, which is the blocked path. Observing that a reply arrives is
   enough to prove the link.

That milestone connects, enumerates, and confirms two-way framing. It does not move the
carriage and does not read firmware identity through an encrypted command, because both of
those likely require the session.

**What is still unknown before even this milestone, and must be learned with the machine
present:**

- Whether the firmware answers any command, or even the enumeration/identity surface,
  before a session exists, or whether it stays silent until `NewAES` succeeds. The bridge
  would send a pre-session command in cleartext, but the firmware may refuse with
  `NO_AES_HANDSHAKE`. This decides whether step 2 or step 3 yields anything.
- The actual USB VID/PID and the BLE service/characteristic UUIDs of the Maker 3. These are
  in none of the native plugins; they are host-side JS data and were not extracted here
  (`evidence-native.md` section 8). They are needed to recognise the device during
  enumeration, though an unfiltered scan sidesteps that.
- Whether moving the carriage is reachable at all without the session. On current evidence
  it is not, because motion commands are encrypted once the session is on and are refused
  without it. So "move the carriage" is **not** a safe no-session milestone; connect,
  enumerate, and observe-the-handshake-reply is.

Past this milestone, any step that makes the Maker accept a cut command runs into the
server-held key. That is where the honest answer is to stop and tell the user plainly: the
machine's encryption is keyed by a secret Cricut issues to its own authenticated client,
and building ODS to send real jobs requires that secret, which means bypassing the device's
authentication. I did not pursue it and do not recommend it.

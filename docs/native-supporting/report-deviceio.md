# CricutDeviceIO (cricut-device-io plugin) findings

Binary: /Applications/Cricut Design Space.app/Contents/Resources/plugins/cricut-device-io/CricutDeviceIO (arm64 slice, sha256 33adcac6...8073). `--version` prints "3.18.0-alpha3". Provider: Hopper via rea. Pseudo-code extracts live in scratchpad/pseudo/.

## 1. Host IPC framing

Each message is a 4-byte native-endian (little-endian on arm64/x86_64) uint32 length prefix followed by a serialized protobuf `cricut.device_io.cli.Message` (top-level oneof at proto-deviceio.proto line 675: LogNotification=1, SetLogLevelCommand=2, the *AdapterCreate/List* requests and responses, plus the AdapterMessage/TransportMessage branches). Evidence:

- `deserialize_message(std::istream&, std::string&)` at 0x100003abc: `istream::read(buf, 4)` ("Couldn't read size for the incoming message!"), `size = *(int32_t*)buf`, `resize(buf, size)`, `istream::read(buf, size)` ("Couldn't read message with size "), then `MessageLite::ParseFromArray` into `cli::Message` ("Couldn't parse incoming data to protobuf message"). Returns optional<Message>.
- `serialize_message(std::ostream&, std::string&, Message const&)` at 0x100003870: `SerializeToString`, `uint32 n = size; ostream::write(&n, 4)` ("Couldn't write message header with size "), `ostream::write(data, n)`, `flush()` ("Failed to perform IO operations on output stream.").
- `main_run_loop(std::ostream&, std::istream&, bool)` at 0x100003da8: `init_crashpad()`, `crashpad_simulate_crash()` if the bool is set, constructs `cli_context_t` with a send lambda (`$_0::operator()` at 0x1000048e0) that locks a `std::mutex` and calls `serialize_message`, then loops `while (deserialize_message(in, buf)) cli_context_t::on_message_received(msg)` until EOF/error. No handshake, no magic.
- Streams: `main` (entry 0x10000400c, pseudo/main.txt lines 70-100) does `ios_base::sync_with_stdio(false)`, unties cin, builds a fresh `std::ostream` over `std::cout`'s streambuf and a fresh `std::istream` over `std::cin`'s streambuf, and passes them to `cricut::device_io::cli::run_loop(std::function<void()>)` (0x10001b7f0) which calls `main_run_loop(out, in, crashFlag)`. The host talks over the child process's stdin/stdout. stderr carries the plain-text diagnostics above.
- Arguments: `main` checks `argc >= 2` and switches on `strlen(argv[1])`. Length 9 == "--version" prints "3.18.0-alpha3" and exits 0 (lines 50-66). Length 7 == "--crash" sets the crash-simulation bool and then runs the normal loop (lines 201-215). Anything else runs the normal loop. `--no-rate-limit`, `--%s=%d`, `--%s=%s` are crashpad handler strings, not parsed by main.
- No TCP listener, unix socket, or XPC service on the host side: nm has no undefined `_listen`, `_accept`, `_bind`, `_xpc_connection_*` or `NSXPC*` (only two libswiftXPC runtime link names). `socketpair`, `_get_exception_ports` strings are crashpad. asio `ip::tcp` is used only for the outbound machine transport (`ip_tcp_transport_impl_t::connect_with_timeout` with `basic_resolver::async_resolve` + `range_connect_op`).

## 2. Machine transports

### (a) BLE (CoreBluetooth; Swift modules btle_corebluetooth and CBTLECoreBluetoothCommon)

- No hard-coded UUIDs. No string matches a 128-bit UUID or a 4-hex 16-bit UUID. TX/RX characteristic UUIDs arrive in `BluetoothLeSerialTransportParams.tx_characteristic_uuid` / `rx_characteristic_uuid` (bytes, proto lines 197-198) and are validated in C++: "invalid TX characteristic UUID" (0x1001cbad8), "invalid RX characteristic UUID" (0x1001cbaf7), "connection failed, TX or RX characteristic UUID is missing" (0x1001cd821). The raw `BluetoothLeTransport*` family (DiscoverServicesAndCharacteristics, Read/Write/SetNotifyCharacteristic, GetAttMtu) lets the host drive GATT itself. Confirmed: UUIDs are host-supplied.
- Scan filtering: `bluetooth_le_central_impl::start_scanning()` at 0x100097c08 clears its cache and calls `_btle_adapter_start_scanning()` (0x100173dec), whose assembly passes `x0=0, x1=0` (`Optional<ScanningOptions>.none`) to `btle_corebluetooth.Adapter.startScanning(options:)` (0x10016d09c). With no UUID array that function calls `[central scanForPeripheralsWithServices:nil options:{CBCentralManagerScanOptionAllowDuplicatesKey: ...}]` (pseudo/adapter-startScanning.txt lines 76-87, `r21 = 0x0`). Scanning is unfiltered by service UUID. A `_btle_adapter_start_scanning_with_options` entry (0x100173ce4) can carry `btle_service_uuids_t`, but the CLI path does not use it.
- No local-name prefix filter and no manufacturer-data parsing. `AdvertisementData.init(localName:manufacturerData:serviceUUIDs:)` stores the three fields and they are copied verbatim into `BluetoothLeSerialDeviceDescriptor { device_id_high, device_id_low, optional local_name = 3, optional bytes manufacturer_data = 4, repeated bytes service_uuids = 5 }` (proto lines 167-174) and `BluetoothLeDeviceDescriptor` (lines 419-425) for the host to filter. No "Cricut"/"Maker"/"Explore" strings exist outside proto and plugin names (pseudo/machine-strings.txt: only "CricutPlugin", "CricutDeviceIO", "_cricut._tcp.local.", "cricut_device_io.proto").
- Device identity: CBPeripheral identifier UUID split into device_id_high/low (`CBPeripheral.btleDeviceId` extension, closure at 0x100173014). Optional `session_id` in the adapter create requests feeds CBCentralManager state restoration ("No session ID provided for BTLE adapter, session restoration feature will be unavailable", 0x1001cd67a).

### (b) USB serial

- Enumeration: `-[UsbSerialDeviceInquirerDelegate runWorker:]` at 0x10007ef00 calls `IOServiceMatching("IOUSBDevice")`, `IONotificationPortCreate(kIOMasterPortDefault)`, adds the run-loop source, `IOServiceAddMatchingNotification(port, "IOServiceFirstMatch", matching, deviceAdded, self)`. `deviceAdded(void*, io_iterator_t)` at 0x10007ea0c iterates, registers `IOServiceAddInterestNotification(..., "IOGeneralInterest", deviceRemoved, ...)` per service, then fires the generic `deviceUpdateCallback`. The matching dictionary has no VID/PID keys. "idVendor"/"idProduct"/"IOUSBHostDevice"/"IOUSBHostInterface"/"IOSerialBSDClient"/"IOCalloutDevice" belong to the bundled libusbp (`_libusbp_serial_port_create` at 0x1001630a8 references "Could not find entry with class IOSerialBSDClient." 0x1001d48ea), which walks IOUSBHostDevice to the IOSerialBSDClient child and reads IOCalloutDevice, i.e. a `/dev/cu.*` CDC-ACM path. CDC serial, not HID (no IOHID symbols; "failed to open serial port: " 0x1001cc99c).
- No hard-coded VID/PID: search for 0x2E24/"2e24" found nothing. `UsbSerialTransportParams { serial_number, vid, pid, interface_number, composite }` (proto 258-266) come from the host per connection; `UsbSerialDeviceDescriptor { vendor_id, product_id, revision, serial_number, os_id }` (238-244) is reported for every USB device, so Cricut-VID filtering is in the Electron host.

### (c) Bluetooth classic (IOBluetooth RFCOMM)

- `-[cdio_ChannelDelegate connectDeviceTask:]` at 0x100080c28 (pseudo/cdio-connectDeviceTask.txt): "connecting with timeout=", clamps ("connect timeout exceeds the HCI page timeout limit (~40.9 s), clamping" 0x1001cce26), pairs if needed via `cdio_PairingDelegate` (IOBluetoothDevicePair; "pairing did not complete within the 120 s safety timeout, aborting"), then `[device performSDPQuery:nil uuids:@[[IOBluetoothSDPUUID uuid16:0x1101]]]` (SPP class 0x1101), chooses the SPP record by `spp_name` or "no SPP service name specified, using the first available one: ", `getRFCOMMChannelID:` then `openRFCOMMChannelSync:withChannelID:delegate:`. Data via `rfcommChannelData:data:length:`, `rfcommChannelOpenComplete:status:`, `rfcommChannelClosed:`.
- PIN: `BluetoothSerialTransportParams.pin_code` (proto 319) answered in `devicePairingPINCodeRequest:` ("PIN code requested, replying with the configured PIN" 0x1001cd019, `replyPINCode:PINCode:`). Inquiry via `cdio_InquiryDelegate` (IOBluetoothDeviceInquiry) yields `BluetoothSerialDeviceDescriptor { address, name, timestamp_ms, paired, likely_present, rssi }`.

### (d) IP/TCP

- Discovery: `cli::ip_tcp_adapter_t::start_scan()` at 0x10000d874 calls `tcp_ip_mdns_device_inquirer::start_inquiry("_cricut._tcp.local.")` (string 0x1001cbac4, xrefs 0x10000d89c). Resolver is the bundled mdns.c single-header library (`_mdns_discovery_send`, `_mdns_query_recv`, `_mdns_record_parse_ptr/srv/txt/a/aaaa`, `mdns::impl::handle_response` 0x100165a20), not NSNetService/dns-sd. Port comes from the SRV record (`std::to_string(port)` in the lambda at 0x100095394) into `IpTcpDeviceDescriptor.port` (string) and `port_number` (uint32); no default port constant.
- TXT keys: the lambda at 0x100095394 looks for a 4-byte key equal to 0x65707974 ("type") and a 13-byte key; "serial_number" is the only 13-character TXT-like string in the binary and matches `IpTcpDeviceDescriptor.serial_number` (inferred, the literal is compared through a pointer). All TXT pairs are also forwarded verbatim as `repeated IpTcpDeviceTxtRecord txt_records` (proto 116) with ipv4/ipv6 address and name.
- Connect: `IpTcpTransportParams { address, port (string), optional enable_keepalive, keepalive_idle_seconds, keepalive_interval_seconds, keepalive_probe_count }` resolved by asio `basic_resolver<tcp>::async_resolve` and `range_connect_op` in `ip_tcp_transport_impl_t::connect_with_timeout`.

## 3. Encryption / challenge-response / signing

None in this binary. nm has no CommonCrypto, Security.framework, SecKey, HMAC, SHA or AES references; the only "crypt"/"sign" hits are protobuf internals, crashpad's `base::RandBytes` (/dev/urandom) and the enum name `BLUETOOTH_LE_CHARACTERISTIC_PROPERTY_AUTHENTICATED_SIGNED_WRITES`. Transports are byte pipes (`TransportSendBytesRequest` / `TransportReceiveBytesNotification`). Pairing is OS-level only:

- BLE: `_btle_device_pair` at 0x1001742dc is `mov w0, #0` (returns false); can_pair/is_paired/unpair are the same family, and the CLI answers "Pairing unsupported." (0x1001cbee1) / "Unpairing unsupported." (0x1001cbf06). CoreBluetooth on macOS has no pairing API, so BluetoothLeTransportPairRequest is a no-op here. `force_unpaired` only logs "forcing device to be in an unpaired state".
- Bluetooth classic: IOBluetoothDevicePair with optional host PIN, per 2(c).
- Any Cricut-specific challenge/response or firmware signing therefore lives in the Electron host or the other plugin.

## 4. Which machines

No machine names or codenames in the binary (no "Maker", "Vulcan", "Venture", "Explore", "Joy", "EasyPress"; see pseudo/machine-strings.txt). The plugin is generic: IpTcp (mDNS `_cricut._tcp`), BluetoothLeSerial, BluetoothLe raw GATT, UsbSerial CDC, BluetoothSerial RFCOMM/SPP, with every identifier (UUIDs, VID/PID, SPP name, PIN, TXT keys) supplied or interpreted by the host. The `_cricut._tcp` and raw-GATT adapters point at the Wi-Fi/BLE generation (Maker 3 / Explore 3 / Venture / Joy Xtra), but that is inference from feature set, not strings. Use the host JS (machines-table.txt, proto-vulcan.proto) for the mapping.

## Not determined

- The exact 13-byte TXT key literal (compared via pointer); "serial_number" is the only plausible candidate.
- Whether the host ever passes `--crash` in production.
- `Message` oneof numbers above field 62 rely on the pre-existing reconstructed proto; the descriptor was not re-dumped.

# Maker 3 AES handshake, offline behaviour, and third-party feasibility (evidence)

Read-only investigation of Design Space 9.68.73 on macOS. Sources and citation scheme:

- `main.js:N` and `chunk:N` are line numbers in the pretty-printed renderer bundles `scratchpad/pretty/main.js` and `scratchpad/pretty/chunk-JVMBAEXI.js`. `JAGV:N` is `scratchpad/pretty/chunk-JAGVFCCQ.js`.
- `main.pretty.js:N` is the deobfuscated Electron main process.
- Native addresses are arm64 virtual addresses in `/Applications/Cricut Design Space.app/Contents/Resources/plugins/device-common/CricutDevice` (bridge 6.12.13, sha256 b4b19665...). Pseudo-code came from rea (Hopper); the full disassembly is `scratchpad/dis-devcommon.txt`, the handshake slice is `scratchpad/dis-handshake.txt`, and the decompiled routines are `scratchpad/pseudo-opendevice.c`, `scratchpad/pseudo-openupdate.c`, `scratchpad/pseudo-blobdecrypt.c`.
- "Proven" means the code path was read end to end. "Inferred" means a step was reconstructed from surrounding evidence and should be confirmed with a live capture.

## 0. Summary

1. The status pairs 1211/1212 and 1213/1214 (`riOPENDEVICEEncryptionGetHelloWebRequest` and friends) are dead plumbing in this build. The renderer never answers them and the bridge never emits them; the two base64 fields the bridge stores on 1212/1214 are read only by the destructor (section 1.1).
2. The real key material arrives before any machine is opened, inside `authData` (`PBUserSettings`), which the renderer fetches once per login from `GET https://apis.cricut.com/v4/Users/Settings` (section 1.2). The bridge uses `settings8`, which is a server-issued, per-user obfuscated blob containing a table of per-machine-type AES-256 master keys (section 1.4).
3. The NewAES exchange (0xC7) is: host sends 48 random bytes, machine replies with 48 bytes, bridge AES-256-ECB decrypts those 48 bytes with the machine-type master key, and the plaintext contains the 32-byte session key at an offset given by its first byte (section 1.5). The session key is per open session; the master key is per machine type, and for some table entries it is personalised with five characters of the machine serial.
4. Offline: the whole `PBUserSettings` object is cached by the CricutLocalData plugin in sqlite under `~/Library/Application Support/LocalData` and replayed when the network store says offline, so a Maker 3 can be opened and driven with no internet once the account has been online on that install (section 2). This is proven from code paths, not from a live offline run.
5. A third-party app needs one authenticated `GET /v4/Users/Settings`, the `settings8` XOR unwrap, the key-table decode, the NewAES decrypt, and the per-command AES-ECB plus 8-bit checksum wrapper (section 3). Three constants the bridge embeds are listed.
6. No Maker 3 is attached to this Mac right now (section 4).

## 1. The handshake, end to end

### 1.1 What the renderer and the bridge do with 1211 to 1214 (nothing)

Renderer. The only decoder of bridge statuses is `convertMessageFromDevice` (chunk:163407 onwards). The four encryption statuses sit in one long fall-through case list together with about eighty other statuses (chunk:163880 to 163883, in the group that starts at chunk:163855), and that group does not touch `webRequest`, `devicesHelloRequest` or either base64 field. The only renderer code that consumes `PBWebRequest.action` is firmware download: `sourceUrl: o.webRequest.action` for `riFWUPWaitingOnDownloadFirmware` (chunk:163544) and `this.$http.get(p.webRequest.action, {responseType:"arraybuffer"})` (chunk:250121). The strings `devices/hello`, `helloKey` and `KeyResponse` occur only inside protobuf codec code (chunk:18830 to 28723), never in a handler. Searches for `nonce`, `machineKey`, `deviceKey` and `cachedKey` hit only Angular CSP nonces (JAGV:1618) and the machine-connection store key (chunk:234312).

Bridge. `OpenDeviceInteraction::update` (sub_100079ad0) switches on the incoming status: `0x4bc` (1212) copies `base64_devices_hello_key_response` into interaction+0x868 and `0x4be` (1214) copies `base64_devices_key_response` into interaction+0x880 and sets the flag at +0x899 (`pseudo-openupdate.c` lines 303 to 343; disassembly 0x100079b98 to 0x100079f58). Every other load of `+0x868` or `+0x880` in the binary is the destructor freeing them (0x10006f24c, 0x10006f25c, inside the object at vtable 0x1004397a8). No instruction anywhere stores 1211 (`0x4bb`) or 1213 (`0x4bd`) into a status field (grep of the full disassembly; the only `0x4bb`/`0x4bd` immediates are protobuf literal-pool offsets at 0x100274118 and 0x1002246f0). Conclusion, proven: in bridge 6.12.13 the hello/key web round trip is not used. The `devices/hello` endpoint name is not present in any artefact; `PBWebRequest { action, parameters, method, body }` is populated only for firmware downloads.

### 1.2 Where the key material really comes from: `GET /v4/Users/Settings`

The renderer attaches `authData` to every bridge start message: `DeviceGetStartMessage` sets `t.authData = yield this._nativePluginDataEndpoints.getUserBridgeSettings()` (main.js:323762 to 323768); the Make flow does the same with `authData: t.userBridgeSettings` (chunk:165896, 165902, 165999). `getUserBridgeSettings` resolves to `srvcSessionData.GetUserBridgeSettings()` (main.js:292069 to 292076).

`GetUserBridgeSettings` (main.js:284155 to 284190) issues one request:

- Method and URL: `GET ${API_ENDPOINT}/v4/Users/Settings` (main.js:284165 to 284166). `API_ENDPOINT` is `https://apis.cricut.com` for production (main.js:82056, 82143; also main.pretty.js:313981).
- Headers: `X-Cricut-Client` (built from `atob("WC1DcmljdXQtQ2xpZW50")`) set to `${APPLICATION.NAME}@${APPLICATION.VERSION}`, which is `Hyperion@9.68.73` on this install (config.decoded.json per `evidence-electron.md` section 5), and `X-Local-ID` (`atob("WC1Mb2NhbC1JRA==")`) set to `String(CLIENT_CLOCK_OFFSET)` (main.js:284159 to 284163). `withCredentials: true` (main.js:284168), so the session cookie goes with it.
- Authentication: the app logs in through Auth0 at `auth.cricut.com`, client id `RueSyWNI5K4vYT9xw7hcBVuKPj22BmvF`, callback `https://cricut.com/callback` (main.js:82079 to 82082). An HTTP interceptor is configured with `uriMatcher: (c) => o(c, a.API_ENDPOINT, i.EXCLUDED_URLS)` (main.js:419515), where `EXCLUDED_URLS` is `["GetAppSessionDataV2", "v4/Lookups/IsOnline", "/desktopdownload"]` (main.js:82084 to 82088). Inferred: that interceptor adds `Authorization: Bearer <Auth0 access token>` to every `apis.cricut.com` call except those three, so `/v4/Users/Settings` carries a bearer token as well as the cookie. The chatbot client shows the same token used explicitly (`Authorization: Bearer ${s}` after `this._authService.trySilentRefresh()`, main.js:272951 to 272966).
- Response: a JSON object whose keys are the `PBUserSettings` field names `setting1, setting2, settings1 ... settings11` (proto-devcommon.proto:2138 to 2152; renderer codec chunk:77959 to 77990). The renderer then overwrites one field before anyone sees it: `settings3 = USER_CRICUT_ID + "Hyperion" + "@" + "9.68.73"` (main.js:284177 to 284181, key via `atob("c2V0dGluZ3Mz")`). That client-side value is the XOR seed used in 1.3 and 1.4.

The promise is memoised for the session (`mUserSettingsPromise`, main.js:292070) and cleared by `ResetSessionData` (main.js:292173 to 292183) and on sign-out (`resetUserBridgeSettings`, main.js:419197).

### 1.3 Electron main: the `_eSal` salt and what it protects

`NativePluginDeviceCommonService` holds `_eSet = ["YWVzLTI1Ni1lY2I=", "YXV0aERhdGE", "c2V0dGluZ3My", "c2V0dGluZ3Mz"]` (`aes-256-ecb`, `authData`, `settings2`, `settings3`) and the 64-byte salt `_eSal` (main.pretty.js:317748 to 317759):

```
d6 b9 75 d7 82 37 a9 d8 73 7a 7b 39 cf 04 11 24 8a a1 d6 e4 e9 d5 ba 1c 28 b3 c6 0b a0 be c5 1e
2e e8 7b 69 d5 30 78 cf 40 09 b2 44 69 1b 40 20 ca fe d1 91 7e 7b bc 51 5d b7 76 2e 59 14 f7 f5
```

`start()` keeps `_eData = args.authData` (main.pretty.js:317849 to 317851). `request()` (main.pretty.js:317895 to 317958) does, with `H = sha512(settings3)` as raw 64 bytes and `_deSal(a, b) = a[i] ^ b[i % b.length]` (definition right after 317958):

1. If the outgoing message has `authData.settings8`, replace it with `utf8( base64decode(settings8) XOR H )` (main.pretty.js:317926 to 317939). This is the only field the main process unwraps before it reaches the bridge.
2. Host-to-bridge AES key `K_host = base64decode( utf8( (base64decode(settings2) XOR H) XOR _eSal ) )` (main.pretty.js:317941 to 317950).
3. Encode the `PBCommonBridge`, zero-pad to 16 bytes, AES-256-ECB with `K_host`, no auto padding, then the 4-byte LE length prefix (main.pretty.js:317951 to 317958).

So `settings2` and `settings3` only protect the stdin pipe between Electron and the bridge. They are not part of the machine handshake. Note that `K_host` is a constant known to the bridge (next section), so `settings2` is just that constant re-wrapped per user and per app version by the server.

### 1.4 Bridge: constants, the stdin key, and the `settings8` key table

Stdin decrypt. The reader `sub_10000814c` reads `[int32 LE len][bytes]` and, unless its third argument is non-zero, decrypts with `sub_10019a1bc(key, buf, len, &outlen)` before `ParseFromArray` (disassembly 0x1000082f8 to 0x10000837c). The key lives in the global `std::string` at 0x10045a140 and is built lazily by `sub_1000b5f74`: copy the 64-byte table at 0x1003d6338, append `0x3c`, then walk indices 0x40 down to 0 keeping the odd ones (rea pseudo-code of sub_1000b5f74; bytes read with rea at 0x1003d6338). That yields the 32-byte constant

```
K_host = ccc4e701bd7557cdd4f0761faaa8ef37a724771fef42ce3ab7f273d6910de44b
       = base64 zMTnAb11V83U8HYfqqjvN6ckdx/vQs46t/Jz1pEN5Es=
```

Proven: the bridge decrypts every host frame with this constant. Inferred: the server's `settings2` must therefore unwrap to the same value through the main-process steps above.

Where `settings8` lands. The bridge dispatcher `sub_10006a68c` logs `"client: {}"` and then reads `PBCommonBridge.auth_data` (field at +0x1e0, default instance 0x1004532a0) and its string at +0x58, and if it is non-empty calls `sub_1000b6940(string)` (0x10006a704 to 0x10006a738). The `PBUserSettings` parser maps `settings8` to offset 0x58 (literal `"NativeModel.PBUserSettings.settings8"` at 0x10015fa88 surrounded by loads of `[x20, #0x58]`; the sequence runs setting1 at 0x10 through settings11 at 0x70). `sub_1000b6940` stores the string into the global `std::string` at 0x100457ad0 only when that global is still empty (0x1000b6958 to 0x1000b6974, store at 0x1000b69d4 to 0x1000b69e8), so the first start message of the process fixes the key table for the process lifetime. The other `auth_data` field the bridge reads is `settings10` (+0x68, at 0x100068fdc and 0x100069080, passed to sub_1000af0ec); its purpose was not traced.

Decoding the table (`sub_1000b60c8`, `pseudo-blobdecrypt.c`). With `S` the unwrapped `settings8` string:

1. `B = base64decode(S[25 : 25+44])` (substring length checks at 0x1000b610c to 0x1000b6128, base64 via `sub_10019c738`, which is a base64 decoder by its `'='` padding handling).
2. `T = C2 || 0x3c || B` where `C2` is the 32 bytes at 0x1003d6379: `5c4c4dee8c0d589139d62a738df284b4 373ab0ce1252c8ef8e1fa17751351bb7` (pseudo lines 64 to 110).
3. `K_blob` = the even-index bytes of `T` taken from the last index downwards (pseudo lines 128 to 190). Inferred detail: AES-256 uses the first 32 of them.
4. Erase `S[25 : 69]` (`sub_10037694c(&s, 0x19)` at 0x1000b63cc with length 0x2c), base64-decode the remainder, AES-256-ECB decrypt with `K_blob` (`sub_10019a1bc` at 0x1000b643c), and `ParseFromArray` the plaintext into a protobuf (sub_1000b6ad0 via sub_10039e58c).

The parsed message (constructor sub_1000f39b4, vtable 0x10043a858) has three repeated fields; its schema name was not identified. Each entry of the first repeated field has: a machine type enum at +0x198, a string at +0x10 (key material), a string at +0x18 (a numeric threshold parsed with `stoll`), a bool at +0xd0, a bool at +0x1c0 and an int at +0x60.

Selecting the master key for the connected machine (`sub_1000b6ad0(machine_type, serial, &out)`):

- Walk the entries; take the first whose type equals `device+0x108` (`PBMachineType`, 14 for the Maker 3) and whose `stoll(threshold)` passes a floating-point comparison against a version value (rea pseudo-code lines 40 to 70; the exact operand was not resolved).
- Base64-decode the entry string, then `sub_1000b661c` keeps the odd or even bytes (flag at +0x1c0) scanning forwards or backwards (flag at +0xd0), producing the 32-byte `K_m`.
- If the entry int at +0x60 equals 5, `sub_100035d00(K_m, 32, serial)` overwrites `K_m[2], K_m[5], K_m[7], K_m[10], K_m[13]` with `serial[0], serial[2], serial[5], serial[9], serial[11]` (rea pseudo-code; requires serial length >= 12, which matches the `([A-Z])([0-9]{6})(\w)([0-9]{4})` serial regex at 0x10003a2fc).
- If nothing matched, fall back to the entry with type 7 (`ZORRO_MT`, proto-devcommon.proto:3066).

### 1.5 The 0xC7 NewAES exchange and the session key

`sub_1000b6e44` (0x1000b6e44 to 0x1000b7074, listing in `dis-handshake.txt` lines 723 to 897) is the handshake:

1. `device->create_command(6)` (vtable +0x140 with `w1 = 6`, 0x1000b6e68) builds `AthenaNewAESCommand` (factory id 6 at 0x10004b2b4, constructor 0x10004f278, vtable 0x100434960).
2. The command bytes (`get_bytes` 0x10004f360) are 0x33 bytes: `00 31` (big-endian length 49), opcode `C7`, then 48 bytes drawn from a 51-byte buffer of `RandomByte()` calls (sub_10004f428 calls sub_10004f5ec 51 times; `std::random_device` over `/dev/urandom` seeding a Mersenne Twister, 0x10004f7e4). The earlier native report records the byte after `C7` as `(rand % 12) + 2`; this pass did not re-derive that detail.
3. The response parser (0x10004f738) requires the first big-endian u16 to be `0x0030`, stores it as the status, and copies `0x30 + 2` bytes (slot +0x60 returns 2, function 0x10004f7dc). "New AES - Unexpected response received from device!" (0x10037d158) is thrown otherwise.
4. Back in the handshake, with status == 0x30 (0x1000b6f50), it computes `K_m` for `device+0x108` and `device+0x58` (serial) via `sub_1000b6ad0` (0x1000b6f7c), then `D = AES256_ECB_decrypt(K_m, response + 2, 48)` (0x1000b6fb8 to 0x1000b6fbc, `w2 = 0x30`).
5. Session key: `device->set_key(D + D[0])` (0x1000b6fd4 `ldrb w8, [x21]`, 0x1000b6fe0 `add x1, x21, x8`, call through vtable +0x58 which is `sub_10005a474`, the routine that mallocs 32 bytes and stores the pointer at device+0x188, 0x10005a4a4). Then `device+0x190 = 1` (0x1000b6ff0) switches encryption on.

Everything after that follows the per-command wrapper already documented in `evidence-native.md` section 3: pad to 16, append an 8-bit additive checksum, AES-256-ECB with the 32-byte key at device+0x188, emit `[padded_len | 0x80][ciphertext]`; opcodes 0x45, 0x46, 0x47 bypass it; responses are decrypted from byte 1 and the trailing sum is verified ("CricutDevice::read_response(CRC Mismatch!)", 0x10037d1dc).

Scope of the keys, answering the question directly:

- Session key (device+0x188): per open session. It is whatever the machine put in its 48-byte reply, so the machine chooses it (proven for the bridge side; the firmware side is inferred). A new NewAES exchange replaces it; `ZorroResetAESCommand` (0xCF, 0x10006221c) exists for tearing it down and its caller was not traced.
- Master key `K_m`: per machine type from the server table, optionally personalised with five serial characters. It does not change per cut.
- Nothing about the session key is written to disk by the bridge. The key table (`settings8`) is held in a process global for the bridge lifetime and comes from the renderer on every bridge start.

### 1.6 What is cached on disk

- Renderer memory: `mUserSettingsPromise` (main.js:292070), cleared on sign-out.
- CricutLocalData plugin: when `FEATURE.OFFLINE_V1` is on (it is `true`, main.js:84128) the full settings JSON is written with `RequestLocalDataSetUserSettings { userSettingsData: utf8(JSON) }` (main.js:292077 to 292091) and read back with `RequestLocalDataGetUserSettings` (main.js:292093 to 292109). The plugin has a `PBUserSettingsLD` message with the same thirteen fields (strings `NativeLocalData.PBUserSettingsLD.setting1 ... settings11` in `plugins/local-data/CricutLocalData`) and uses sqlite (`a.db`, `metadata.db`, `CREATE TABLE` strings in the same binary). The database root is `NATIVE.PATH_APPLICATION_DATA`, which main sets to the parent of Electron's `userData` directory, plus `LocalData` (main.pretty.js:324480 to 324490), i.e. `~/Library/Application Support/LocalData/...`. That directory does not exist on this Mac, so this install has never stored settings offline.
- Keychain: `security dump-keychain | grep -i cricut` returns nothing. The only Cricut files under `~/Library` are `Preferences/com.cricut.design.plist`, `HTTPStorages/com.cricut.design` (cookie jar) and the Electron singleton symlinks.
- Electron main: holds `authData` in `_eData` (main.pretty.js:317849) for the window lifetime; nothing persisted.

## 2. Offline behaviour

Network state. The renderer's network store starts from `navigator.onLine` (JAGV:10198) and is updated by `window` `online`/`offline` events, each confirmed with `this._healthCheck.checkOnline()` (JAGV:10210 to 10226), whose URL is `${API_ENDPOINT}/v4/Lookups/IsOnline` (main.js:279294). `_networkQuery.online` reads that store (JAGV:10248 to 10252).

Key material when offline. `GetUserBridgeSettings` is wrapped in `requestOnlineFallbackOffline(onlineFn, offlineFn)` (main.js:284485 to 284520): if the store says offline it calls the LocalData read directly; if online it tries the HTTPS call and, on failure with `OFFLINE_FALLBACK_ENABLED` (true, main.js:84127), falls back to the LocalData copy. `settings8` is in that JSON, so the bridge receives the same key table offline. The bridge itself never needs the network at open time (sections 1.4 and 1.5), and the machine supplies the session key. The LocalData start message only includes `userSettings` and `cricutId` when online (main.js:283093 to 283099), which is the write path; the read path above does not require that.

Validity window. No expiry is applied to the cached settings in the renderer; they survive until `ResetSessionData` or sign-out. The bridge's only time-like check is the per-entry numeric threshold in `sub_1000b6ad0`. Inferred: a cached `settings8` keeps working until the server rotates the table or ships a newer threshold, and a Maker 3 does not need a previously negotiated session key because it negotiates a fresh one on every open.

Other offline gates seen in the Make flow: the "Unable to Cut" offline message is only about fonts and images the account no longer has locally (`OFFLINE_UNABLE_TO_CUT_DESCRIPTION`, matpreview-components-i18n.txt:27 to 28); the machine-connection base class consults `isOnline()` only when switching between machine classes (chunk:244273); the post-open analytic summary skips the firmware check when offline (chunk:164570). Material and pressure data come from `GET /materials/Machines/{serial}/materials` (main.js:289136) and are outside this investigation.

Plain answer. Yes, the Maker 3 can be used with no internet at all, provided the account has been signed in online at least once on that install so the CricutLocalData sqlite holds the user settings (including `settings8`), and the project only uses assets already present locally. This is proven from the code paths and not yet demonstrated with the network cable pulled.

## 3. Feasibility for a third-party Rust app

What a one-time authenticated request yields: the `PBUserSettings` JSON, and specifically `settings8`, which contains the master key table for every machine type. Once decoded, the app can open the machine and negotiate session keys without any further cloud contact, for as long as the table stays valid (section 2). It should be stressed that this is a server-held secret being redistributed through the user's own account; keeping it on disk makes the user responsible for it.

What must be reproduced, in order:

1. Login. Auth0 authorisation code flow against `https://auth.cricut.com` with client id `RueSyWNI5K4vYT9xw7hcBVuKPj22BmvF` and redirect `https://cricut.com/callback` (main.js:82079 to 82082), then the app session (`GetAppSessionDataV2`, excluded from the bearer interceptor, main.js:82085). The exact audience and scopes were not extracted.
2. `GET https://apis.cricut.com/v4/Users/Settings` with `X-Cricut-Client: Hyperion@9.68.73`, `X-Local-ID: <clock offset seconds>`, the session cookie and (inferred) `Authorization: Bearer <token>` (main.js:284155 to 284170). Keep the response fields `settings8` and the account's Cricut user id. The `X-Cricut-Client` value matters because the server derives `settings3` dependent material from the same `cricutId + "Hyperion@" + version` string the client builds (main.js:284177 to 284181).
3. Unwrap: `S = base64decode(settings8) XOR sha512(cricutId + "Hyperion@9.68.73")` with the 64-byte digest repeated (main.pretty.js:317926 to 317939). The `_eSal` salt is not involved in `settings8`; it is only part of the `settings2` to `K_host` path, which a third-party app does not need because it never talks to the bridge.
4. Decode the key table from `S` as in section 1.4 (constant `C2` at 0x1003d6379, erase `S[25:69]`, base64, AES-256-ECB with `K_blob`), parse the protobuf, pick the entry with type 14 (`SCAMANDER_MT`) that passes the threshold, apply the odd/even and direction flags, and the serial personalisation if the entry's int field is 5. The message schema of the table is unknown, so this step needs one real `settings8` to pin down the field numbers.
5. Serial transport. Open the CDC device (`/dev/cu.usbmodem*`, IOSerialBSDClient per `evidence-native.md` section 2). Framing for the Maker family is `[u16 BE length][opcode][payload]` (0x10004f360). Send `00 31 C7` followed by 48 random bytes (byte after `C7` reportedly `(rand % 12) + 2`); expect a response whose first u16 is `0x0030` followed by 48 bytes (0x10004f738). Decrypt those 48 bytes with AES-256-ECB and `K_m`; the session key is the 32 bytes at offset `D[0]` of the plaintext (0x1000b6fd4 to 0x1000b6fe8). Firmware NACK names such as `NO_AES_HANDSHAKE` and `UNKNOWN_ENCRYPTION_REQUIREMENT` (str-devcommon.txt lines 4640 to 4840) are what you see if this step is skipped.
6. Every later command except opcodes 0x45, 0x46, 0x47: build `[u16 BE len][opcode][payload]`, zero-pad to a multiple of 16 leaving one byte, write the 8-bit sum of all preceding bytes as the last byte, AES-256-ECB encrypt with the session key, send `[padded_len | 0x80][ciphertext]` (sub_1000394dc). Responses: decrypt bytes[1..] with the same key, strip zero padding, verify the trailing 8-bit sum (sub_1000b307c, sub_1000b4744 for machine types >= 11).
7. Command set and Make sequence: opcode table and status flow in `evidence-native.md` sections 3 and 4 (SetMachineState 0x30, SetSpeedState 0x31, SetFullToolState 0x3e, ToolDetect 0xea, LoadMat 0x84, BeginDraw 0x21, DrawVectorFloat 0x42 / MultiDrawVector 0x43, EndDraw 0x22, GetInterfaceState 0x73, AbortCutting 0x68).

Duration. The session key lives until the machine is reset, power cycled, or another NewAES/ResetAES is issued (inferred from the firmware NACK names and the presence of 0xCF; not observed). The master key table lives until the server changes `settings8` for the account or the entry threshold no longer matches the machine; nothing in the client enforces a shorter window.

What a passive capture needs:

- Device path: on macOS the bridge opens the IOCalloutDevice, which is `/dev/cu.usbmodemXXXX` for a CDC-ACM device (no Cricut VID/PID is compiled into any plugin or into the renderer; the host receives `UsbSerialDeviceDescriptor { vendor_id, product_id ... }` and the `VidPid` filter list is server data, inferred because neither 0x2E24 nor any `vid:` literal exists in the JS; the only hardcoded USB ids, 0x302C/0x3101 and 0x20D3/0x2001 at main.js:466426 to 466428, belong to the separate "GeminiSDK" heat-press discovery).
- `ENABLE_BRIDGE_LOG_FILES=1` in the bridge's environment enables spdlog file output to `<plugin dir>/logs/bridge.log` (getenv at 0x1000f08a4, path from sub_1000f2af4, literal 0x1000f2b90). The same effect is available from the dev hub flag `DH_MACHINES_ENABLE_BRIDGE_LOG_FILES`, which adds feature flag `ENABLE_BRIDGE_LOG_FILES = 34` to the start message (chunk:165319 to 165322, enum chunk:140522). Caveat: plugins are spawned with the host's environment and no custom env (`spawn(path, args)`, main.pretty.js:317316 to 317324), so the env var has to be set on the Design Space process itself, for example by launching it from a terminal with the variable exported.
- What the log contains: the opcode only (`"sending command 0x{:02X}"`, 0x1000b4cbc) and the result status (`"command result: 0x{:02X} {}"`, 0x1000b366c), plus state dumps such as `SET MACHINE STATE`, `SET SPEED STATE` and `SET TOOL STATE` with their numeric parameters. No format string dumps whole frames in hex, so `bridge.log` will not give plaintext packets. The `PBLog` message sent to the host has a `command` and an `unencrypted_full` field (proto-devcommon.proto:750 to 757) and the host handles `GET_COMMAND_LOG_MA = 31` (main.pretty.js:34645); whether `unencrypted_full` carries the clear-text frame was not verified. That path is the most promising place to obtain plaintext without patching.
- For the wire bytes themselves, a USB sniff of the CDC endpoint captures only the encrypted stream after `C7`, which is still useful to confirm framing, the 48-byte nonce exchange and the `0x80` length flag. With the decoded `K_m` the capture can then be decrypted offline.

## 4. Is a Maker 3 attached right now

No. Read-only checks run on 12 October 2026:

- `ls /dev/cu.* /dev/tty.*` shows only `cu.Bluetooth-Incoming-Port`, `cu.debug-console` and their `tty.` twins. No `usbmodem` or `usbserial` node.
- `ioreg -p IOUSB -w0` lists three `AppleT8132USBXHCI` controllers with a single `SanDisk 3.2Gen1` device; `ioreg -p IOUSB -l | grep -i cricut` prints nothing.
- `system_profiler SPUSBDataType | grep -i cricut` and `system_profiler SPBluetoothDataType | grep -i cricut` print nothing.

So no vendor or product id could be read from hardware. Nothing was sent to a machine or to Cricut's servers.

## 5. Not determined

- The protobuf schema and field numbers of the key table inside `settings8`, and the operand of the per-entry version comparison in `sub_1000b6ad0`.
- The exact `Authorization` header on `/v4/Users/Settings` (bearer inferred from the interceptor configuration) and the Auth0 audience and scopes.
- The semantics of the first byte of the NewAES plaintext beyond "offset of the session key", and what the firmware does with the host's 48 random bytes.
- Whether `PBLog.unencrypted_full` carries whole frames.
- Which feature flags change the reader's unencrypted path (the third argument of sub_10000814c).
- `settings10` usage in the bridge (read at 0x100068fdc).
- `device-common-next/CricutDevice` was not separately decompiled; its string table is identical to `device-common`.

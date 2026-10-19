# Cricut Design Space 9.68.73: Electron main process, plugins and IPC

## Method and citation scheme

`main.js`, `preload.js` and `preload-plugin.js` are protected with javascript-obfuscator: there is a rotated string array (`a0_0x542d`), a decoder (`a0_0x51a5`, defined at byte 7640680 of the original `main.js`) and about 12,000 local aliases of that decoder. I wrote `tools/deob.js`. It evaluates only the array, the decoder and the rotation IIFE in a `vm` context, follows alias chains, and inlines 136,292 decoder calls as string literals. Prettier 3.3.3 then formats the result.

Files in the scratchpad:
- `tools/deob.js` is the deobfuscator, which can be rerun. Usage: `node tools/deob.js <in> <out> <arrayFn> <decoderFn>`.
- `main.deob.js` and `main.pretty.js` (343,800 lines) are the deobfuscated and formatted main bundle.
- `preload.pretty.js` and `preload-plugin.deob.js` are the deobfuscated preloads.
- `tools/decbin.js` and `config.decoded.json` are the config.bin decryptor and its output.
- `ipc_regs.txt` lists all 137 ipcMain registrations (line, method, channel).
- `channels.txt` holds the plugin channel enum.

Unless a citation says otherwise, `main.pretty.js:N` means line N of the formatted file. Literal strings also have byte offsets in the original `asar/cricut-ele-design-space/main.js`, but those offsets point into the string table, not the call site. For example, `--new-comm-lib` is at byte 7720971 and `YWVzLTI1Ni1lY2I=` is at byte 7822668. Renderer code (`asar/design-space-app/*.js`) is minified but not obfuscated, so it is cited by byte offset.

## 1. How the main process launches native plugins

**Path table.** `main.pretty.js:316459-316469`, for the packaged build (TYPE != SOURCE). The root is `PATH_PLUGIN_ROOT = <bin>./Resources/plugins` (`main.pretty.js` about 316370).

| Key | Path | Service class (logger name) | Mode |
|---|---|---|---|
| CricutDevice | `./device-common/CricutDevice` | NativePluginDeviceCommonService | STREAM, one process per window |
| CricutDeviceNext | `./device-common-next/CricutDevice` | same service, used when flag `DH_MACHINES_USE_BRIDGE_NEXT` is set | STREAM |
| CricutDeviceIO | `./cricut-device-io/CricutDeviceIO` | NativePluginCDIOService | raw byte pipe, per window or shared |
| CricutVulcanDevice | `./plugin-vulcan/CricutVulcanDevice` | NativePluginVulcanService | STREAM |
| CricutPathOperation | `./path-operation/CricutPathOperation` | NativePluginPathOperationService | one-shot process per request |
| CricutSystemFont | `./system-fonts/CricutSystemFont` | (system font service) | one-shot; `--system0` prints system info |
| CricutSystemPrinter | `./system-printer/CricutSystemPrinter` | (printer service) | one-shot or multi-response |
| CricutLocalData | `./local-data/CricutLocalData` | (local data service) | ASYNC, long-lived, request IDs |
| CricutMagick | `./canvas/cricut-magick` | used by the printer service (`getMagickPath`, `main.pretty.js:326523`) | |
| CricutCanvas | `./canvas/CricutCanvas` | appears in the path table only. I found no spawn site in main.js, so the renderer or another path probably uses it. Unverified. | |

**Spawn calls.** All plugins start through `require("child_process").spawn` (`main.pretty.js:316562`). None of them pass a custom env or cwd.
- In `startWindowPlugin(windowId, args)` at `main.pretty.js:317316-317324`, the call is `spawn(this._pluginExecPath, args)`. The process is keyed by `event.sender.id`, the webContents id, so each window or tab gets its own process. `stopNativeZombieProcesses()` SIGKILLs any orphans.
- CricutDevice runs with `["--new-comm-lib"]` (`main.pretty.js:317852`): `await this.startWindowPlugin(_0x4b4ca6, ["--new-comm-lib"])`.
- The `--version` probe runs via `runWithParams(["--version"])` (`main.pretty.js:317661`).
- The system info probe is `spawn(<CricutSystemFont>, ["--system0"])` (`main.pretty.js:317140`). It parses `key: value` lines from stdout into HARDWARE info such as `PHYICAL_MEMORY` and `OS_VERSION`.
- CricutDeviceIO is spawned at `main.pretty.js:325844-325851` and `325908-325915`. With the `NATIVE_CDIO_PLUGIN_LOGS` flag set, the options are `{windowsHide: true, stdio: ["pipe","pipe","pipe"]}`. Otherwise the call is a bare `spawn(path)`. When the flag `DH_MACHINES_ENABLE_SINGLE_INSTANCE_CDIO` is set, one shared process serves all windows (`_sharedPluginProcess`).
- One-shot plugins (PathOperation, SystemPrinter and others) use `sendPluginRequest` (`main.pretty.js:319363-319470`). It spawns a fresh process per request, writes one framed message, collects stdout until `close`, and kills the process after `DH_APPLICATION_NATIVE_REQUEST_TIMEOUT`.

**Transport.** The transport is the child's stdin and stdout pipes only. There is no TCP, WebSocket, named pipe or gRPC. The only `http.createServer` in main is the auto-update server on `http://127.0.0.1:<port>` (`main.pretty.js:323854-323858`), which hands the update zip to Squirrel.Mac.

**Framing.** Each message is a 4-byte little-endian length followed by a protobuf payload.
- Write side, `sendData` (`main.pretty.js:317462-317500`):
  ```
  const _0x19eeff = Buffer.alloc(4);
  _0x19eeff.writeInt32LE(_0xf031f9.length, 0); stdin.cork(); stdin.write(_0x19eeff); stdin.write(_0xf031f9); nextTick(uncork)
  ```
- Read side, `onProcessData` (`main.pretty.js:317527-317589`): `readUInt32LE(0)`, wait until `len+4` bytes have arrived, slice, call `parseMessageData`, then recurse. The one-shot variant does the same at `main.pretty.js:319448-319455`.

**Payload schemas.** These are protobufjs static modules bundled inline (`_0xbd1b65.NativeModel...`).
- CricutDevice uses `NativeModel.Bridge.PBCommonBridge` for both requests and responses (`main.pretty.js:317788` decode, `317904` fromObject, `317951` encode). The type is defined at `main.pretty.js:24437`. Its package is `@cricut/bridge-proto ^6.12.4` (per config.bin).
- **Requests to CricutDevice are encrypted** with AES-256-ECB, no auto-padding, zero-padded to 16 bytes (`main.pretty.js:317926-317958`). The constructor holds `_eSet = ["YWVzLTI1Ni1lY2I=", "YXV0aERhdGE", "c2V0dGluZ3My", "c2V0dGluZ3Mz"]`, which decodes to `aes-256-ecb`, `authData`, `settings2` and `settings3`. It also holds a 64-byte XOR salt `_eSal` (`main.pretty.js:317748-317759`).
  - The key comes from the `authData` object that the renderer passes to `PluginDeviceCommonStart` (`main.pretty.js:317849-317851`).
  - To derive it, take `sha512(authData.settings3)`, XOR it into `authData.settings2`, XOR the result with `_eSal`, then base64-decode to get the AES key.
  - If `authData.settings8` is present, it is XOR-decoded the same way.
  - Responses are not decrypted. `parseMessageData` calls `PBCommonBridge.decode` directly on the plugin output (`main.pretty.js:317785-317790`). So only the host-to-plugin direction is encrypted.
- CricutVulcanDevice uses `cricut.plugin_vulcan.proto.Request` and `Response` (`main.pretty.js:326550-326580`). Request oneofs are findDevices, getPodInfo, updatePodFirmware, uploadMainUnitFirmware, activatePod, getDeviceErrors and setDeviceSerialNumber (`main.pretty.js:164986`).
- CricutPathOperation uses `NativePathOperation.PBPathOperationInteractionMessage`, matched by `messageGuid` (`main.pretty.js:326208-326250`).
- CricutLocalData uses `NativeLocalData.PBLocalDataInteractionMessage` (about `main.pretty.js:319266`).
- CricutDeviceIO passes opaque bytes through. `PluginCdioSendBytes` does `stdin.write(bytes)` unframed (`main.pretty.js:325790`). Stdout chunks go straight to the renderer as `PluginCdioReceiveBytes` (`main.pretty.js:325812-325821`). Any framing for CDIO therefore lives in the renderer.

## 2. Renderer to main IPC contract for devices

The full list of 137 registrations is in `ipc_regs.txt`. Plugin channel names are an enum (`main.pretty.js:317229-317262`). The device-related channels follow.

**Machine bridge (CricutDevice)**, registered at `main.pretty.js:317764-317782`:
- `PluginDeviceCommonStart` is a handle; its argument is `{authData,...}`. It spawns the bridge with `--new-comm-lib`.
- `PluginDeviceCommonStop` is a handle. It sends `methodAction: STOP_BRIDGE_MA` and falls back to SIGKILL.
- `PluginDeviceCommonRequest` is a handle; its argument is a plain object, which main turns into PBCommonBridge with `fromObject`.
- `PluginDeviceCommonClearMatCutExecutionPlan` is an `on` listener.
- `PluginDeviceCommonVersion` is a handle.
- Main sends `PluginDeviceCommonOnResponse(decoded PBCommonBridge)` (`main.pretty.js:317818`) and `PluginDeviceCommonOnClose(exitCode, pid, signal, intentional)` (`main.pretty.js:317992`) back to the renderer.

**PBMethodAction values** (`main.pretty.js:81014`): NONE, GET_PLUGIN_VERSION, GET_OPEN_DEVICE, SET_OR_UPDATE_DEVICE, STARTUP_BRIDGE, STOP_BRIDGE, ON_INTERACTION_UPDATE, ENUMERATE, OPEN, CLOSE, CLEAR_BUFFER, WRITE_BUFFER, READ_BUFFER, REFRESH_BLUETOOTH_SOCKET, GET_DRAW_VECTORS, GET_PATH_FROM_VECTORS, GET_MACHINE_DEFAULT_STATES, SET/GET/ENABLE_LEARNING_MODEL, EXECUTE_COMMAND, SET_MACHINE_MODE, GET_MACHINE_MODES, GET_COMMAND_LOG, DRAW_DEVICE_CUT_PATH, GENERATE_MATCUT_EXECUTION_PLAN, ENCRYPT_AES_PATH and DECRYPT_AES_PATH. Each value carries the suffix `_MA`.

**PBInteractionStatus** (`main.pretty.js:80203`) is the state machine for load, go and firmware. Values include `riWaitOnMatLoad`, `riMatLoaded`, `riMatUnloaded`, `riWaitOnGo`, `riGoPressed`, `riPausePressed`, `riFiducialSearching`, `riWaitingOnMaterialSelected`, `riMaterialSelected`, `riToolInfoReceived`, `riMatInMotion*`, `riDeviceDisconnectedNotification`, `riConnectionSelfHeal*`, `riPTCC*` (print-then-cut calibration), `riFWUP*` (`riFWUPDownloadingFirmware`, `riFWUPRebootingToBootLoader`, `riFWUPErasingFirmware`, `riFWUPUpdatingFirmware`, ...) and `riMATCUTSetPathData`.

**PBCommonBridge fields relevant to devices** (`main.pretty.js:24437`): status, handle, interaction, methodAction, device, deviceList, matId, matPathData, matcutExecutionPlan, materialSelectedPayload, toolInfo, abortCut, progress, firmwareVersion, requiredFirmwareVersion, firmwareFile, firmwareUpdateConfig, forceBtUpdate, bluetoothNameToMachineType, bluetoothReadSize, bluetoothReadData, bluetoothTimeout, btAutoPairConfig, thisComputerBluetoothVersion, thisComputerMacAddress, authData, devicesHelloRequest, base64DevicesHelloKeyResponse, base64DevicesKeyResponse, cartridgeData, cartridgeImageSetGroupEncrypted, rawCustomCommand, executionPlans, connectionSelfHealInfo, cprAvailable, cprEngage, cprResult, featureFlags and logLevel.

**Raw device IO (CricutDeviceIO)**: `PluginCdioStart` and `PluginCdioStop` are handles, and `PluginCdioSendBytes` is an `on` listener (`main.pretty.js:325741-325795`). Main sends `PluginCdioReceiveBytes` back. The preload also listens for `pluginCdioClose`, `pluginCdioOnDestroyAdapter` and `pluginCdioOnCloseAdapter` (`preload.pretty.js:295-314`).

**Vulcan**: `PluginVulcanStart`, `PluginVulcanStop` and `PluginVulcanRequest` are handles, and main sends `PluginVulcanResponse` back (`main.pretty.js:326560-326580`).

**Bluetooth and machine housekeeping**:
- `SystemPreferencesOpenBluetooth`, `SystemPreferencesOpenBluetoothDevices` and `SystemPreferencesGetValues`.
- `LockMachine`, `UnlockMachine`, `CheckMachineIsLocked` and `DisconnectMachinesFromOtherWindows` (about `main.pretty.js:327412-327460`). These provide cross-window arbitration of a single machine.
- `MachinesUpdated` broadcasts `OnMachinesUpdated`.
- `MachineFamilyUpdated` and `MachineConnectionTooltipContent`/`IconLocation`/`ToolbarText`.

No channel names contain "usb", "serial" or "hid". USB, serial and HID access happens inside the native plugins, driven by PBMethodAction ENUMERATE, OPEN and WRITE_BUFFER, or by raw CDIO bytes.

**Preload shape.** `preload.js` assigns `window.ipcBridge = {...}`, with sub-objects `.events`, `.performance`, `.plugins` and `.update` (`preload.pretty.js:61, 449, 472, 489, 532`). Every method takes the **channel name as its first argument** from the renderer, for example `pluginDeviceCommonRequest: (ch, msg) => ipcRenderer.invoke(ch, msg)` (`preload.pretty.js:514`). The preload is therefore a generic relay with no allow-list. The renderer side maps it as `sendBridgeMessage(c){r.pluginDeviceCommonRequest(Ja.PluginDeviceCommonRequest,c)}` (`design-space-app/main.js` byte 15017128). Every window and BrowserView uses `contextIsolation: false` and `nodeIntegration: false` (`main.pretty.js:318583-318584, 320122-320123, 321163-321168, 322916-322917`), so the page shares one JS world with the preload.

## 3. preload-plugin.js and plugin windows

`preload-plugin.deob.js` exposes this object:
```
window.CricutApplicationPlugin = { exitPlugin: x => ipcRenderer.send("APP_PLUGIN_EXIT", x),
  getCurrentMetaData: () => ipcRenderer.invoke("APP_PLUGIN_METADATA"), getConfig: () => ipcRenderer.invoke('APP_PLUGIN_CONFIG'),
  onPluginShown: cb => ipcRenderer.on("APP_PLUGIN_SHOWN", ...), onPluginClose: cb => ipcRenderer.on("APP_PLUGIN_CLOSE", ...) }
```
It has nothing to do with the native device plugins. It is the preload for **web "application plugins"**: `PATH_PLUGIN_APPS = ["@cricut/design-space-monogram-maker"]` (`main.pretty.js:316382`; `APPLICATION_PLUGINS` in config.bin). `PluginApplicationLaunch` creates a separate hidden, transparent, frameless `BrowserWindow` per plugin app (`main.pretty.js:318561-318600`) with `show:false, transparent:true, frame:false, contextIsolation:false, preload: .../preload-plugin.js`. Main shows it and sends `APP_PLUGIN_SHOWN` (`main.pretty.js:318665`), and sends `APP_PLUGIN_CLOSE` on close (`main.pretty.js:318676`). Native binaries get **no** hidden window. They are plain child processes keyed by the requesting webContents id.

Window topology: a TABS shell window hosts up to **2 BrowserView tabs**, each a full Angular renderer (`_addTab`, `main.pretty.js:321160`: `if (getAllTabBrowserViews().length >= 2) return undefined`). Separate LOGIN, ABOUT (kept hidden) and plugin-app windows sit alongside.

## 4. Where the Make payload goes and its shape

The flow runs from renderer to main to the CricutDevice stdin.
1. When the bridge reaches `needPathData`, the renderer calls `_machinePathDataService.mapPathDataForMachine(mat, printThenCutData)`, then `sendPathData(w)` (`design-space-app/main.js` byte about 8733880).
2. The interaction handler sends this message (`design-space-app/chunk-JVMBAEXI.js` byte 3206791):
   `selectPath: t => this._bridgeComm.sendMessage({matPathData: t.pathData, status: W.riMATCUTSetPathData})`
3. Material selection sends `{matcutExecutionPlan: {} | undefined, materialSelectedPayload: {machineMargins, matHeight, matMode, matWidth, prescribedMatHeightInches, selected, skipMatMeasure}, status: riMaterialSelected}`.
4. Tool selection sends `{device, toolInfo, status: riToolInfoReceived}` (same offset).
5. `sendMessage` resolves to `ipcBridge.plugins.pluginDeviceCommonRequest("PluginDeviceCommonRequest", obj)`. Main then runs `PBCommonBridge.fromObject`, protobuf encode, AES-256-ECB encryption, the 4-byte LE length prefix, and writes the result to CricutDevice stdin (`main.pretty.js:317895-317960`).

**Shape of `matPathData` (PBMatPathData, `main.pretty.js:51426`).** It is built by `mapPathDataForMachine` (`design-space-app/main.js` byte 8050243):
- Top level: `materialSize` (PBSize from the mat), `pathEncrypted = !flag TURN_OFF_PATH_ENCRYPTION_TO_BRIDGE`, `fiducialId` and `fiducialData` (print-then-cut corner fiducials), and `imageData[]`.
- Each `imageData[]` entry is itself a PBMatPathData-like record with these fields: `pathData`, `transform`, `actualPathType`, `artTool`, `pathColor`, `pathId`, `imageId`, `imageName`, `imageType`, `importType`, `pathSplitData`, `useCustomPressure`, `customPressure`, `inPoint`, `outPoint`, `boundingBox` and `quadrant`.
- Progress counters: `totalV3`, `totalThisPassV3`, `totalNumPasses`, `sentV3`, `sentThisPassV3`, `currentPassV2` and `lastCutPathIndex`.
- Metrics: `pathWidthPt`, `pathHeightPt`, `pathLengthPt`, `pathAreaPt`, `pathXLengthPt` and `pathYLengthPt`.
- `pathData` is the layer's `layerContourDetails.clientPathSourceEncrypted`, a path that is already encrypted on the server side. It is decrypted to plain SVG path data via `_machineMakeAdapter.getLayerPath` only when the flag is off.
- Bifrost devices sort `imageData` by output type and then by colour.

**`matcutExecutionPlan`** (PBMatCutExecutionPlan, `main.pretty.js:73212`) has the fields `{id, toolInfo, matPathData, sortingDistances, groupedExecutions}`. The plugin generates it and returns it. **Main intercepts it and caches it per window**: `_matcutExecutionPlans[win] = msg.matcutExecutionPlan; msg.matcutExecutionPlan = {}` (`main.pretty.js:317791-317795`). When the renderer later sends `matcutExecutionPlan: {}`, main splices the cached plan back in (`main.pretty.js:317897-317900`). The renderer never holds the plan.

Separately, `MakeSvg` and `AddSvgToCanvas` (`main.pretty.js:320722-320752`) only relay an SVG between tabs. `MakeSvg` broadcasts `OnMakeSvg` to **every** webContents (`getAllWebContents().forEach(w => w.send(OnMakeSvg, payload))`).

## 5. config.bin

config.bin is AES-256-CBC encrypted JSON (`main.pretty.js:316681-316698`, `getAppConfigParams`).
- The key and IV come from `d = HMAC-SHA256(key="Ćɼȋçŭŧ…", data="")`. The key is `d[0:32]` and the IV is `d[0:16]`. The HMAC key codepoints are U+0106 U+027C U+020B U+00E7 U+016D U+0167 U+2026.
- The loader reads the file from `<exe dir>/Resources/app.asar/cricut-ele-design-space/config.bin` (`main.pretty.js:316702-316726`).
- I decrypted it with `tools/decbin.js`. The output is in `config.decoded.json`. It contains no secrets, only build metadata:
  - `ENVIRONMENT: production00a`, `APPLICATION_NAME: Hyperion`, `APPLICATION_VERSION 9.68.73`, `APPLICATION_TYPE PRODUCTION`, `APPLICATION_PLATFORM osx`, `APPLICATION_ARCHITECTURE x64`, `APPLICATION_PLUGINS ["@cricut/design-space-monogram-maker"]`, `GIT_BRANCH production1`, `GIT_COMMIT 6651ba19bf0d...`.
  - A `DEPENDENCIES` map: Angular ~18.2.14, `@cricut/bridge-proto ^6.12.4`, `@cricut/canvas-* 72.197.401`, `@cricut/bifrost-wasm 18.1.0`, and others.
  - Note that `APPLICATION_ARCHITECTURE` is x64.

## 6. Memory and freeze factors

**Flags.** There is no `--max-old-space-size`, `--js-flags`, `setV8Flags` or GPU switch anywhere in the asar. The only Chromium switch is `app.commandLine.appendSwitch("enable-features", "SharedArrayBuffer, WebAssemblyUnlimitedSyncCompilation")` (`main.pretty.js:324406-324409`).
- `WebAssemblyUnlimitedSyncCompilation` allows large synchronous WASM compiles on the main thread, which can freeze the UI. The canvas engine and `@cricut/bifrost-wasm` are WASM.
- The V8 heap therefore stays at the Electron default, roughly 4 GB per renderer.

**Throttling.** `backgroundThrottling` is driven by `DH_MACHINES_BACKGROUND_THROTTLING`, which defaults to `true` (`main.pretty.js:316131, 320125, 321164`). A background tab running a cut can be throttled.

**Processes.** Each canvas tab is a full renderer (up to 2 tabs) that spawns **7 web workers**: canvas-webw-composition, -cricanvas, -geo, -path, -raster, -render and -trace (`design-space-app/chunk-JVMBAEXI.js`). Each tab also gets its **own CricutDevice process** and possibly its own CDIO process.

**Stream reassembly is quadratic.** `onProcessData` (`main.pretty.js:317531-317548`) sums all buffered chunk sizes with `reduce` on every incoming chunk. On completion it calls `Buffer.concat` on the whole buffer. Large plugin responses such as `matcutExecutionPlan`, draw vectors or path data for big designs mean repeated scans and copies on the main-process event loop.

**Large payloads are copied many times.** One Make crosses this chain:
1. Renderer object to structured clone over IPC.
2. `PBCommonBridge.fromObject` in main.
3. `encode`.
4. A spread into a JS **array of numbers**: `[...cipher.update(buf), ...cipher.update(pad), ...cipher.final()]` and then `Buffer.from(array)` (`main.pretty.js:317952-317957`). For a payload of N bytes this allocates N boxed array elements, about 8 to 16 bytes each, plus a spread call. Multi-megabyte cut paths cost a lot of memory and CPU here. This is a likely source of freezes on big designs.
5. The decoded response is sent back to the renderer as a full object via `webContents.send` (`main.pretty.js:317818`).

**Cached execution plans.** `_matcutExecutionPlans` holds every window's latest plan in the main process until `PluginDeviceCommonClearMatCutExecutionPlan` is called.

**Canvas cache.** `CacheCanvasChange` writes the full `canvasData` to `LocalData/<userId>/<cacheFolder>/<id>.bin` (`main.pretty.js:318971-318992`). `getCacheFiles` reads **every** cache .bin into memory just to answer `CanvasCacheGetFirstFoundCache` (`main.pretty.js:318993-319020`).

**Broadcasts.** `MakeSvg` broadcasts the payload to every webContents (`main.pretty.js:320739-320752`), so it is serialised once per window.

**File protocol.** `protocol.interceptBufferProtocol("file", ...)` reads every `file://` asset fully into a Buffer with `fs.readFile`, including WASM (`main.pretty.js` about 326600-326660). No streaming is used.

**Diagnostics built into the app.** `AppStressMemory` is a deliberate test hook that allocates 500 x 1 MiB of random buffers (`main.pretty.js:324175-324196`). The flags `DH_SIMULATE_LOW_MEMORY_ISSUE`, `DH_CANVAS_LOG_WHEN_PATH_NODE_COUNT` ("Warning log event when total node count over limit"), `DH_CANVAS_LOG_WHEN_CONTOUR_COUNT` and `DH_CANVAS_LOG_WHEN_LAYER_COUNT` (`main.pretty.js:311758-311768`) show that Cricut tracks large-design node, contour and layer counts as a known problem.

## Open items

- I did not find where CricutCanvas is spawned in the main process. A plausible explanation is that the renderer calls it through another channel or that it is legacy, but this is not verified.
- I did not inspect how the renderer frames the raw CDIO bytes.
- The `authData.settings2`, `settings3` and `settings8` values come from the renderer or the server at runtime, so I could not derive the AES key statically.

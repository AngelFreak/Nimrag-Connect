# Frida hook points (phase 2)

These are for Garmin Connect **5.31 (versionCode 11761)** only. The obfuscated names change with every app build, so look them up again for any other version: search for the anchors listed, which survive obfuscation.

Each entry gives the class and method as jadx names them, the signature, and the anchor used to find it again. Nothing here has been hooked yet; this phase was static only.

Most of what these hooks would show can now be read from the HCI snoop capture with `tools/decode`. Hooks are only needed for the open questions that a capture cannot answer: MLR timers, decrypted Garmin Auth traffic, and the content the app builds before upload.

## Lowest level: GATT

| Purpose | Class.method (signature) | Anchor |
| --- | --- | --- |
| Raw write | `defpackage.y84.apply(BluetoothGattCharacteristic) : boolean`, which calls `setWriteType`, `setValue` and `BluetoothGatt.writeCharacteristic` | the only `writeCharacteristic` call in the BLE layer; logger "BleCommunication" in `v84` |
| Raw write (framework fallback) | `android.bluetooth.BluetoothGatt.writeCharacteristic(BluetoothGattCharacteristic)` | public API; does not depend on the app version |
| Raw notify | `defpackage.v84.onCharacteristicChanged(BluetoothGatt, BluetoothGattCharacteristic) : void` | only override in the app; Android 13+ routes the 3-argument callback to it |
| MTU | `defpackage.v84.onMtuChanged(BluetoothGatt, int, int)`; request in `defpackage.a94` (`requestMtu(515)`) | literal 515 |

## Multi-link

| Purpose | Class.method (signature) | Anchor |
| --- | --- | --- |
| Inbound demux by handle | `defpackage.cit.m9425l(int handle, byte[] data) : void` | logger tag "MultiLinkCommunicator"; "Dropping … bytes from service ID" |
| Control frames (handle 0) | `defpackage.ms6.mo46828c(cb4, UUID, UUID, byte[]) : void` | logger tag "CommandManager"; "Unknown command: {}" |
| Register | `defpackage.ms6.m57204f(int serviceId, int mode) : ListenableFuture` | "Register {} pending auth" |
| Reliable decision | `defpackage.kit.mo8947r(UUID, UUID, boolean)`, which reads `MLRInitializer.isLoaded()` | "do not map to a valid MultiLinkService" |
| MLR native boundary | `com.garmin.device.multilink.reliable.MLRConnectionHelper`: `mlrReceiveRawPacket(int, byte[])`, `mlrOpenConnection(int, int, int)`, `mlrReadyToSend()`, `mlrReliableRunProtocol()`, `onOpenHandle(int handle, int serviceId, int mtu)` | class is not obfuscated; library `libreliable-ml.so` |

## GFDI framing

| Purpose | Class.method (signature) | Anchor |
| --- | --- | --- |
| Encode, plaintext out (before COBS) | `defpackage.en6.m40411a(int length, byte[] message) : void` | COBS loop with `/ 254`; writes `0`, the body, then `0` |
| Build message (length, id, CRC) | `defpackage.vns.m72690a(fg80 txn, int msgId, qqi writer) : byte[]` | logger "MessageWriter"; "Response payload is larger than 65535" |
| Encrypt seam (identity unless Garmin Auth) | `pms.encrypt(byte[])`; implementation `ebj` | Kotlin metadata names "sessionKey", "hostInitializationVector" |
| Decode, frame in | `defpackage.dn6.m38321a() : byte[]` (COBS reader) | used by `kns` "MessageReader" |
| Reader loop: length, CRC, id | `defpackage.h6p.run()`, `case 15` | "Failed to read message: CRC check failed" |
| Decrypt seam | `mms.decrypt(byte[])`; implementation `dbj` | "GarminAuthMessageDecryptor" |

## Dispatch

| Purpose | Class.method (signature) | Anchor |
| --- | --- | --- |
| Register a handler | `defpackage.vid.mo72438e(int msgId, ur00 listener)` | "Overwriting request listener for type" |
| Per-request dispatch | coroutine `defpackage.yid`, which calls `ur00.onMessageReceived(int msgId, byte[] payload, Responder)` | "No handler for" (`xid`) |
| Send a request and await the response | `defpackage.vid.mo72436b(int msgId, byte[] payload, continuation)` | "Outgoing strategy not set" |
| Message names | `defpackage.sns.m67763a(int) : String` | "Acknowledgement", "Device Information" |

## File transfer

| Purpose | Class.method (signature) | Anchor |
| --- | --- | --- |
| FileManager receive (5004/5054/5022) | `com.garmin.gfdi.file.C8648a.m22289g(int msgId, byte[] payload, yid.C32802a responder)` | "Received file transfer request with no receiver task" |
| Chunk parser and CRC check | `defpackage.yw40.mo63519a(byte[]) : jow` | logger "StandardFileDataParser" |
| Download request (5002) | `defpackage.wtz` (logger "ReadFileTask"), method `m74589c` | "Download file response is too short" |
| Archive (5008) | `com.garmin.gfdi.file.C8648a.m22287c(int index, continuation)` | `EnumC8652e.ARCHIVE` |
| Archive after server upload | `defpackage.a7d` ("DeviceSyncUpload"), around `mo46583a` | "file successfully archived on device" |
| Handshake (5024/5050/5027) | `defpackage.zxl.onMessageReceived(int, byte[], Responder)` | "DeviceInformation message received: Begin Handshake" |
| Auth choice | `com.garmin.android.gfdi.DefaultAuthDelegate.isGarminAuthAllowed(String) : boolean` | class not obfuscated |

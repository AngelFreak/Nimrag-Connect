# Fixtures

Nothing under this directory may contain personal data: no positions, unit ids, MAC addresses, account ids, notification text or cloud responses.

- **Protocol (phase 0).** The committed fixture is a synthetic session generated inside `tools/decode/tests/cli.rs` with made-up identifiers. Real HCI snoop captures live in `private/captures/<date-name>/btsnoop_hci.log`, which git ignores. The test `every_private_capture_decodes_without_unknown_frames` replays all of them with `nimrag-decode --strict` whenever they are present.
- **FIT (phase 1).** Sample FIT files go here only after positions and identifiers have been scrubbed.

Adding a capture:

```sh
adb -s <serial> bugreport bugreport.zip     # snoop log must be enabled in Developer options
unzip -j bugreport.zip FS/data/misc/bluetooth/logs/btsnoop_hci.log -d private/captures/<date-name>/
cargo test -p nimrag-decode                 # replays it in strict mode
```

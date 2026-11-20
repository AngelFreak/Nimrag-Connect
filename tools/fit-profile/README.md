# fit-profile: local FIT profile tables

**Generated output is local only. `core/src/fit/generated/` is git-ignored; never
commit or publish it** (Garmin material, see `docs/LICENSING.md`). Only this
generator and the lookup API (`core/src/fit/profile.rs`) are committed.

`extract.py` reads the jadx output of the owner's own Garmin Connect 5.31
(versionCode 11761) APK under `private/re/` and writes
`core/src/fit/generated/profile_data.rs`. `core/build.rs` compiles that file in
(or the file named by env `NIMRAG_FIT_PROFILE`) and sets `cfg(fit_profile)`;
without it the committed `core/src/fit/profile_empty.rs` is used, every lookup
returns `None` and `profile::available()` is false. Tests asserting real names
run only under `cfg(fit_profile)`. After regenerating, run `cargo test` (build.rs
reruns on file change).

Second source (used for the server on runcho since 2026-11-20): `from_sdk.py`
reads Garmin's Python FIT SDK (`garmin_fit_sdk/profile.py` from the
`garmin-fit-sdk` wheel, FIT Protocol License, personal use; parsed with `ast`,
never executed) and writes the same shape (`MESSAGES`, `ENUMS`, `VERSION`). The
public SDK lacks Garmin-internal messages (training status, recovery time,
training load and balance, Firstbeat results, sleep summary) and some fields
(monitoring 35-38, body battery in stress_level); a local overlay
(`private/fit-overlay.json`, never committed) adds the ones the derivation reads,
with the numbers and scales the derive tests record.

```sh
# wheel from https://pypi.org/project/garmin-fit-sdk/ unpacked under private/fitsdk/py/x
python3 -I tools/fit-profile/from_sdk.py private/fitsdk/py/x/garmin_fit_sdk/profile.py \
    core/src/fit/generated --overlay private/fit-overlay.json
```

## Regenerate

```sh
# 1. normal jadx output (already in private/re/jadx/base/sources)
# 2. enum classes in jadx fallback mode (names survive only in <clinit>):
for c in $(sed 's/\.java//' private/re/fitraw/enumcands.txt); do
  ~/opt/jadx/bin/jadx --no-res -m fallback --single-class defpackage.$c \
    -d private/re/jadx/enums_fallback/$c private/re/apk/base.apk
done
# 3. generate
python3 -I tools/fit-profile/extract.py private/re/jadx/base/sources core/src/fit/generated
# tests
python3 -I tools/fit-profile/test_extract.py
```

`enumcands.txt` = jadx-output files that are `public enum` with `NAME(<int>)`
constants. `--enums-dir` overrides the fallback directory (default
`<sources>/../../enums_fallback`). Output is deterministic (sorted).

## How the data is recovered

* Messages: classes `extends wls` (Mesg). `new wls("record", 20)`, then
  `m74138e(new l5h(name, num, baseTypeId, scale, offset, units, accumulated,
  ppy.TYPE))` per field; components `r5h(dest, accumulate, bits, scale, offset)`
  and subfields `fr50(name, base, scale, offset, units)` + `(refField, refValue)`
  are attached to a field by its *position* in the message (`get(i)`).
* jadx `--deobf` replaces some literals by library constants of the same value
  (`TurfConstants.UNIT_DEGREES` for `"degrees"`, `TsExtractor.TS_STREAM_TYPE_SPLICE_INFO`
  for base type 134, `NotificationConstants.KEY_DATA_MIME_TYPE` for the field name
  `"type"`, ...). `extract.py` resolves these from the decompiled constant
  definitions and fails if one cannot be resolved.
* The profile type of a field (`ppy.X`, lower-cased into `type_name`) is a local
  alias or constant at the call site. The `ppy` enum constant *names* referenced
  by field definitions are real; unreferenced constants in `ppy` are garbage
  (jadx "fake fields") and are not used.
* `mesg_num` enum: name map built in the class with the `m9563k(num, map, "NAME", ...)` chain.
* Other enums: R8 removed the unused FIT enum classes and the constant *fields*
  of the survivors, so jadx's normal output shows garbage names for them
  (`file`: `MFG_RANGE_MIN(1)`, ... `DIVE_PLAN(25)`). The real names are the
  `const-string` arguments in `<clinit>`; fallback mode shows them and
  `parse_fallback_enum` reads `(name, value)` from `<init>(name, ordinal, value)`.
* Enum classes carry no FIT type name (R8 removed it), so a class is attributed
  to a type by content: it must contain all `ENUM_ANCHORS[type]` (NAME, value)
  pairs. Types without a match have no enum data (nothing is invented).

## Caveats

* No FIT profile version string exists in the app (only FIT protocol 2.0 in the
  SDK's version enum), so there is no `profile_version()`.
* `FieldInfo::array` is always `false`: the app's profile does not record it.
* Enum coverage is partial: only enum classes that R8 kept (see the generated
  `ENUMS` table). `sport`, `sub_sport`, `exercise_category`, `sleep_level` etc. are
  present only if listed in the final report of the extraction run.
* Duplicated component entries (e.g. `monitoring_info.obsolete_cycles_goal`
  lists the same component twice) are in the source and kept as-is.

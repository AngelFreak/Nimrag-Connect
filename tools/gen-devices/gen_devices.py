#!/usr/bin/env python3
"""Generate core/src/devices/catalogue.rs from the private Gadgetbridge clone.

Reads private/re/gadgetbridge (commit 7257742b, NOT committed to this repo):
  - DeviceType.java        Garmin enum entries (order = matching order)
  - devices/garmin/**      coordinator classes (name pattern, flag overrides)
  - res/values/strings.xml display names (devicetype_garmin_*)
  - FitCodeGenerator/src/main/resources/fit_profile.json  ONLY the
    product -> DeviceType link of manufacturer 1 entries (no Garmin names).
Run:  python3 -I tools/gen-devices/gen_devices.py private/re/gadgetbridge > core/src/devices/catalogue.rs && cargo fmt --all
"""
import glob, json, os, re, sys

root = sys.argv[1]
J = os.path.join(root, "app/src/main/java/nodomain/freeyourgadget/gadgetbridge")
G = os.path.join(J, "devices/garmin")

# (rust field, java method, default when never overridden)
BOOLS = [
    ("heart_rate", "supportsHeartRateMeasurement"),
    ("manual_heart_rate", "supportsManualHeartRateMeasurement"),
    ("spo2", "supportsSpo2"),
    ("respiratory_rate", "supportsRespiratoryRate"),
    ("stress", "supportsStressMeasurement"),
    ("body_energy", "supportsBodyEnergy"),
    ("hrv", "supportsHrvMeasurement"),
    ("sleep_score", "supportsSleepScore"),
    ("intensity_minutes", "supportsPai"),
    ("vo2_max", "supportsVO2Max"),
    ("vo2_max_multisport", "supportsVO2MultiSport"),
    ("race_prediction", "supportsRacePrediction"),
    ("training_load", "supportsTrainingLoad"),
    ("solar_charging", "supportsSolarCharging"),
    ("weather", "supportsWeather"),
    ("calendar_events", "supportsCalendarEvents"),
    ("call_control", "supportsCallControl"),
    ("recorded_activities", "supportsRecordedActivities"),
    ("data_fetching", "supportsDataFetching"),
]

classes = {}
for f in glob.glob(G + "/**/*.*", recursive=True):
    if not f.endswith((".java", ".kt")):
        continue
    src = open(f, encoding="utf-8").read()
    m = re.search(r"class\s+(\w+)\s*(?:\(\))?\s*(?:extends|:)\s*(\w+)", src)
    if not m:
        continue
    classes[m.group(1)] = (f, m.group(2), src)


def body(src, name):
    m = re.search(r"\b%s\s*\([^)]*\)[^{=]*[{=]" % name, src)
    if not m:
        return None
    i = m.end()
    if src[i - 1] == "=":
        return src[i:i + 40].split("\n")[0]
    depth, j = 1, i
    while depth:
        depth += {"{": 1, "}": -1}.get(src[j], 0)
        j += 1
    return src[i:j - 1]


dynamic = []


def literal(b):
    b = re.sub(r"//.*", "", b)
    r = re.findall(r"return\s+([^;]+?);?\s*$|^\s*(true|false)\s*$", b.strip(), re.M)
    vals = [x[0] or x[1] for x in r]
    if len(vals) == 1:
        return vals[0].strip()
    return None


def resolve(cls, method):
    c = cls
    while c in classes:
        _, parent, src = classes[c]
        b = body(src, method)
        if b is not None:
            v = literal(b)
            if v in ("true", "false", "0", "1") or re.fullmatch(r"(DeviceCoordinator\.)?DeviceKind\.\w+", v or ""):
                return v
            dynamic.append((cls, c, method))
            return None
        c = parent
    return None


def pattern(cls):
    c = cls
    while c in classes:
        _, parent, src = classes[c]
        m = re.search(r'Pattern\.compile\("((?:[^"\\]|\\.)*)"\)', src)
        if m:
            return m.group(1).replace("\\\\", "\\")  # undo Java string escaping
        c = parent


strings = open(os.path.join(root, "app/src/main/res/values/strings.xml"), encoding="utf-8").read()
names = dict(re.findall(r'<string name="(devicetype_garmin_\w+)"[^>]*>([^<]*)</string>', strings))
dt = open(os.path.join(J, "model/DeviceType.java"), encoding="utf-8").read()
entries = re.findall(r"^\s*(GARMIN_\w+)\((\w+)::new\)", dt, re.M)
prof = json.load(open(os.path.join(root, "FitCodeGenerator/src/main/resources/fit_profile.json")))
prods = {}
for d in prof["devices"]:
    if d["manufacturer"] == 1 and d.get("type"):
        prods.setdefault(d["type"].split(".")[-1], []).append(d["product"])

KIND = {"WATCH": "Watch", "BIKE_COMPUTER": "BikeComputer", "FITNESS_BAND": "FitnessBand",
        "CHEST_STRAP": "ChestStrap", "PHONE": "Phone", "UNKNOWN": "Unknown"}
copyrights = set()
out = []
for key, cls in entries:
    f, _, src = classes[cls]
    chain, c = [], cls
    while c in classes:
        chain.append(classes[c][0]); c = classes[c][1]
    for p in chain:
        h = re.match(r"/\*\s*Copyright \(C\)\s*(.*?)\n\s*\n", open(p, encoding="utf-8").read(), re.S)
        if h:
            copyrights.add(re.sub(r"\s*\n\s*", " ", h.group(1)).strip())
    res = re.search(r"R\.string\.(devicetype_garmin_\w+)", src)
    flags = {}
    for field, meth in BOOLS:
        v = resolve(cls, meth)
        flags[field] = v == "true"
    if resolve(cls, "supportsVO2MultiSport") is None:
        flags["vo2_max_multisport"] = flags["vo2_max"]  # Gadgetbridge: supportsVO2MultiSport = supportsVO2Max
    if resolve(cls, "supportsManualHeartRateMeasurement") is None:
        flags["manual_heart_rate"] = flags["heart_rate"]
    k = resolve(cls, "getDeviceKind")
    kind = KIND[k.split(".")[-1]] if k else "Unknown"
    bc = resolve(cls, "getBatteryCount")
    out.append(dict(key=key, cls=cls, name=names[res.group(1)], pat=pattern(cls), kind=kind,
                    flags=flags, battery=int(bc) if bc is not None else 1,
                    exp=resolve(cls, "isExperimental") == "true",
                    prods=sorted(prods.get(key, []))))

hdr = sorted(copyrights)
w = sys.stdout.write


def rs(x):
    return json.dumps(x, ensure_ascii=False)  # same escapes as Rust for our data

w("//! Garmin device catalogue, GENERATED by tools/gen-devices/gen_devices.py. Do not edit.\n//!\n")
w("//! Ported from Gadgetbridge (https://codeberg.org/Freeyourgadget/Gadgetbridge), commit\n")
w("//! 7257742b96a30fff0db9e79230435ac2aeb39f57, licence AGPL-3.0-or-later. Sources:\n")
w("//!   app/src/main/java/nodomain/freeyourgadget/gadgetbridge/model/DeviceType.java\n")
w("//!   app/src/main/java/nodomain/freeyourgadget/gadgetbridge/devices/garmin/**/*Coordinator.{java,kt}\n")
w("//!   app/src/main/res/values/strings.xml (devicetype_garmin_* display names)\n")
w("//!   FitCodeGenerator/src/main/resources/fit_profile.json (product id -> DeviceType link only)\n")
w("//! Original copyright lines of the ported files (aggregated):\n")
for h in hdr:
    w("//!   Copyright (C) %s\n" % h)
w("\nuse super::{DeviceKind, Features, Model};\n\n")
w("/// Gadgetbridge `DeviceType` order; name matching returns the first hit, as Gadgetbridge does.\n")
w("#[allow(clippy::needless_update)] // `..Features::NONE` is a no-op when every flag is set\npub static MODELS: &[Model] = &[\n")
for e in out:
    f = e["flags"]
    on = ", ".join("%s: true" % k for k, _ in BOOLS if f[k])
    w("    Model {\n        id: %s,\n        name: %s,\n        name_pattern: %s,\n" % (
        rs(e["key"]), rs(e["name"]), rs(e["pat"])))
    w("        product_ids: &[%s],\n        kind: DeviceKind::%s,\n" % (", ".join(map(str, e["prods"])), e["kind"]))
    w("        features: Features { %s%s..Features::NONE },\n" % (on, ", " if on else ""))
    w("        battery_count: %d,\n        experimental: %s,\n        verified: %s,\n    },\n" % (
        e["battery"], "true" if e["exp"] else "false", "true" if e["key"] == "GARMIN_INSTINCT_2" else "false"))
w("];\n")
sys.stderr.write("models %d, dynamic overrides skipped: %s\n" % (len(out), sorted(set(m for _, _, m in dynamic))))
sys.stderr.write("no pattern: %s\n" % [e["key"] for e in out if not e["pat"]])
sys.stderr.write("no product: %d\n" % sum(1 for e in out if not e["prods"]))

#!/usr/bin/env python3
"""Generate core/src/fit/generated/profile_data.rs from Garmin's Python FIT SDK.

Usage:
    python3 -I tools/fit-profile/from_sdk.py <garmin_fit_sdk/profile.py> <output dir, e.g. core/src/fit/generated> [--overlay FILE]

The input is `garmin_fit_sdk/profile.py` from the `garmin-fit-sdk` wheel on PyPI,
downloaded for personal use under the FIT Protocol License (keep it under
private/). The file is parsed with `ast` and read with `ast.literal_eval`: it is
never imported or executed.

Mapping (the SDK's own decoder rules):
* A field's scale/offset apply only when the SDK lists one of each; with several
  (a field split into components) the field itself is unscaled.
* Component i: target field `components[i]`, `bits[i]`, `scale[i]`, `offset[i]`,
  `units[i]`; it accumulates when its target field is accumulated.
* Subfields: `map` entries give the (reference field, raw value) pairs. A subfield's
  base type is its type when that is a base type, else the parent's base type.
* `array` comes from the SDK (the jadx extraction could not recover it).

--overlay FILE (optional, local, e.g. private/fit-overlay.json): Garmin-internal
messages and fields the public SDK lacks (training status, recovery time, sleep
summary, ...), as JSON `{"messages": {"<num>": {"name": ..., "fields": [[num, name,
base_type, type, scale, offset, units], ...]}}}`. Overlay fields only fill gaps:
a field number the SDK defines is never replaced.

Same output shape as extract.py (`MESSAGES`, `ENUMS`, `VERSION`), deterministic.
LOCAL USE ONLY: the output is git-ignored and must never be committed or published.
"""
import ast
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from extract import rs_bt, rs_opt_f, rs_opt_s, rs_str  # noqa: E402

BASE_TYPES = {
    "enum", "sint8", "uint8", "sint16", "uint16", "sint32", "uint32", "string",
    "float32", "float64", "uint8z", "uint16z", "uint32z", "byte", "sint64",
    "uint64", "uint64z",
}


def load_profile(path):
    """The `Profile = {...}` literal from profile.py, without executing anything."""
    with open(path, encoding="utf-8") as fh:
        tree = ast.parse(fh.read(), filename=path)
    for node in tree.body:
        if (isinstance(node, ast.Assign) and len(node.targets) == 1
                and isinstance(node.targets[0], ast.Name) and node.targets[0].id == "Profile"):
            return ast.literal_eval(node.value)
    raise ValueError("no `Profile = {...}` assignment in %s" % path)


def as_list(v):
    if isinstance(v, list):
        return v
    return [v]


def single(values, default):
    """The field's own scale/offset: only when exactly one is listed."""
    values = as_list(values)
    return values[0] if len(values) == 1 else default


def field_units(units):
    if isinstance(units, str):
        return units
    return units[0] if len(units) == 1 else ""


def nth(values, i, default):
    values = as_list(values)
    return values[i] if i < len(values) else default


def components_of(f, fields_by_num):
    out = []
    units = f["units"]
    for i, dest in enumerate(f.get("components", [])):
        target = fields_by_num.get(dest)
        out.append({
            "dest": int(dest),
            "bits": int(nth(f["bits"], i, 0)),
            "scale": nth(f["scale"], i, 1),
            "offset": nth(f["offset"], i, 0),
            "units": nth(units, i, "") if isinstance(units, list) else "",
            "accumulate": bool(target and target.get("is_accumulated") is True),
        })
    return out


def is_true(v):
    return v is True or v == "true"


def convert(profile):
    """(messages, enums, version): plain data, sorted, ready to render."""
    messages = {}
    for key, msg in profile["messages"].items():
        num = int(key)
        fields_by_num = {int(k): v for k, v in msg["fields"].items()}
        fields = []
        for fnum in sorted(fields_by_num):
            f = fields_by_num[fnum]
            base = f["base_type"]
            subs = []
            for s in f.get("sub_fields", []):
                subs.append({
                    "name": s["name"],
                    "base_type": s["type"] if s["type"] in BASE_TYPES else base,
                    "type_name": s["type"],
                    "scale": single(s["scale"], 1),
                    "offset": single(s["offset"], 0),
                    "units": field_units(s["units"]),
                    "refs": [(int(m["num"]), int(m["raw_value"])) for m in s.get("map", [])],
                    "components": components_of(s, fields_by_num),
                })
            fields.append({
                "num": fnum,
                "name": f["name"],
                "base_type": base,
                "type_name": f["type"],
                "scale": single(f["scale"], 1),
                "offset": single(f["offset"], 0),
                "units": field_units(f["units"]),
                "array": is_true(f.get("array")),
                "accumulated": f.get("is_accumulated") is True,
                "components": components_of(f, fields_by_num),
                "subfields": subs,
            })
        messages[num] = {"name": msg["name"], "fields": fields}

    enums = {}
    for tname, values in profile["types"].items():
        vals = {}
        for raw, name in values.items():
            try:
                v = int(raw)
            except (TypeError, ValueError):
                continue
            if v >= 0 and isinstance(name, str):
                vals.setdefault(v, name)
        if vals:
            enums[tname] = vals

    v = profile.get("version", {})
    version = int(v["major"]) * 1000 + int(v["minor"]) if "major" in v and "minor" in v else None
    return messages, enums, version


def apply_overlay(messages, overlay):
    """Add overlay messages/fields that the SDK lacks; returns the number of fields added."""
    added = 0
    for key, om in overlay.get("messages", {}).items():
        num = int(key)
        msg = messages.setdefault(num, {"name": om.get("name", "mesg_%d" % num), "fields": []})
        have = {f["num"] for f in msg["fields"]}
        for fnum, name, base, tname, scale, offset, units in om.get("fields", []):
            if base not in BASE_TYPES:
                raise ValueError("overlay %d.%d: unknown base type %r" % (num, fnum, base))
            if fnum in have:
                continue
            msg["fields"].append({
                "num": fnum, "name": name, "base_type": base, "type_name": tname,
                "scale": scale, "offset": offset, "units": units, "array": False,
                "accumulated": False, "components": [], "subfields": [],
            })
            added += 1
        msg["fields"].sort(key=lambda f: f["num"])
    return added


def render_component(c):
    return "comp(%d,%d,%s,%s,%s,%s)" % (
        c["dest"], c["bits"], rs_opt_f(c["scale"], 1.0), rs_opt_f(c["offset"], 0.0),
        rs_opt_s(c["units"]), "true" if c["accumulate"] else "false")


def render_rust(messages, enums, version, source):
    out = []
    w = out.append
    w("// @generated by tools/fit-profile/from_sdk.py -- DO NOT EDIT.")
    w("// Derived from %s (Garmin FIT SDK, FIT Protocol License)." % source)
    w("// LOCAL USE ONLY: git-ignored, must never be committed or published.")
    w("")
    w("pub(super) static MESSAGES: &[MessageInfo] = &[")
    for number in sorted(messages):
        msg = messages[number]
        w("    MessageInfo { global: %d, name: %s, fields: &[" % (number, rs_str(msg["name"])))
        for f in msg["fields"]:
            subs = ",".join(
                "sub(%s,%s,%s,%s,%s,%s,&[%s],&[%s])" % (
                    rs_str(s["name"]), rs_bt(s["base_type"]), rs_str(s["type_name"]),
                    rs_opt_f(s["scale"], 1.0), rs_opt_f(s["offset"], 0.0), rs_opt_s(s["units"]),
                    ",".join("(%d,%d)" % r for r in s["refs"]),
                    ",".join(render_component(c) for c in s["components"]))
                for s in f["subfields"])
            comps = ",".join(render_component(c) for c in f["components"])
            entry = "fe(%d,%s,%s,%s,%s,%s,%s,%s,&[%s],&[%s])" % (
                f["num"], rs_str(f["name"]), rs_bt(f["base_type"]), rs_str(f["type_name"]),
                rs_opt_f(f["scale"], 1.0), rs_opt_f(f["offset"], 0.0), rs_opt_s(f["units"]),
                "true" if f["accumulated"] else "false", comps, subs)
            if f["array"]:
                entry = "arr(%s)" % entry
            w("        %s," % entry)
        w("    ] },")
    w("];")
    w("")
    w("/// (type name, [(value, name)]) sorted by type name then value.")
    w("pub(super) static ENUMS: &[(&str, &[(u64, &str)])] = &[")
    for tname in sorted(enums):
        vals = enums[tname]
        w("    (%s, &[%s])," % (rs_str(tname), ", ".join(
            "(%d, %s)" % (v, rs_str(vals[v])) for v in sorted(vals))))
    w("];")
    w("")
    w("pub(super) static VERSION: Option<u32> = %s;" % ("None" if version is None else "Some(%d)" % version))
    w("")
    return "\n".join(out)


def main(argv):
    args = list(argv[1:])
    overlay_path = None
    if "--overlay" in args:
        i = args.index("--overlay")
        if i + 1 >= len(args):
            sys.stderr.write(__doc__)
            return 2
        overlay_path = args[i + 1]
        del args[i:i + 2]
    if len(args) != 2:
        sys.stderr.write(__doc__)
        return 2
    path, out_dir = args
    profile = load_profile(path)
    messages, enums, version = convert(profile)
    v = profile.get("version", {})
    source = "garmin-fit-sdk profile %s.%s.%s" % (v.get("major"), v.get("minor"), v.get("patch"))
    if overlay_path:
        with open(overlay_path, encoding="utf-8") as fh:
            added = apply_overlay(messages, json.load(fh))
        source += " + local overlay (%d fields)" % added
    text = render_rust(messages, enums, version, source)
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "profile_data.rs"), "w", encoding="utf-8") as fh:
        fh.write(text)
    nfields = sum(len(m["fields"]) for m in messages.values())
    nvals = sum(len(e) for e in enums.values())
    sys.stderr.write("%s: messages=%d fields=%d enum_types=%d enum_values=%d\n" % (
        source, len(messages), nfields, len(enums), nvals))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

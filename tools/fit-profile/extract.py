#!/usr/bin/env python3
"""Generate core/src/fit/profile_data.rs from jadx output of the Garmin Connect APK.

Usage:
    python3 -I tools/fit-profile/extract.py <jadx sources dir> <output dir, e.g. core/src/fit/generated> [--enums-dir DIR]

Inputs
------
* <jadx sources dir>: jadx (--deobf) sources of the app, e.g.
  private/re/jadx/base/sources. The FIT SDK message definitions survive as
  `new wls("record", 20)` / `new l5h(name, num, base, scale, offset, units,
  accumulated, ppy.TYPE)` calls. jadx --deobf replaces some string/int literals
  by the library constants they equal (e.g. `TurfConstants.UNIT_DEGREES`), so
  those symbols are resolved back to literals from the other decompiled classes.
* --enums-dir: directory of per-class `jadx -m fallback` output of the FIT enum
  classes (R8 stripped the constant *fields*, so jadx's normal output has garbage
  names, but the constant name strings survive in <clinit> and fallback mode
  shows them). Default: <sources>/../../enums_fallback.

The output is deterministic (everything sorted).
"""
import os
import re
import sys

# ---------------------------------------------------------------------------
# obfuscated member names of the FIT SDK classes in Garmin Connect 5.31
# (wls = Mesg, l5h = Field, fr50 = SubField, r5h = FieldComponent)
FIELDS_LIST = "f204963c"
COMPONENTS_LIST = "f120137h"
SUBFIELDS_LIST = "f120138i"
ADD_FIELD = "m74138e"
SUBFIELD_ADD_REF = "m42679b"
SUBFIELD_ADD_COMPONENT = "m42678a"

APP_VERSION = "Garmin Connect 5.31 (versionCode 11761)"

BASE_TYPES = {
    0: "enum", 1: "sint8", 2: "uint8", 7: "string", 10: "uint8z", 13: "byte",
    131: "sint16", 132: "uint16", 133: "sint32", 134: "uint32", 136: "float32",
    137: "float64", 139: "uint16z", 140: "uint32z", 142: "sint64",
    143: "uint64", 144: "uint64z",
}


class ExtractError(Exception):
    pass


# ---------------------------------------------------------------------------
# Java literal / constant handling

_INT_RE = re.compile(r"^-?(0[xX][0-9a-fA-F]+|\d+)[lL]?$")
_FLOAT_RE = re.compile(r"^-?(\d+\.?\d*|\.\d+)([eE][-+]?\d+)?[dDfF]?$")


def unescape_java_string(body):
    out = []
    i = 0
    while i < len(body):
        c = body[i]
        if c == "\\" and i + 1 < len(body):
            n = body[i + 1]
            if n == "u" and i + 5 < len(body) + 0:
                out.append(chr(int(body[i + 2:i + 6], 16)))
                i += 6
                continue
            out.append({"n": "\n", "t": "\t", "r": "\r", '"': '"',
                        "\\": "\\", "'": "'", "0": "\0"}.get(n, n))
            i += 2
            continue
        out.append(c)
        i += 1
    return "".join(out)


class Resolver:
    """Resolves `Class.CONST` symbols jadx left in place of literals.

    The simple class name is looked up among the decompiled classes; if the
    referencing file imports it (see `set_context`) only the imported class is
    considered, and if several classes still define the constant with different
    values the resolution fails instead of guessing.
    """

    def __init__(self, root):
        self.root = root
        self._by_basename = None
        self._cache = {}
        self._imports = {}

    def set_context(self, text):
        """Remember the `import a.b.C;` lines of the file being parsed."""
        self._imports = {
            m.group(2): m.group(1) + "." + m.group(2)
            for m in re.finditer(r"^import ([\w.]+)\.(\w+);", text, re.M)}

    def _index(self):
        if self._by_basename is None:
            self._by_basename = {}
            for dirpath, _dirs, files in os.walk(self.root):
                for f in files:
                    if f.endswith(".java"):
                        self._by_basename.setdefault(f[:-5], []).append(
                            os.path.join(dirpath, f))
        return self._by_basename

    def resolve(self, symbol):
        full = self._imports.get(symbol.split(".")[0], "")
        key = (symbol, full)
        if key in self._cache:
            return self._cache[key]
        parts = symbol.split(".")
        if len(parts) < 2:
            raise ExtractError("cannot resolve bare symbol %r" % symbol)
        const = parts[-1]
        files = self._index().get(parts[0], [])
        if full:
            suffix = os.sep + full.replace(".", os.sep) + ".java"
            narrowed = [f for f in files if f.endswith(suffix)]
            files = narrowed or files
        pat = re.compile(
            r"\b(?:String|int|long|short|byte)\s+" + re.escape(const) +
            r"\s*=\s*(\"(?:[^\"\\]|\\.)*\"|-?(?:0[xX][0-9a-fA-F]+|\d+)[lL]?)\s*;")
        values = []
        for path in sorted(files):
            with open(path, encoding="utf-8", errors="replace") as fh:
                m = pat.search(fh.read())
            if m:
                values.append(parse_literal(m.group(1), None))
        if not values:
            raise ExtractError("unresolved constant %s" % symbol)
        if any(v != values[0] for v in values):
            raise ExtractError("ambiguous constant %s: %r" % (symbol, values))
        self._cache[key] = values[0]
        return values[0]


def parse_literal(tok, resolver):
    """Java token -> str | int | float | bool."""
    tok = tok.strip()
    if tok.startswith('"'):
        return unescape_java_string(tok[1:-1])
    if tok in ("true", "false"):
        return tok == "true"
    if _INT_RE.match(tok):
        t = tok.rstrip("lL")
        return int(t, 16) if t.lower().lstrip("-").startswith("0x") else int(t)
    if _FLOAT_RE.match(tok):
        return float(tok.rstrip("dDfF"))
    if re.match(r"^[A-Za-z_][\w]*(\.[A-Za-z_]\w*)+$", tok):
        if resolver is None:
            raise ExtractError("symbol %r where a literal is required" % tok)
        return resolver.resolve(tok)
    raise ExtractError("cannot parse java token %r" % tok)


def split_args(text):
    """Split a Java argument list at top-level commas (strings/parens aware)."""
    args, depth, cur, i = [], 0, [], 0
    while i < len(text):
        c = text[i]
        if c == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            cur.append(text[i:j + 1])
            i = j + 1
            continue
        if c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
        if c == "," and depth == 0:
            args.append("".join(cur).strip())
            cur = []
        else:
            cur.append(c)
        i += 1
    if "".join(cur).strip():
        args.append("".join(cur).strip())
    return args


def balanced_args(text, start):
    """text[start:] begins right after '('. Return (inner text, index after ')')."""
    depth, i = 1, start
    while i < len(text):
        c = text[i]
        if c == '"':
            i += 1
            while text[i] != '"':
                i += 2 if text[i] == "\\" else 1
        elif c == "(":
            depth += 1
        elif c == ")":
            depth -= 1
            if depth == 0:
                return text[start:i], i + 1
        i += 1
    raise ExtractError("unbalanced parentheses")


# ---------------------------------------------------------------------------
# message definitions

_TOKEN_RE = re.compile(
    r"(?P<wls>new wls\()"
    r"|(?P<alias>\b(?:ppy\s+)?(?P<avar>ppyVar\d*)\s*=\s*ppy\.(?P<aval>\w+)\s*;)"
    r"|(?P<field>\b" + ADD_FIELD + r"\(new l5h\()"
    r"|(?P<comp>\b" + FIELDS_LIST + r"\.get\((?P<cidx>\d+)\)\." + COMPONENTS_LIST +
    r"\.add\(new r5h\()"
    r"|(?P<subadd>\b" + FIELDS_LIST + r"\.get\((?P<sidx>\d+)\)\." + SUBFIELDS_LIST +
    r"\.add\(new fr50\()"
    r"|(?P<subref>\b" + FIELDS_LIST + r"\.get\((?P<rfi>\d+)\)\." + SUBFIELDS_LIST +
    r"\.get\((?P<rsi>\d+)\)\." + SUBFIELD_ADD_REF + r"\()"
    r"|(?P<subcomp>\b" + FIELDS_LIST + r"\.get\((?P<cfi>\d+)\)\." + SUBFIELDS_LIST +
    r"\.get\((?P<csi>\d+)\)\." + SUBFIELD_ADD_COMPONENT + r"\(new r5h\()"
)


def _num(v, what):
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        raise ExtractError("%s is not a number: %r" % (what, v))
    return v


def _base_type(v):
    if v not in BASE_TYPES:
        raise ExtractError("unknown FIT base type id %r" % (v,))
    return BASE_TYPES[v]


def _component(args, resolver):
    if len(args) != 5:
        raise ExtractError("component with %d args" % len(args))
    return {
        "dest": int(_num(parse_literal(args[0], resolver), "dest")),
        "accumulate": bool(parse_literal(args[1], resolver)),
        "bits": int(_num(parse_literal(args[2], resolver), "bits")),
        "scale": float(_num(parse_literal(args[3], resolver), "scale")),
        "offset": float(_num(parse_literal(args[4], resolver), "offset")),
    }


def parse_message_source(text, resolver):
    """Parse one jadx source file; returns a list of message dicts."""
    messages = []
    aliases = {}
    cur = None
    for m in _TOKEN_RE.finditer(text):
        kind = m.lastgroup
        # lastgroup is the last *matched* named group; map nested group names
        if m.group("wls"):
            inner, _ = balanced_args(text, m.end())
            args = split_args(inner)
            if len(args) == 1:  # copy constructor `new wls(other)`
                continue
            if len(args) != 2:
                raise ExtractError("wls(%s)" % inner)
            cur = {
                "name": parse_literal(args[0], resolver),
                "number": int(parse_literal(args[1], resolver)),
                "fields": [],
            }
            messages.append(cur)
        elif m.group("alias"):
            aliases[m.group("avar")] = m.group("aval")
        elif m.group("field"):
            if cur is None:
                raise ExtractError("field outside message")
            inner, _ = balanced_args(text, m.end())
            args = split_args(inner)
            if len(args) != 8:
                raise ExtractError("l5h with %d args: %s" % (len(args), inner))
            ptype = args[7]
            if ptype.startswith("ppy."):
                ptype = ptype[4:]
            elif ptype in aliases:
                ptype = aliases[ptype]
            else:
                raise ExtractError("unknown profile type expr %r" % ptype)
            cur["fields"].append({
                "name": parse_literal(args[0], resolver),
                "num": int(_num(parse_literal(args[1], resolver), "field num")),
                "base_type": _base_type(parse_literal(args[2], resolver)),
                "scale": float(_num(parse_literal(args[3], resolver), "scale")),
                "offset": float(_num(parse_literal(args[4], resolver), "offset")),
                "units": parse_literal(args[5], resolver),
                "accumulated": bool(parse_literal(args[6], resolver)),
                "type_name": ptype.lower(),
                "components": [],
                "subfields": [],
            })
        elif m.group("comp"):
            inner, _ = balanced_args(text, m.end())
            idx = int(m.group("cidx"))
            cur["fields"][idx]["components"].append(
                _component(split_args(inner), resolver))
        elif m.group("subadd"):
            inner, _ = balanced_args(text, m.end())
            args = split_args(inner)
            if len(args) != 5:
                raise ExtractError("fr50 with %d args" % len(args))
            cur["fields"][int(m.group("sidx"))]["subfields"].append({
                "name": parse_literal(args[0], resolver),
                "base_type": _base_type(parse_literal(args[1], resolver)),
                "scale": float(_num(parse_literal(args[2], resolver), "scale")),
                "offset": float(_num(parse_literal(args[3], resolver), "offset")),
                "units": parse_literal(args[4], resolver),
                "refs": [],
                "components": [],
            })
        elif m.group("subref"):
            inner, _ = balanced_args(text, m.end())
            args = split_args(inner)
            sub = cur["fields"][int(m.group("rfi"))]["subfields"][int(m.group("rsi"))]
            sub["refs"].append((
                int(parse_literal(args[0], resolver)),
                int(parse_literal(args[1], resolver)),
            ))
        elif m.group("subcomp"):
            inner, _ = balanced_args(text, m.end())
            sub = cur["fields"][int(m.group("cfi"))]["subfields"][int(m.group("csi"))]
            sub["components"].append(_component(split_args(inner), resolver))
        else:  # pragma: no cover
            raise ExtractError("unhandled token %s" % kind)
    return messages


def extract_messages(root, resolver):
    messages = {}
    for dirpath, _dirs, files in os.walk(root):
        for f in sorted(files):
            if not f.endswith(".java"):
                continue
            path = os.path.join(dirpath, f)
            with open(path, encoding="utf-8", errors="replace") as fh:
                text = fh.read()
            if not re.search(r"class \w+ extends wls\b", text):
                continue
            resolver.set_context(text)
            try:
                found = parse_message_source(text, resolver)
            except (ExtractError, IndexError, ValueError) as e:
                raise ExtractError("%s: %s" % (path, e))
            for msg in found:
                msg["source"] = f
                if msg["number"] in messages:
                    raise ExtractError("duplicate message number %d (%s, %s)" % (
                        msg["number"], msg["name"], messages[msg["number"]]["name"]))
                messages[msg["number"]] = msg
    return messages


# ---------------------------------------------------------------------------
# mesg_num names (dms: ckd.m9563k(num, map, "NAME", num2, "NAME2") chains)

_MESG_PUT_RE = re.compile(r"\bm9563k\(")
_MESG_FIRST_RE = re.compile(r"\bmap\.put\(Integer\.valueOf\(")


def extract_mesg_nums(root, resolver):
    """Find the class that builds the mesg_num -> NAME map from m9563k calls."""
    best = {}
    for dirpath, _dirs, files in os.walk(root):
        for f in sorted(files):
            if not f.endswith(".java"):
                continue
            with open(os.path.join(dirpath, f), encoding="utf-8",
                      errors="replace") as fh:
                text = fh.read()
            if text.count("m9563k(") < 20:
                continue
            resolver.set_context(text)
            found = parse_mesg_num_source(text, resolver)
            if len(found) > len(best):
                best = found
    return best


def parse_mesg_num_source(text, resolver):
    out = {}
    for m in _MESG_PUT_RE.finditer(text):
        inner, _ = balanced_args(text, m.end())
        args = split_args(inner)
        if len(args) != 5:
            continue
        out[int(parse_literal(args[0], resolver))] = parse_literal(args[2], resolver)
        out[int(parse_literal(args[3], resolver))] = parse_literal(args[4], resolver)
    return {k: v.lower() for k, v in out.items()}


# ---------------------------------------------------------------------------
# enums from `jadx -m fallback` output

_NEW_RE = re.compile(r"^\s*(\S+) (r\d+) = new (\S+)\s*$")
_STR_RE = re.compile(r'^\s*(?:java\.lang\.String )?(r\d+) = "((?:[^"\\]|\\.)*)"\s*$')
_INT_ASSIGN_RE = re.compile(r"^\s*(r\d+) = (-?\d+)(?:L)?\s*$")
_COPY_RE = re.compile(r"^\s*(?:\S+ )?(r\d+) = (r\d+)\s*$")
_INIT_RE = re.compile(r"^\s*(r\d+)\.<init>\(([^)]*)\)\s*$")
_CLASS_RE = re.compile(r"^public enum (\S+) ")


def parse_fallback_enum(text):
    """Return (class name, [(NAME, value)]) from a fallback-mode enum class.

    Registers are tracked linearly; only `<init>(name, ordinal, value, ...)`
    calls on objects `new`-ed with the enum's own class are taken.
    """
    cm = None
    for line in text.splitlines():
        cm = _CLASS_RE.match(line)
        if cm:
            break
    if not cm:
        raise ExtractError("not an enum class")
    cls = cm.group(1)
    strs, ints, types = {}, {}, {}
    consts = []
    for line in text.splitlines():
        m = _NEW_RE.match(line)
        if m and m.group(3) == cls:
            types[m.group(2)] = cls
            continue
        m = _STR_RE.match(line)
        if m:
            strs[m.group(1)] = unescape_java_string(m.group(2))
            ints.pop(m.group(1), None)
            continue
        m = _INT_ASSIGN_RE.match(line)
        if m:
            ints[m.group(1)] = int(m.group(2))
            strs.pop(m.group(1), None)
            continue
        m = _COPY_RE.match(line)
        if m:
            d, s = m.group(1), m.group(2)
            for table in (strs, ints, types):
                if s in table:
                    table[d] = table[s]
                else:
                    table.pop(d, None)
            continue
        m = _INIT_RE.match(line)
        if m and types.get(m.group(1)) == cls:
            a = [x.strip() for x in m.group(2).split(",")]
            if len(a) >= 3 and a[0] in strs and a[2] in ints:
                consts.append((strs[a[0]], ints[a[2]]))
    return cls, consts


def extract_enum_dir(enums_dir):
    """All fallback-decompiled enum classes under enums_dir -> {class: consts}."""
    result = {}
    if not enums_dir or not os.path.isdir(enums_dir):
        return result
    for dirpath, _dirs, files in os.walk(enums_dir):
        for f in sorted(files):
            if not f.endswith(".java"):
                continue
            with open(os.path.join(dirpath, f), encoding="utf-8",
                      errors="replace") as fh:
                text = fh.read()
            try:
                cls, consts = parse_fallback_enum(text)
            except ExtractError:
                continue
            if len(consts) >= 3:
                result[cls] = consts
    return result


# The decompiled enum classes carry no FIT type name (R8 removed it), so a class
# is attributed to a profile type only if it contains *all* of these (NAME, value)
# anchor pairs. Attribution is by content, never invented: a type with no match
# simply has no enum data.
ENUM_ANCHORS = {
    "file": [("ACTIVITY", 4), ("MONITORING_B", 32), ("SETTINGS", 2), ("SLEEP_DATA", 49)],
    "event_type": [("START", 0), ("STOP", 1), ("MARKER", 3)],
    "event": [("TIMER", 0), ("WORKOUT", 3), ("WORKOUT_STEP", 4)],
    "sport": [("GENERIC", 0), ("RUNNING", 1), ("CYCLING", 2), ("TRANSITION", 3),
              ("FITNESS_EQUIPMENT", 4), ("SWIMMING", 5), ("WALKING", 11)],
    "sub_sport": [("GENERIC", 0), ("TREADMILL", 1), ("STREET", 2), ("TRAIL", 3)],
    "activity_type": [("GENERIC", 0), ("RUNNING", 1), ("CYCLING", 2),
                      ("WALKING", 6), ("SEDENTARY", 8)],
    "language": [("ENGLISH", 0), ("FRENCH", 1), ("ITALIAN", 2), ("GERMAN", 3)],
    "gps_mode": [("OFF", 0), ("GPS", 1), ("GPS_GLONASS", 2), ("POWER_SAVING", 3)],
    "day_of_week": [("SUNDAY", 0), ("MONDAY", 1), ("TUESDAY", 2)],
    "bool": [("FALSE", 0), ("TRUE", 1)],
    "weather_report": [("CURRENT", 0), ("FORECAST", 1), ("DAILY_FORECAST", 2)],
    "weather_status": [("CLEAR", 0), ("PARTLY_CLOUDY", 1), ("MOSTLY_CLOUDY", 2), ("RAIN", 3)],
    "weather_severity": [("UNKNOWN", 0), ("WARNING", 1), ("WATCH", 2), ("ADVISORY", 3)],
    "golf_club": [("DRIVER", 1), ("WOOD_3", 2), ("WOOD_5", 3)],
}


def attribute_enums(class_consts, known_types=None):
    """-> ({type_name: [(value, name_lower)]}, [unattributed classes], [ambiguous])."""
    out, used, ambiguous = {}, set(), []
    for tname in sorted(ENUM_ANCHORS):
        if known_types is not None and tname not in known_types:
            continue  # only label enums with a type name the profile really uses
        anchors = ENUM_ANCHORS[tname]
        hits = []
        for cls in sorted(class_consts):
            have = set(class_consts[cls])
            if all(a in have for a in anchors):
                hits.append(cls)
        if len(hits) == 1:
            out[tname] = class_consts[hits[0]]
            used.add(hits[0])
        elif len(hits) > 1:
            # several classes share the anchors (e.g. sport/activity_type start the
            # same way): pick the largest only if the others are strict subsets
            hits.sort(key=lambda c: -len(class_consts[c]))
            sets = [set(class_consts[c]) for c in hits]
            if all(s <= sets[0] for s in sets[1:]):
                ambiguous.append((tname, hits))
            else:
                ambiguous.append((tname, hits))
    return out, sorted(set(class_consts) - used), ambiguous


# ---------------------------------------------------------------------------
# rendering

def rs_str(s):
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def rs_opt_f(v, default):
    if v == default:
        return "None"
    return "Some(%s)" % repr(float(v))


def rs_opt_s(s):
    return "None" if s == "" else "Some(%s)" % rs_str(s)


def rs_f(v):
    r = repr(float(v))
    return r


def rs_bt(name):
    return "BaseType::" + name.capitalize()


def render_component(c):
    return "comp(%d,%d,%s,%s,None,%s)" % (
        c["dest"], c["bits"], rs_opt_f(c["scale"], 1.0), rs_opt_f(c["offset"], 0.0),
        "true" if c["accumulate"] else "false")


def render_rust(messages, enums, mesg_nums):
    out = []
    w = out.append
    w("// @generated by tools/fit-profile/extract.py -- DO NOT EDIT.")
    w("// Derived from %s (obfuscated FIT SDK classes `wls`/`l5h` recovered with jadx)." % APP_VERSION)
    w("// LOCAL USE ONLY: git-ignored, must never be committed or published.")
    w("// Regenerate: python3 -I tools/fit-profile/extract.py <jadx sources> core/src/fit/generated")
    w("// The app exposes no FIT profile version string (only FIT protocol 2.0).")
    w("")
    w("pub(super) static MESSAGES: &[MessageInfo] = &[")
    nfields = 0
    for number in sorted(messages):
        msg = messages[number]
        w("    MessageInfo { global: %d, name: %s, fields: &[" % (number, rs_str(msg["name"])))
        seen = set()
        for f in sorted(msg["fields"], key=lambda f: f["num"]):
            if f["num"] in seen:
                continue
            seen.add(f["num"])
            nfields += 1
            subs = ",".join(
                "sub(%s,%s,%s,%s,%s,%s,&[%s],&[%s])" % (
                    rs_str(s["name"]), rs_bt(s["base_type"]), rs_str(s["base_type"]),
                    rs_opt_f(s["scale"], 1.0), rs_opt_f(s["offset"], 0.0),
                    rs_opt_s(s["units"]),
                    ",".join("(%d,%d)" % r for r in s["refs"]),
                    ",".join(render_component(c) for c in s["components"]))
                for s in f["subfields"])
            comps = ",".join(render_component(c) for c in f["components"])
            w("        fe(%d,%s,%s,%s,%s,%s,%s,%s,&[%s],&[%s])," % (
                f["num"], rs_str(f["name"]), rs_bt(f["base_type"]),
                rs_str(f["type_name"]), rs_opt_f(f["scale"], 1.0),
                rs_opt_f(f["offset"], 0.0), rs_opt_s(f["units"]),
                "true" if f["accumulated"] else "false", comps, subs))
        w("    ] },")
    w("];")
    w("")
    w("/// (type name, [(value, name)]) sorted by type name then value.")
    w("pub(super) static ENUMS: &[(&str, &[(u64, &str)])] = &[")
    all_enums = dict(enums)
    all_enums["mesg_num"] = [(k, v) for k, v in mesg_nums.items()]
    nvals = 0
    for tname in sorted(all_enums):
        vals = {}
        for item in all_enums[tname]:
            name, value = (item if isinstance(item[0], str) else (item[1], item[0]))
            if value < 0:
                continue
            vals.setdefault(value, name.lower())
        nvals += len(vals)
        w("    (%s, &[%s])," % (rs_str(tname), ", ".join(
            "(%d, %s)" % (v, rs_str(vals[v])) for v in sorted(vals))))
    w("];")
    w("")
    w("pub(super) static VERSION: Option<u32> = None;")
    w("")
    return "\n".join(out), (len(messages), nfields, len(all_enums), nvals)


def main(argv):
    args = [a for a in argv[1:] if not a.startswith("--")]
    enums_dir = None
    for i, a in enumerate(argv):
        if a == "--enums-dir":
            enums_dir = argv[i + 1]
            args = [x for x in args if x != enums_dir]
    if len(args) != 2:
        sys.stderr.write(__doc__)
        return 2
    root, out_path = args
    if enums_dir is None:
        enums_dir = os.path.join(root, "..", "..", "enums_fallback")
    resolver = Resolver(root)
    messages = extract_messages(root, resolver)
    mesg_nums = extract_mesg_nums(root, resolver)
    class_consts = extract_enum_dir(enums_dir)
    known = {f["type_name"] for m in messages.values() for f in m["fields"]}
    enums, unattributed, ambiguous = attribute_enums(class_consts, known)
    text, stats = render_rust(messages, enums, mesg_nums)
    # out_path is a directory (core/src/fit/generated); the file is git-ignored.
    os.makedirs(out_path, exist_ok=True)
    with open(os.path.join(out_path, "profile_data.rs"), "w", encoding="utf-8") as fh:
        fh.write(text)
    sys.stderr.write("messages=%d fields=%d enum_types=%d enum_values=%d\n" % stats)
    sys.stderr.write("enum classes found=%d attributed=%d unattributed=%s\n" % (
        len(class_consts), len(enums), ",".join(unattributed)))
    for t, hits in ambiguous:
        sys.stderr.write("AMBIGUOUS enum %s: %s\n" % (t, ",".join(hits)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))

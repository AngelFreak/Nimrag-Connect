#!/usr/bin/env python3
"""Recover .proto schemas from jadx-decompiled protobuf-java generated classes.

Primary source: every generated outer class embeds its serialized
FileDescriptorProto as Java string literals passed to
``Descriptors.FileDescriptor.internalBuildGeneratedFileFrom(new String[]{...})``.
Those bytes are decoded with a small self-contained wire-format reader (no
third-party imports) and rendered back to .proto text.

Cross-check: the Java class bodies are scanned (brace-aware, string-aware) for
``*_FIELD_NUMBER`` constants, enum constants ``NAME(n)``, oneof ``XxxCase``
enums and proto2 ``hasXxx()`` accessors. A field whose descriptor entry is not
confirmed by its Java constant is marked ``// inferred``.

Usage (run isolated, it only reads the decompiled sources):
    python3 -I tools/proto-recover/recover.py [--src DIR] [--out DIR]
                                              [--descriptor-set FILE]
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DEFAULT_SRC = REPO / "private/re/jadx/base/sources/com/garmin/proto/generated"
DEFAULT_OUT = REPO / "docs/protocol/proto"
APP = "Garmin Connect 5.31 (11761)"

# --------------------------------------------------------------------------
# Java source -> descriptor bytes
# --------------------------------------------------------------------------

_JAVA_SIMPLE_ESCAPES = {
    "n": "\n", "t": "\t", "b": "\b", "r": "\r", "f": "\f",
    "s": " ", '"': '"', "'": "'", "\\": "\\",
}


def decode_java_literal(body):
    """Decode the inside of a Java string literal (no surrounding quotes)."""
    out = []
    i = 0
    n = len(body)
    while i < n:
        c = body[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        if i + 1 >= n:
            raise ValueError("dangling backslash in Java literal")
        e = body[i + 1]
        if e == "u":
            j = i + 1
            while j < n and body[j] == "u":
                j += 1
            out.append(chr(int(body[j:j + 4], 16)))
            i = j + 4
        elif e in "01234567":
            j = i + 1
            limit = 3 if e in "0123" else 2
            while j < n and j - (i + 1) < limit and body[j] in "01234567":
                j += 1
            out.append(chr(int(body[i + 1:j], 8)))
            i = j
        elif e in _JAVA_SIMPLE_ESCAPES:
            out.append(_JAVA_SIMPLE_ESCAPES[e])
            i += 2
        else:
            raise ValueError("unknown Java escape \\%s" % e)
    return "".join(out)


_DESC_CALL = "internalBuildGeneratedFileFrom(new String[]{"


def extract_descriptor_bytes(java_src):
    """Return the serialized FileDescriptorProto embedded in a generated class."""
    start = java_src.find(_DESC_CALL)
    if start < 0:
        return None
    i = start + len(_DESC_CALL)
    parts = []
    n = len(java_src)
    while i < n:
        c = java_src[i]
        if c in " \t\r\n,+":
            i += 1
        elif c == '"':
            j = i + 1
            while java_src[j] != '"':
                j += 2 if java_src[j] == "\\" else 1
            parts.append(decode_java_literal(java_src[i + 1:j]))
            i = j + 1
        elif c == "}":
            break
        else:
            raise ValueError("unexpected %r in descriptor array" % c)
    text = "".join(parts)
    # protoc emits descriptor bytes as ISO-8859-1 characters.
    return text.encode("latin-1")


# --------------------------------------------------------------------------
# Minimal protobuf wire-format reader + descriptor.proto schema
# --------------------------------------------------------------------------

def read_varint(buf, pos):
    result = 0
    shift = 0
    while True:
        b = buf[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            return result, pos
        shift += 7
        if shift > 70:
            raise ValueError("varint too long")


def iter_fields(buf):
    pos = 0
    n = len(buf)
    while pos < n:
        key, pos = read_varint(buf, pos)
        num, wt = key >> 3, key & 7
        if wt == 0:
            val, pos = read_varint(buf, pos)
        elif wt == 1:
            val, pos = buf[pos:pos + 8], pos + 8
        elif wt == 2:
            ln, pos = read_varint(buf, pos)
            val, pos = buf[pos:pos + ln], pos + ln
        elif wt == 5:
            val, pos = buf[pos:pos + 4], pos + 4
        else:
            raise ValueError("unsupported wire type %d" % wt)
        yield num, wt, val


def _int32(v):
    v &= (1 << 64) - 1
    return v - (1 << 64) if v >= 1 << 63 else v


# Schema: message name -> {field number: (attr name, kind, repeated)}
# kinds: "str", "bytes", "int", "bool", or another schema message name.
SCHEMA = {
    "FileDescriptorSet": {1: ("file", "FileDescriptorProto", True)},
    "FileDescriptorProto": {
        1: ("name", "str", False), 2: ("package", "str", False),
        3: ("dependency", "str", True), 10: ("public_dependency", "int", True),
        11: ("weak_dependency", "int", True),
        4: ("message_type", "DescriptorProto", True),
        5: ("enum_type", "EnumDescriptorProto", True),
        6: ("service", "ServiceDescriptorProto", True),
        7: ("extension", "FieldDescriptorProto", True),
        8: ("options", "FileOptions", False),
        9: ("source_code_info", "bytes", False),
        12: ("syntax", "str", False), 14: ("edition", "int", False),
    },
    "DescriptorProto": {
        1: ("name", "str", False), 2: ("field", "FieldDescriptorProto", True),
        6: ("extension", "FieldDescriptorProto", True),
        3: ("nested_type", "DescriptorProto", True),
        4: ("enum_type", "EnumDescriptorProto", True),
        5: ("extension_range", "ExtensionRange", True),
        8: ("oneof_decl", "OneofDescriptorProto", True),
        7: ("options", "MessageOptions", False),
        9: ("reserved_range", "ReservedRange", True),
        10: ("reserved_name", "str", True),
    },
    "ExtensionRange": {1: ("start", "int", False), 2: ("end", "int", False),
                       3: ("options", "bytes", False)},
    "ReservedRange": {1: ("start", "int", False), 2: ("end", "int", False)},
    "FieldDescriptorProto": {
        1: ("name", "str", False), 3: ("number", "int", False),
        4: ("label", "int", False), 5: ("type", "int", False),
        6: ("type_name", "str", False), 2: ("extendee", "str", False),
        7: ("default_value", "str", False), 9: ("oneof_index", "int", False),
        10: ("json_name", "str", False), 8: ("options", "FieldOptions", False),
        17: ("proto3_optional", "bool", False),
    },
    "OneofDescriptorProto": {1: ("name", "str", False),
                             2: ("options", "bytes", False)},
    "EnumDescriptorProto": {
        1: ("name", "str", False),
        2: ("value", "EnumValueDescriptorProto", True),
        3: ("options", "EnumOptions", False),
        4: ("reserved_range", "ReservedRange", True),
        5: ("reserved_name", "str", True),
    },
    "EnumValueDescriptorProto": {1: ("name", "str", False),
                                 2: ("number", "int", False),
                                 3: ("options", "EnumValueOptions", False)},
    "ServiceDescriptorProto": {1: ("name", "str", False),
                               2: ("method", "MethodDescriptorProto", True),
                               3: ("options", "bytes", False)},
    "MethodDescriptorProto": {
        1: ("name", "str", False), 2: ("input_type", "str", False),
        3: ("output_type", "str", False), 4: ("options", "bytes", False),
        5: ("client_streaming", "bool", False),
        6: ("server_streaming", "bool", False),
    },
    "FileOptions": {
        1: ("java_package", "str", False),
        8: ("java_outer_classname", "str", False),
        10: ("java_multiple_files", "bool", False),
        20: ("java_generate_equals_and_hash", "bool", False),
        27: ("java_string_check_utf8", "bool", False),
        9: ("optimize_for", "int", False), 11: ("go_package", "str", False),
        16: ("cc_generic_services", "bool", False),
        17: ("java_generic_services", "bool", False),
        18: ("py_generic_services", "bool", False),
        23: ("deprecated", "bool", False),
        31: ("cc_enable_arenas", "bool", False),
        36: ("objc_class_prefix", "str", False),
        37: ("csharp_namespace", "str", False),
        39: ("swift_prefix", "str", False),
    },
    "MessageOptions": {1: ("message_set_wire_format", "bool", False),
                       2: ("no_standard_descriptor_accessor", "bool", False),
                       3: ("deprecated", "bool", False),
                       7: ("map_entry", "bool", False)},
    "FieldOptions": {1: ("ctype", "int", False), 2: ("packed", "bool", False),
                     3: ("deprecated", "bool", False), 5: ("lazy", "bool", False),
                     6: ("jstype", "int", False), 10: ("weak", "bool", False)},
    "EnumOptions": {2: ("allow_alias", "bool", False),
                    3: ("deprecated", "bool", False)},
    "EnumValueOptions": {1: ("deprecated", "bool", False)},
}


def parse_message(buf, schema_name):
    """Decode ``buf`` into a dict per SCHEMA. Unknown fields go to ``_unknown``."""
    spec = SCHEMA[schema_name]
    msg = {}
    for num, wt, val in iter_fields(buf):
        if num not in spec:
            msg.setdefault("_unknown", []).append((num, wt))
            continue
        attr, kind, repeated = spec[num]
        if kind == "str":
            v = val.decode("utf-8")
        elif kind == "bytes":
            v = bytes(val)
        elif kind == "int":
            if wt == 2:  # packed repeated
                vals, p = [], 0
                while p < len(val):
                    x, p = read_varint(val, p)
                    vals.append(_int32(x))
                msg.setdefault(attr, []).extend(vals)
                continue
            v = _int32(val)
        elif kind == "bool":
            v = bool(val)
        else:
            v = parse_message(val, kind)
        if repeated:
            msg.setdefault(attr, []).append(v)
        else:
            msg[attr] = v
    return msg


# --------------------------------------------------------------------------
# Naming
# --------------------------------------------------------------------------

# Multi-letter tokens kept whole when snake-casing proto file names.
_ACRONYMS = ["GDI", "HTTP", "GNCS", "LTE", "HSA", "PSA", "SaMD", "SD", "IQ",
             "UI", "ECG", "LTE"]
_TOKEN_RE = re.compile(
    "|".join(sorted((re.escape(a) for a in set(_ACRONYMS)), key=len, reverse=True))
    + r"|[A-Z]+(?=[A-Z][a-z]|\d|$)|[A-Z]?[a-z]+|\d+")


def snake_file_name(proto_name):
    """'GDIConnectIQHTTP.proto' -> 'gdi_connect_iq_http.proto'."""
    stem = proto_name[:-len(".proto")] if proto_name.endswith(".proto") else proto_name
    tokens = _TOKEN_RE.findall(stem)
    if "".join(tokens) != stem:
        raise ValueError("cannot tokenise %r -> %r" % (stem, tokens))
    return "_".join(t.lower() for t in tokens) + ".proto"


# --------------------------------------------------------------------------
# Java cross-check scanner
# --------------------------------------------------------------------------

_JAVA_TOKEN_RE = re.compile(
    r'"(?:\\.|[^"\\])*"'                       # string literal
    r"|'(?:\\.|[^'\\])*'"                      # char literal
    r"|//[^\n]*|/\*.*?\*/"                     # comments
    r"|\b(?P<kw>class|enum|interface)\s+(?P<name>\w+)"
    r"|(?P<brace>[{}])"
    r"|\b(?P<const>[A-Z0-9_]+)_FIELD_NUMBER\s*=\s*(?P<cnum>-?\d+)"
    r"|^[ \t]*(?P<ev>[A-Za-z_][A-Za-z0-9_]*)\((?P<evnum>-?\d+)\)\s*[,;]"
    r"|\b(?P<vconst>[A-Za-z0-9_]+)_VALUE\s*=\s*(?P<vnum>-?\d+)\s*;"
    r"|\b(?P<has>has[A-Z]\w*)\(\)",
    re.S | re.M)


def scan_java(java_src):
    """Map dotted Java class path (outer excluded) -> facts found in its body."""
    classes = {}
    stack = []           # list of (name or None, kind)
    pending = None
    for m in _JAVA_TOKEN_RE.finditer(java_src):
        if m.group("kw"):
            pending = (m.group("name"), m.group("kw"))
        elif m.group("brace") == "{":
            stack.append(pending if pending else (None, None))
            pending = None
            if stack[-1][0] is not None:
                path = ".".join(s[0] for s in stack[1:] if s[0] is not None)
                classes.setdefault(path, {"kind": stack[-1][1], "consts": {},
                                          "enum_values": {}, "value_consts": {},
                                          "has": set()})
        elif m.group("brace") == "}":
            if stack:
                stack.pop()
        else:
            named = [s for s in stack if s[0] is not None]
            if not named or stack[-1][0] is None:
                continue
            path = ".".join(s[0] for s in named[1:])
            info = classes.get(path)
            if info is None:
                continue
            if m.group("const"):
                info["consts"][m.group("const")] = int(m.group("cnum"))
            elif m.group("ev") and info["kind"] == "enum":
                info["enum_values"][m.group("ev")] = int(m.group("evnum"))
            elif m.group("vconst") and info["kind"] == "enum":
                info["value_consts"][m.group("vconst")] = int(m.group("vnum"))
            elif m.group("has"):
                info["has"].add(m.group("has"))
    return classes


def _camel(name):
    """protoc's UnderscoresToCamelCase(name, cap_first=True) as used by Java."""
    out = []
    cap_next = True
    for c in name:
        if "a" <= c <= "z":
            out.append(c.upper() if cap_next else c)
            cap_next = False
        elif "A" <= c <= "Z":
            out.append(c)
            cap_next = False
        elif "0" <= c <= "9":
            out.append(c)
            cap_next = True
        else:
            cap_next = True
    return "".join(out)


# --------------------------------------------------------------------------
# .proto rendering
# --------------------------------------------------------------------------

TYPE_NAMES = {1: "double", 2: "float", 3: "int64", 4: "uint64", 5: "int32",
              6: "fixed64", 7: "fixed32", 8: "bool", 9: "string", 12: "bytes",
              13: "uint32", 15: "sfixed32", 16: "sfixed64", 17: "sint32",
              18: "sint64"}
TYPE_GROUP, TYPE_MESSAGE, TYPE_ENUM = 10, 11, 14
LABELS = {1: "optional", 2: "required", 3: "repeated"}
OPTIMIZE = {1: "SPEED", 2: "CODE_SIZE", 3: "LITE_RUNTIME"}
CTYPE = {0: "STRING", 1: "CORD", 2: "STRING_PIECE"}
JSTYPE = {0: "JS_NORMAL", 1: "JS_STRING", 2: "JS_NUMBER"}
MAX_FIELD = 536870912  # exclusive upper bound for "max"


def _escape_string(text):
    out = []
    for b in text.encode("utf-8"):
        c = chr(b)
        if c in '"\\':
            out.append("\\" + c)
        elif 0x20 <= b < 0x7F:
            out.append(c)
        else:
            out.append("\\%03o" % b)
    return "".join(out)


class Symbols:
    """All fully-qualified type names, plus children per scope."""

    def __init__(self):
        self.children = {}   # scope fqn -> set of child simple names

    def add_file(self, fd):
        pkg = fd.get("package", "")

        def walk(scope, msgs, enums):
            kids = self.children.setdefault(scope, set())
            for e in enums:
                kids.add(e["name"])
            for msg in msgs:
                kids.add(msg["name"])
                for f in msg.get("field", []):
                    kids.add(f["name"])
                walk(scope + "." + msg["name"], msg.get("nested_type", []),
                     msg.get("enum_type", []))

        walk("." + pkg if pkg else "", fd.get("message_type", []),
             fd.get("enum_type", []))


class Renderer:
    def __init__(self, fd, symbols, java_classes, file_map, smart_map=None):
        self.fd = fd
        self.pkg = fd.get("package", "")
        self.syntax = fd.get("syntax", "proto2")
        self.symbols = symbols
        self.java = java_classes
        self.file_map = file_map
        self.smart_map = smart_map
        self.smart_users = {}   # (extendee, number) -> [extension names]
        self.lines = []
        self.stats = {"messages": 0, "enums": 0, "fields": 0, "inferred": 0,
                      "oneofs": 0, "extensions": 0, "notes": []}

    # -- helpers ----------------------------------------------------------
    def emit(self, depth=0, text=""):
        self.lines.append(("  " * depth + text) if text else "")

    def type_ref(self, fqn, scope):
        """Shortest safe spelling of ``fqn`` referenced from message ``scope``."""
        prefix = "." + self.pkg + "." if self.pkg else "."
        if not fqn.startswith(prefix):
            return fqn
        rel = fqn[len(prefix):]
        first = rel.split(".")[0]
        # Walk enclosing message scopes inner -> outer; any other symbol with
        # the same first component would capture the lookup.
        s = scope
        while len(s) > len(prefix) - 1:
            if first in self.symbols.children.get(s, ()) and s + "." + first != prefix + first:
                return fqn
            s = s.rsplit(".", 1)[0]
        return rel

    def java_info(self, path):
        return self.java.get(".".join(path))

    def field_options(self, f):
        opts = []
        o = f.get("options", {})
        if "default_value" in f:
            dv = f["default_value"]
            if f.get("type") == 9:
                dv = '"%s"' % _escape_string(dv)
            elif f.get("type") == 12:
                dv = '"%s"' % dv
            opts.append("default = %s" % dv)
        if "ctype" in o:
            opts.append("ctype = %s" % CTYPE[o["ctype"]])
        if "packed" in o:
            opts.append("packed = %s" % str(o["packed"]).lower())
        if "jstype" in o:
            opts.append("jstype = %s" % JSTYPE[o["jstype"]])
        for k in ("lazy", "deprecated", "weak"):
            if k in o:
                opts.append("%s = %s" % (k, str(o[k]).lower()))
        if "_unknown" in o:
            self.stats["notes"].append("unrendered field option on %s" % f["name"])
        return " [%s]" % ", ".join(opts) if opts else ""

    def check_field(self, f, path, info, is_ext=False):
        """Return (inferred, reason) from the Java cross-check."""
        if info is None:
            return True, "no Java class %s" % ".".join(path)
        const = f["name"].upper()
        if f.get("type") == TYPE_GROUP:
            const = f["type_name"].rsplit(".", 1)[-1].upper()
        got = info["consts"].get(const)
        if got != f["number"]:
            return True, "Java %s_FIELD_NUMBER=%s vs descriptor %d" % (
                const, got, f["number"])
        if (not is_ext and self.syntax == "proto2" and f.get("label") != 3
                and "oneof_index" not in f
                and ("has" + _camel(f["name"])) not in info["has"]):
            return True, "proto2 singular field without has%s()" % _camel(f["name"])
        return False, ""

    def field_line(self, f, scope, path, info, in_oneof=False, map_entries=None,
                   is_ext=False):
        self.stats["fields"] += 1
        t = f.get("type")
        label = LABELS.get(f.get("label", 1))
        if t in (TYPE_MESSAGE, TYPE_ENUM, TYPE_GROUP) or "type_name" in f:
            tname = self.type_ref(f["type_name"], scope)
        else:
            tname = TYPE_NAMES[t]
        entry = (map_entries or {}).get(f.get("type_name"))
        if entry is not None:
            kf, vf = entry["field"][0], entry["field"][1]
            kt = TYPE_NAMES[kf["type"]]
            vt = (self.type_ref(vf["type_name"], scope) if "type_name" in vf
                  else TYPE_NAMES[vf["type"]])
            decl = "map<%s, %s> %s = %d" % (kt, vt, f["name"], f["number"])
        else:
            if in_oneof:
                prefix = ""
            elif self.syntax == "proto3":
                prefix = ("optional " if f.get("proto3_optional")
                          else "repeated " if label == "repeated" else "")
            else:
                prefix = label + " "
            decl = "%s%s %s = %d" % (prefix, tname, f["name"], f["number"])
        decl += self.field_options(f) + ";"
        inferred, why = self.check_field(f, path, info, is_ext)
        if inferred:
            self.stats["inferred"] += 1
            self.stats["notes"].append("%s.%s: %s" % (".".join(path), f["name"], why))
            decl += "  // inferred"
        return decl

    # -- blocks -----------------------------------------------------------
    def render_enum(self, e, depth, path):
        self.stats["enums"] += 1
        epath = path + [e["name"]]
        info = self.java_info(epath)
        self.emit(depth, "enum %s {" % e["name"])
        o = e.get("options", {})
        if o.get("allow_alias"):
            self.emit(depth + 1, "option allow_alias = true;")
        if o.get("deprecated"):
            self.emit(depth + 1, "option deprecated = true;")
        for v in e.get("value", []):
            line = "%s = %d" % (v["name"], v["number"])
            if v.get("options", {}).get("deprecated"):
                line += " [deprecated = true]"
            line += ";"
            jv = jc = None
            if info:
                jv = info["enum_values"].get(v["name"])
                jc = info["value_consts"].get(v["name"])
            # Constants appear either as NAME(n) or NAME(NAME_VALUE) with
            # NAME_VALUE = n; accept either, reject any disagreement.
            if v["number"] not in (jv, jc) or jv not in (None, v["number"]):
                self.stats["inferred"] += 1
                self.stats["notes"].append("%s.%s: Java enum constant %s" % (
                    ".".join(epath), v["name"], jv))
                line += "  // inferred"
            self.emit(depth + 1, line)
        self.render_reserved(e, depth + 1, inclusive_end=True)
        self.emit(depth, "}")

    def render_reserved(self, d, depth, inclusive_end):
        parts = []
        for r in d.get("reserved_range", []):
            end = r["end"] if inclusive_end else r["end"] - 1
            if r["start"] == end:
                parts.append(str(r["start"]))
            else:
                parts.append("%d to %s" % (r["start"],
                                           "max" if end in (MAX_FIELD - 1, 2**31 - 1) else end))
        if parts:
            self.emit(depth, "reserved %s;" % ", ".join(parts))
        names = d.get("reserved_name", [])
        if names:
            self.emit(depth, "reserved %s;" % ", ".join('"%s"' % n for n in names))

    def render_extensions(self, exts, depth, scope, path, info):
        by_extendee = {}
        for x in exts:
            by_extendee.setdefault(x["extendee"], []).append(x)
        for extendee, fields in by_extendee.items():
            self.emit(depth, "extend %s {" % self.type_ref(extendee, scope))
            for x in fields:
                self.stats["extensions"] += 1
                line = self.field_line(x, scope, path, info, is_ext=True)
                others = [o for o in self.smart_users.get((extendee, x["number"]), [])
                          if o != x["name"]]
                if others:
                    line += ("  // SEEN: number %d is also bound by %s in another "
                             "file of this app" % (x["number"], ", ".join(others)))
                self.emit(depth + 1, line)
            self.emit(depth, "}")

    def render_message(self, m, depth, scope, path):
        self.stats["messages"] += 1
        mscope = scope + "." + m["name"]
        mpath = path + [m["name"]]
        info = self.java_info(mpath)
        if info is None:
            self.stats["notes"].append("no Java class for message %s" % ".".join(mpath))
        self.emit(depth, "message %s {" % m["name"])
        o = m.get("options", {})
        if o.get("message_set_wire_format"):
            self.emit(depth + 1, "option message_set_wire_format = true;")
        if o.get("deprecated"):
            self.emit(depth + 1, "option deprecated = true;")
        if o.get("no_standard_descriptor_accessor"):
            self.emit(depth + 1, "option no_standard_descriptor_accessor = true;")

        nested = m.get("nested_type", [])
        map_entries = {mscope + "." + n["name"]: n for n in nested
                       if n.get("options", {}).get("map_entry")}
        group_types = {mscope + "." + n["name"] for n in nested}
        oneofs = m.get("oneof_decl", [])
        done_oneofs = set()
        fields = m.get("field", [])
        for f in fields:
            oi = f.get("oneof_index")
            if oi is not None and not f.get("proto3_optional"):
                if oi in done_oneofs:
                    continue
                done_oneofs.add(oi)
                self.stats["oneofs"] += 1
                oname = oneofs[oi]["name"]
                case = info and self.java_info(mpath + [_camel(oname) + "Case"])
                note = "" if case else "  // inferred (no Java %sCase enum)" % _camel(oname)
                self.emit(depth + 1, "oneof %s {%s" % (oname, note))
                for g in fields:
                    if g.get("oneof_index") == oi:
                        self.emit(depth + 2, self.field_line(g, mscope, mpath, info, True))
                self.emit(depth + 1, "}")
                continue
            if f.get("type") == TYPE_GROUP and f["type_name"] in group_types:
                self.stats["notes"].append("group field %s.%s" % (".".join(mpath), f["name"]))
            self.emit(depth + 1, self.field_line(f, mscope, mpath, info,
                                                 map_entries=map_entries))
        for r in m.get("extension_range", []):
            end = "max" if r["end"] >= MAX_FIELD else str(r["end"] - 1)
            rng = str(r["start"]) if end == str(r["start"]) else "%d to %s" % (r["start"], end)
            self.emit(depth + 1, "extensions %s;" % rng)
        self.render_reserved(m, depth + 1, inclusive_end=False)
        if m.get("extension"):
            self.render_extensions(m["extension"], depth + 1, mscope, mpath, info)
        for e in m.get("enum_type", []):
            self.emit()
            self.render_enum(e, depth + 1, mpath)
        for n in nested:
            if mscope + "." + n["name"] in map_entries:
                continue
            self.emit()
            self.render_message(n, depth + 1, mscope, mpath)
        self.emit(depth, "}")

    def render(self, outer):
        fd = self.fd
        self.emit(0, "// Recovered from %s com.garmin.proto.generated.%s by "
                     "tools/proto-recover. Field names/numbers are SEEN (from "
                     "generated constants); types marked \"// inferred\" where the "
                     "type could not be read directly." % (APP, outer))
        self.emit(0, "// Types, labels, oneofs and enums are read from the "
                     "FileDescriptorProto embedded in the class; numbers are "
                     "cross-checked against *_FIELD_NUMBER constants.")
        self.emit(0, "// Original descriptor file name: %s" % fd["name"])
        if self.smart_map:
            self.emit(0, "//")
            self.emit(0, "// Services bound into Smart via 'extend Smart' "
                         "(from the *Extension classes):")
            for num, name, typ, src in sorted(self.smart_map):
                self.emit(0, "//   %3d  %-40s %s  (%s)" % (num, name, typ, src))
        self.emit()
        self.emit(0, 'syntax = "%s";' % self.syntax)
        self.emit()
        if self.pkg:
            self.emit(0, "package %s;" % self.pkg)
            self.emit()
        public = set(fd.get("public_dependency", []))
        weak = set(fd.get("weak_dependency", []))
        deps = fd.get("dependency", [])
        for i, d in enumerate(deps):
            kind = "public " if i in public else "weak " if i in weak else ""
            self.emit(0, 'import %s"%s";' % (kind, self.file_map.get(d, d)))
        if deps:
            self.emit()
        o = fd.get("options", {})
        for key in ("java_package", "java_outer_classname", "go_package",
                    "objc_class_prefix", "csharp_namespace", "swift_prefix"):
            if key in o:
                self.emit(0, 'option %s = "%s";' % (key, o[key]))
        for key in ("java_multiple_files", "java_generate_equals_and_hash",
                    "java_string_check_utf8", "cc_generic_services",
                    "java_generic_services", "py_generic_services",
                    "deprecated", "cc_enable_arenas"):
            if key in o:
                self.emit(0, "option %s = %s;" % (key, str(o[key]).lower()))
        if "optimize_for" in o:
            self.emit(0, "option optimize_for = %s;" % OPTIMIZE[o["optimize_for"]])
        if "_unknown" in o:
            self.stats["notes"].append("unrendered file options %s" % o["_unknown"])
        scope = "." + self.pkg if self.pkg else ""
        info = self.java_info([])
        if fd.get("extension"):
            self.emit()
            self.render_extensions(fd["extension"], 0, scope, [], info)
        for e in fd.get("enum_type", []):
            self.emit()
            self.render_enum(e, 0, [])
        for m in fd.get("message_type", []):
            self.emit()
            self.render_message(m, 0, scope, [])
        for s in fd.get("service", []):
            self.emit()
            self.emit(0, "service %s {" % s["name"])
            for mt in s.get("method", []):
                self.emit(1, "rpc %s (%s%s) returns (%s%s);" % (
                    mt["name"], "stream " if mt.get("client_streaming") else "",
                    mt["input_type"], "stream " if mt.get("server_streaming") else "",
                    mt["output_type"]))
            self.emit(0, "}")
        return "\n".join(self.lines) + "\n"


# --------------------------------------------------------------------------
# Driver
# --------------------------------------------------------------------------

def _encode_varint(v):
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def load_sources(src_dir):
    """Return list of (outer class name, raw descriptor bytes, java facts)."""
    loaded = []
    for path in sorted(Path(src_dir).glob("*.java")):
        text = path.read_text(encoding="utf-8")
        raw = extract_descriptor_bytes(text)
        if raw is None:
            print("skip (no descriptor): %s" % path.name, file=sys.stderr)
            continue
        loaded.append((path.stem, raw, scan_java(text)))
    return loaded


def recover(src_dir, out_dir, descriptor_set=None):
    loaded = load_sources(src_dir)
    if not loaded:
        raise SystemExit("no generated classes with descriptors under %s" % src_dir)
    files = []
    for outer, raw, java in loaded:
        fd = parse_message(raw, "FileDescriptorProto")
        if fd.get("syntax", "proto2") not in ("proto2", "proto3"):
            raise SystemExit("%s: unsupported syntax %r" % (outer, fd.get("syntax")))
        if "_unknown" in fd:
            raise SystemExit("%s: unknown FileDescriptorProto fields %s" % (outer, fd["_unknown"]))
        files.append((outer, raw, java, fd))

    file_map = {fd["name"]: snake_file_name(fd["name"]) for _, _, _, fd in files}
    if len(set(file_map.values())) != len(file_map):
        raise SystemExit("output file name collision: %s" % file_map)
    symbols = Symbols()
    for _, _, _, fd in files:
        symbols.add_file(fd)

    smart_map = []
    smart_fqn = None
    for outer, _, _, fd in files:
        for m in fd.get("message_type", []):
            if m["name"] == "Smart" and m.get("extension_range"):
                smart_fqn = "." + fd["package"] + ".Smart"
    for outer, _, _, fd in files:
        for x in fd.get("extension", []):
            if x["extendee"] == smart_fqn:
                smart_map.append((x["number"], x["name"], x["type_name"], outer))

    ext_users = {}
    for _, _, _, fd in files:
        for x in fd.get("extension", []):
            ext_users.setdefault((x["extendee"], x["number"]), []).append(x["name"])

    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    totals = {"files": 0, "messages": 0, "enums": 0, "fields": 0,
              "inferred": 0, "oneofs": 0, "extensions": 0}
    notes = []
    for outer, raw, java, fd in files:
        is_smart = smart_fqn and any(
            "." + fd["package"] + "." + m["name"] == smart_fqn
            for m in fd.get("message_type", []))
        r = Renderer(fd, symbols, java, file_map, smart_map if is_smart else None)
        r.smart_users = ext_users
        text = r.render(outer)
        (out / file_map[fd["name"]]).write_text(text, encoding="utf-8")
        totals["files"] += 1
        for k in ("messages", "enums", "fields", "inferred", "oneofs", "extensions"):
            totals[k] += r.stats[k]
        notes.extend("%s: %s" % (outer, n) for n in r.stats["notes"])

    if descriptor_set:
        blob = b"".join(b"\x0a" + _encode_varint(len(raw)) + raw
                        for _, raw, _, _ in files)
        Path(descriptor_set).write_bytes(blob)

    # Duplicate extension numbers on Smart would be a real conflict.
    seen = {}
    for num, name, _, outer in smart_map:
        if num in seen:
            notes.append("Smart extension number %d used by %s and %s" % (num, seen[num], name))
        seen[num] = name
    return totals, notes, smart_map, file_map


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--src", default=str(DEFAULT_SRC))
    ap.add_argument("--out", default=str(DEFAULT_OUT))
    ap.add_argument("--descriptor-set",
                    help="also write the embedded descriptors as a FileDescriptorSet")
    ap.add_argument("--json", help="write a machine-readable summary here")
    args = ap.parse_args(argv)
    totals, notes, smart_map, file_map = recover(args.src, args.out, args.descriptor_set)
    print("recovered: " + ", ".join("%s=%d" % kv for kv in totals.items()))
    print("Smart extensions: %d" % len(smart_map))
    for n in notes:
        print("note: " + n)
    if args.json:
        Path(args.json).write_text(json.dumps({
            "totals": totals, "notes": notes, "file_map": file_map,
            "smart": [{"number": a, "name": b, "type": c, "outer": d}
                      for a, b, c, d in sorted(smart_map)]}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Structural diff between recovered app descriptors and a reference proto set.

Inputs are two FileDescriptorSets: the app's (``recover.py --descriptor-set``)
and one compiled from the reference .proto files with protoc. Output is a
Markdown list of differences keyed by message, using only names, numbers and
types (no reference text is reproduced).

    python3 -I tools/proto-recover/gb_diff.py APP.pb REF.pb [--packages P,...]
"""

import argparse
import importlib.util
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("recover", HERE / "recover.py")
recover = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(recover)

# Reference files that do not use the app's GDI.Proto.* package naming.
REF_PACKAGE_BY_FILE = {
    "gdi_realtime_settings.proto": "GDI.Proto.RealtimeSettings",
    "gdi_explore_sync.proto": "GDI.Proto.ExploreSync",
    "gdi_ecg_service.proto": "GDI.Proto.SaMD",
    "gdi_smart_proto.proto": "GDI.Proto.Smart",
}


def index(fds, remap_files=False):
    """fqn-in-app-namespace -> ('msg'|'enum', descriptor); also type renames."""
    types, rename = {}, {}
    for fd in fds:
        pkg = fd.get("package", "")
        app_pkg = pkg
        if remap_files:
            app_pkg = REF_PACKAGE_BY_FILE.get(Path(fd["name"]).name, pkg)

        def walk(msgs, enums, scope, app_scope):
            for e in enums:
                rename["." + scope + "." + e["name"]] = app_scope + "." + e["name"]
                types[app_scope + "." + e["name"]] = ("enum", e)
            for m in msgs:
                rename["." + scope + "." + m["name"]] = app_scope + "." + m["name"]
                types[app_scope + "." + m["name"]] = ("msg", m)
                walk(m.get("nested_type", []), m.get("enum_type", []),
                     scope + "." + m["name"], app_scope + "." + m["name"])

        walk(fd.get("message_type", []), fd.get("enum_type", []), pkg, app_pkg)
    return types, rename


def ftype(f, rename):
    if "type_name" in f:
        return rename.get(f["type_name"], f["type_name"].lstrip("."))
    return recover.TYPE_NAMES[f["type"]]


def label(f):
    return recover.LABELS.get(f.get("label", 1))


def short(t, pkg):
    return t[len(pkg) + 1:] if t.startswith(pkg + ".") else t


CAPS = ("GDI.Proto.Core.FeatureCapabilitiesRequest",
        "GDI.Proto.Core.FeatureCapabilitiesResponse")


class Differ:
    def __init__(self, app_fds, ref_fds):
        self.app_fds, self.ref_fds = app_fds, ref_fds
        self.atypes, self.arename = index(app_fds)
        self.rtypes, self.rrename = index(ref_fds, remap_files=True)
        self.pairs = {}      # app fqn -> ref fqn
        self.how = {}        # app fqn -> "name" | "structure"
        self.conflicts = []

    # -- pairing ----------------------------------------------------------
    def pair(self, a, r, how):
        if a not in self.atypes or r not in self.rtypes:
            return False
        if self.atypes[a][0] != self.rtypes[r][0]:
            return False
        if a in self.pairs:
            if self.pairs[a] != r:
                self.conflicts.append((a, self.pairs[a], r))
            return False
        self.pairs[a], self.how[a] = r, how
        return True

    def build_pairs(self, seeds):
        for name, (kind, _) in self.atypes.items():
            if name in self.rtypes and self.rtypes[name][0] == kind:
                self.pair(name, name, "name")
        queue = list(self.pairs.items())
        for a, r in seeds:
            if r.startswith("GDI.Proto.Smart."):
                continue  # reference placeholder, nothing to compare
            if self.pair(a, r, "structure"):
                queue.append((a, r))
        while queue:
            a, r = queue.pop()
            if self.atypes[a][0] != "msg":
                continue
            af = {f["number"]: f for f in self.atypes[a][1].get("field", [])}
            rf = {f["number"]: f for f in self.rtypes[r][1].get("field", [])}
            for n in set(af) & set(rf):
                if "type_name" in af[n] and "type_name" in rf[n]:
                    ta, tr = ftype(af[n], self.arename), ftype(rf[n], self.rrename)
                    if tr.startswith("GDI.Proto.Smart."):
                        continue  # reference placeholder
                    if self.pair(ta, tr, "structure"):
                        queue.append((ta, tr))

    def ref_type(self, fr, pkg):
        """Describe a reference field type without quoting reference names."""
        if "type_name" not in fr:
            return recover.TYPE_NAMES[fr["type"]]
        kind = self.rtypes.get(ftype(fr, self.rrename), ("type",))[0]
        return {"msg": "a message", "enum": "an enum"}.get(kind, "a named type")

    def same_type(self, fa, fr):
        ta, tr = ftype(fa, self.arename), ftype(fr, self.rrename)
        if "type_name" in fa and "type_name" in fr:
            return self.pairs.get(ta) == tr or ta == tr
        return ta == tr

    # -- per-type diffs ---------------------------------------------------
    def diff_message(self, name, pkg, defaults):
        am = self.atypes[name][1]
        rm = self.rtypes[self.pairs[name]][1]
        by_name = self.how[name] == "name"
        out = []
        af = {f["number"]: f for f in am.get("field", [])}
        rf = {f["number"]: f for f in rm.get("field", [])}
        for n in sorted(set(af) | set(rf)):
            if name in CAPS and n >= 10:
                continue  # covered by the capability-extension section
            a, r = af.get(n), rf.get(n)
            if r is None:
                out.append("app-only field %d `%s` (%s %s)" % (
                    n, a["name"], label(a), short(ftype(a, self.arename), pkg)))
                continue
            if a is None:
                out.append("reference-only field %d (%s %s)" % (
                    n, label(r), self.ref_type(r, pkg)))
                continue
            bits = []
            if by_name and a["name"] != r["name"]:
                bits.append("named differently in reference")
            if not self.same_type(a, r):
                bits.append("type app %s vs ref %s" % (
                    short(ftype(a, self.arename), pkg), self.ref_type(r, pkg)))
            if label(a) != label(r):
                bits.append("label app %s vs ref %s" % (label(a), label(r)))
            if ("oneof_index" in a) != ("oneof_index" in r):
                bits.append("oneof membership differs")
            if "default_value" in a and a.get("default_value") != r.get("default_value"):
                defaults.append("%s.%s=%s" % (short(name, pkg), a["name"], a["default_value"]))
            if bits:
                out.append("field %d `%s`: %s" % (n, a["name"], "; ".join(bits)))
        if name not in CAPS:
            ar = [(x["start"], x["end"] - 1) for x in am.get("extension_range", [])]
            rr = [(x["start"], x["end"] - 1) for x in rm.get("extension_range", [])]
            if ar != rr:
                out.append("extension ranges app %s vs ref %s" % (ar, rr))
        return out

    def diff_enum(self, name):
        ae = self.atypes[name][1]
        re_ = self.rtypes[self.pairs[name]][1]
        out = []
        av = {v["number"]: v["name"] for v in ae.get("value", [])}
        rv = {v["number"]: v["name"] for v in re_.get("value", [])}
        renamed = [n for n in set(av) & set(rv) if av[n] != rv[n]]
        app_only = sorted(set(av) - set(rv))
        ref_only = sorted(set(rv) - set(av))
        if app_only:
            out.append("app-only values: %s" % ", ".join("%s=%d" % (av[n], n) for n in app_only))
        if ref_only:
            out.append("reference-only values: %s" % ", ".join(str(n) for n in ref_only))
        if renamed:
            out.append("%d value(s) carry different names in reference (app names are authoritative)"
                       % len(renamed))
        return out

    # -- report -----------------------------------------------------------
    def report_package(self, pkg):
        lines, defaults, missing = [], [], []
        names = sorted(k for k in self.atypes if k.startswith(pkg + "."))
        for name in names:
            rel = short(name, pkg)
            if name not in self.pairs:
                parent = name.rsplit(".", 1)[0]
                if parent in self.atypes and parent not in self.pairs:
                    continue  # covered by the parent
                missing.append(rel)
                continue
            kind = self.atypes[name][0]
            d = (self.diff_message(name, pkg, defaults) if kind == "msg"
                 else self.diff_enum(name))
            tag = ""
            if self.how[name] == "structure":
                tag = " (reference names it differently; paired by field-number structure)"
            if d:
                lines.append("- `%s`%s:" % (rel, tag))
                lines.extend("    - " + x for x in d)
        paired_ref = set(self.pairs.values())
        ref_only = sorted(short(k, pkg) for k in self.rtypes
                          if k.startswith(pkg + ".") and k not in paired_ref
                          and k.rsplit(".", 1)[0] not in self.rtypes)
        if missing:
            lines.append("- no counterpart in reference: %s"
                         % ", ".join("`%s`" % m for m in missing))
        if ref_only:
            lines.append("- %d reference type(s) in this package could not be paired with an app type"
                         % len(ref_only))
        if defaults:
            lines.append("- proto2 defaults declared by the app but not by the reference: %s"
                         % ", ".join("`%s`" % d for d in defaults))
        return lines


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("app")
    ap.add_argument("ref")
    ap.add_argument("--packages", help="comma list of GDI.Proto.* packages to report")
    args = ap.parse_args(argv)
    app_fds = recover.parse_message(Path(args.app).read_bytes(), "FileDescriptorSet")["file"]
    ref_fds = recover.parse_message(Path(args.ref).read_bytes(), "FileDescriptorSet")["file"]
    d = Differ(app_fds, ref_fds)
    app_pkgs = sorted({fd["package"] for fd in app_fds})
    ref_pkgs = {k.rsplit(".", 1)[0] for k in d.rtypes}
    wanted = args.packages.split(",") if args.packages else app_pkgs
    seeds = []

    # Smart: app binds services via extensions, the reference via plain fields.
    print("## Smart (top-level service map)\n")
    app_smart = {}
    for fd in app_fds:
        for x in fd.get("extension", []):
            if x["extendee"] == ".GDI.Proto.Smart.Smart":
                app_smart.setdefault(x["number"], []).append(x)
    ref_smart = {f["number"]: f for f in d.rtypes.get("GDI.Proto.Smart.Smart", ("msg", {}))[1].get("field", [])}
    placeholders, same = [], 0
    for n in sorted(set(app_smart) | set(ref_smart)):
        a, r = app_smart.get(n, []), ref_smart.get(n)
        if r is None:
            for x in a:
                print("- %d: app-only `%s` -> %s" % (n, x["name"], ftype(x, d.arename)))
        elif not a:
            print("- %d: reference-only binding to an empty placeholder message" % n)
        else:
            for x in a:
                ta, tr = ftype(x, d.arename), ftype(r, d.rrename)
                seeds.append((ta, tr))
                if tr.startswith("GDI.Proto.Smart."):
                    placeholders.append(n)
                if x["name"] != r["name"]:
                    print("- %d: app `%s` vs reference name differs" % (n, x["name"]))
                elif not tr.startswith("GDI.Proto.Smart."):
                    same += 1
    print("- %d numbers agree in name with a real schema in the reference" % same)
    if placeholders:
        print("- reference binds these numbers only to an empty placeholder message: %s"
              % ", ".join(str(n) for n in placeholders))
    print()

    for cap in CAPS:
        print("## %s extensions\n" % cap)
        app_ext, ref_ext = {}, {}
        for fd in app_fds:
            for x in recover_all_extensions(fd):
                if x["extendee"] == "." + cap:
                    app_ext[x["number"]] = x
        for fd in ref_fds:
            for x in recover_all_extensions(fd):
                if d.rrename.get(x["extendee"], x["extendee"]) == cap:
                    ref_ext[x["number"]] = x
        for f in d.rtypes.get(cap, ("msg", {}))[1].get("field", []):
            if f["number"] >= 10:
                ref_ext.setdefault(f["number"], f)
        agree = 0
        for n in sorted(set(app_ext) | set(ref_ext)):
            a, r = app_ext.get(n), ref_ext.get(n)
            if r is None:
                print("- %d: app-only `%s` (%s)" % (n, a["name"], ftype(a, d.arename)))
            elif a is None:
                print("- %d: reference-only binding" % n)
            else:
                seeds.append((ftype(a, d.arename), ftype(r, d.rrename)))
                if ftype(a, d.arename) != ftype(r, d.rrename):
                    print("- %d: type app %s vs ref %s" % (n, ftype(a, d.arename), ftype(r, d.rrename)))
                else:
                    agree += 1
        print("- %d numbers agree (the reference declares them as plain fields, the app as extensions)" % agree)
        print()

    d.build_pairs(seeds)
    if d.conflicts:
        print("## App messages the reference models as several distinct messages\n")
        for a in sorted({c[0] for c in d.conflicts}):
            print("- `%s`" % a)
        print()
    for pkg in wanted:
        if pkg == "GDI.Proto.Smart":
            continue
        print("## %s\n" % pkg)
        if pkg not in ref_pkgs and not any(k.startswith(pkg + ".") for k in d.pairs):
            n = sum(1 for k, v in d.atypes.items() if k.rsplit(".", 1)[0] == pkg and v[0] == "msg")
            print("- reference has no file for this package (%d top-level app messages)\n" % n)
            continue
        lines = d.report_package(pkg)
        print("\n".join(lines) if lines else "- no differences")
        print()
    return 0


def recover_all_extensions(fd):
    out = list(fd.get("extension", []))

    def walk(msgs):
        for m in msgs:
            out.extend(m.get("extension", []))
            walk(m.get("nested_type", []))
    walk(fd.get("message_type", []))
    return out


if __name__ == "__main__":
    sys.exit(main())

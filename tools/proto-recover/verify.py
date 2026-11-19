#!/usr/bin/env python3
"""Round-trip check for recover.py output.

Compiles every emitted .proto with protoc (one file per invocation, so the
app's genuine duplicate Smart extension number does not collide), then decodes
the resulting FileDescriptorProto and compares it with the descriptor embedded
in the decompiled class (written by ``recover.py --descriptor-set``).

Usage:
    python3 -I tools/proto-recover/verify.py --protoc "python -m grpc_tools.protoc" \
        --orig ORIG.pb [--proto-dir docs/protocol/proto]
Exit status 0 only if every file compiles and matches its original descriptor.
"""

import argparse
import importlib.util
import shlex
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("recover", HERE / "recover.py")
recover = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(recover)


def normalise(node, file_map):
    """Drop fields protoc fills in on its own and rename files to emitted names."""
    if isinstance(node, list):
        return [normalise(x, file_map) for x in node]
    if not isinstance(node, dict):
        return node
    out = {}
    for k, v in node.items():
        if k == "json_name":
            continue
        if k == "name" and isinstance(v, str) and v.endswith(".proto"):
            v = file_map.get(v, v)
        if k == "dependency":
            v = [file_map.get(d, d) for d in v]
        out[k] = normalise(v, file_map)
    return out


def diff(a, b, path="", out=None):
    out = [] if out is None else out
    if type(a) is not type(b):
        out.append("%s: %r != %r" % (path, a, b))
    elif isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                out.append("%s.%s only in %s" % (path, k, "original" if k in a else "emitted"))
            else:
                diff(a[k], b[k], "%s.%s" % (path, k), out)
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append("%s: length %d != %d" % (path, len(a), len(b)))
        for i, (x, y) in enumerate(zip(a, b)):
            diff(x, y, "%s[%d]" % (path, i), out)
    elif a != b:
        out.append("%s: %r != %r" % (path, a, b))
    return out


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("--protoc", required=True, help="protoc command line")
    ap.add_argument("--orig", required=True, help="FileDescriptorSet from recover.py")
    ap.add_argument("--proto-dir", default=str(recover.DEFAULT_OUT))
    args = ap.parse_args(argv)

    orig = recover.parse_message(Path(args.orig).read_bytes(), "FileDescriptorSet")["file"]
    file_map = {fd["name"]: recover.snake_file_name(fd["name"]) for fd in orig}
    protoc = shlex.split(args.protoc)
    failures = 0
    with tempfile.TemporaryDirectory() as tmp:
        for fd in orig:
            name = file_map[fd["name"]]
            out = Path(tmp) / (name + ".pb")
            res = subprocess.run(protoc + ["--proto_path=" + args.proto_dir,
                                           "--descriptor_set_out=" + str(out), name],
                                 capture_output=True, text=True)
            if res.returncode != 0:
                failures += 1
                print("COMPILE FAIL %s\n%s" % (name, res.stderr.strip()))
                continue
            got = recover.parse_message(out.read_bytes(), "FileDescriptorSet")["file"][0]
            problems = diff(normalise(fd, file_map), normalise(got, file_map))
            if problems:
                failures += 1
                print("MISMATCH %s" % name)
                for p in problems[:20]:
                    print("   " + p)
    print("%d files checked, %d failing" % (len(orig), failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())

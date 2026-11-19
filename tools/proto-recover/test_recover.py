#!/usr/bin/env python3
"""Tests for recover.py.

    python3 -I tools/proto-recover/test_recover.py
Set PROTOC (e.g. "python -m grpc_tools.protoc") to also run the protoc
round-trip tests; the real-source tests skip when the decompiled tree is absent.
"""

import importlib.util
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name):
    spec = importlib.util.spec_from_file_location(name, HERE / (name + ".py"))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


recover = _load("recover")
verify = _load("verify")

PROTOC = os.environ.get("PROTOC")
HAVE_SRC = recover.DEFAULT_SRC.is_dir()


class JavaLiteralTest(unittest.TestCase):
    def test_simple_and_unicode_escapes(self):
        self.assertEqual(recover.decode_java_literal(r"\n\u0012ab\"\\"), "\n\x12ab\"\\")

    def test_latin1_chars_pass_through(self):
        self.assertEqual(recover.decode_java_literal("ß\\b"), "ß\b")

    def test_octal_escape(self):
        self.assertEqual(recover.decode_java_literal(r"\0\177"), "\x00\x7f")

    def test_descriptor_array_with_two_elements(self):
        src = 'x = Descriptors.FileDescriptor.internalBuildGeneratedFileFrom(new String[]{"\\n\\u0001", "ß"}, deps);'
        self.assertEqual(recover.extract_descriptor_bytes(src), b"\n\x01\xdf")


class WireTest(unittest.TestCase):
    def test_negative_enum_value_decodes_as_int32(self):
        # EnumValueDescriptorProto{name:"N", number:-1}
        buf = b"\x0a\x01N\x10" + b"\xff" * 9 + b"\x01"
        self.assertEqual(recover.parse_message(buf, "EnumValueDescriptorProto"),
                         {"name": "N", "number": -1})


class NamingTest(unittest.TestCase):
    def test_snake_file_names(self):
        cases = {"GDIConnectIQHTTP.proto": "gdi_connect_iq_http.proto",
                 "GDISmart.proto": "gdi_smart.proto",
                 "GDISaMD.proto": "gdi_samd.proto",
                 "GDISDCard.proto": "gdi_sd_card.proto",
                 "GDIHSAData.proto": "gdi_hsa_data.proto",
                 "wifiSetupGDI.proto": "wifi_setup_gdi.proto"}
        for src, want in cases.items():
            self.assertEqual(recover.snake_file_name(src), want, src)

    def test_java_camel_case_capitalises_after_digits(self):
        self.assertEqual(recover._camel("p2p_connected"), "P2PConnected")
        self.assertEqual(recover._camel("max_speed_1000m_s"), "MaxSpeed1000MS")


class JavaScanTest(unittest.TestCase):
    SRC = '''
public final class Outer {
    private static String d = "class Fake { int X_FIELD_NUMBER = 9; }";
    public static final class Msg extends GeneratedMessageV3 {
        public static final int FOO_BAR_FIELD_NUMBER = 3;
        public boolean hasFooBar() { return true; }
        public enum Kind implements ProtocolMessageEnum {
            A(1),
            B(B_VALUE);
            public static final int B_VALUE = 7;
        }
        public static final class Inner extends GeneratedMessageV3 {
            public static final int Z_FIELD_NUMBER = 5;
        }
    }
}
'''

    def test_constants_attach_to_nested_classes(self):
        classes = recover.scan_java(self.SRC)
        self.assertEqual(classes["Msg"]["consts"], {"FOO_BAR": 3})
        self.assertEqual(classes["Msg.Inner"]["consts"], {"Z": 5})
        self.assertIn("hasFooBar", classes["Msg"]["has"])

    def test_braces_inside_strings_are_ignored(self):
        self.assertNotIn("Fake", recover.scan_java(self.SRC))

    def test_enum_constants_both_forms(self):
        kind = recover.scan_java(self.SRC)["Msg.Kind"]
        self.assertEqual(kind["enum_values"], {"A": 1})
        self.assertEqual(kind["value_consts"], {"B": 7})


@unittest.skipUnless(HAVE_SRC, "decompiled sources not present")
class RealSourceTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp()
        cls.out = Path(cls.tmp) / "proto"
        cls.pb = Path(cls.tmp) / "orig.pb"
        cls.totals, cls.notes, cls.smart, _ = recover.recover(
            recover.DEFAULT_SRC, cls.out, cls.pb)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp)

    def test_every_field_confirmed_by_java_constants(self):
        self.assertEqual(self.totals["inferred"], 0, self.notes)

    def test_core_service_bound_at_13(self):
        self.assertIn((13, "core_service", ".GDI.Proto.Core.CoreService", "GDICoreExtension"),
                      self.smart)

    def test_duplicate_smart_number_is_reported(self):
        self.assertTrue(any("number 93" in n for n in self.notes))

    def test_header_present(self):
        text = (self.out / "gdi_core.proto").read_text()
        self.assertTrue(text.startswith("// Recovered from Garmin Connect 5.31 (11761) "
                                        "com.garmin.proto.generated.GDICore by tools/proto-recover."))

    @unittest.skipUnless(PROTOC, "PROTOC not set")
    def test_protoc_round_trip_matches_embedded_descriptors(self):
        self.assertEqual(verify.main(["--protoc", PROTOC, "--orig", str(self.pb),
                                      "--proto-dir", str(self.out)]), 0)

    @unittest.skipUnless(PROTOC, "PROTOC not set")
    def test_round_trip_detects_a_changed_field_number(self):
        bad = Path(self.tmp) / "bad"
        shutil.copytree(self.out, bad)
        p = bad / "gdi_core.proto"
        p.write_text(p.read_text().replace("sync_request = 1;", "sync_request = 999;"))
        res = subprocess.run(shlex.split(PROTOC) + ["--proto_path=" + str(bad),
                                                    "--descriptor_set_out=" + os.devnull,
                                                    "gdi_core.proto"])
        self.assertEqual(res.returncode, 0)  # still valid proto ...
        self.assertEqual(verify.main(["--protoc", PROTOC, "--orig", str(self.pb),
                                      "--proto-dir", str(bad)]), 1)  # ... but caught


if __name__ == "__main__":
    unittest.main()

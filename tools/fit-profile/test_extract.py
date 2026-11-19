"""Unit tests for extract.py using small synthetic Java snippets.

Run: python3 -I -m unittest discover -s tools/fit-profile   (or run this file).
"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract  # noqa: E402


class FakeResolver:
    table = {
        "TsExtractor.TS_STREAM_TYPE_SPLICE_INFO": 134,
        "TurfConstants.UNIT_DEGREES": "degrees",
        "NotificationConstants.KEY_DATA_MIME_TYPE": "type",
    }

    def resolve(self, sym):
        return self.table[sym]


SAMPLE = '''
public class h500 extends wls {
    static {
        wls wlsVar = new wls("record", 20);
        f90724e = wlsVar;
        wlsVar.m74138e(new l5h("timestamp", 253, TsExtractor.TS_STREAM_TYPE_SPLICE_INFO, 1.0d, 0.0d, "s", false, ppy.DATE_TIME));
        ppy ppyVar = ppy.UINT16;
        wlsVar.m74138e(new l5h("altitude", 2, 132, 5.0d, 500.0d, "m", false, ppyVar));
        wlsVar.f204963c.get(1).f120137h.add(new r5h(78, false, 16, 5.0d, 500.0d));
        ppy ppyVar2 = ppy.UINT8;
        wlsVar.m74138e(new l5h("heart_rate", 3, 2, 1.0d, 0.0d, "bpm", false, ppyVar2));
        wlsVar.m74138e(new l5h("heading", 4, 132, 100.0d, 0.0d, TurfConstants.UNIT_DEGREES, false, ppyVar));
        wlsVar.m74138e(new l5h(NotificationConstants.KEY_DATA_MIME_TYPE, 5, 0, 1.0d, 0.0d, "", false, ppy.FILE));
        wlsVar.f204963c.get(4).f120138i.add(new fr50("sub, one", TsExtractor.TS_STREAM_TYPE_SPLICE_INFO, 1.0d, 0.0d, "reps"));
        wlsVar.f204963c.get(4).f120138i.get(0).m42679b(6, 20L);
        wlsVar.f204963c.get(4).f120138i.get(0).m42679b(5, 62L);
        wlsVar.f204963c.get(4).f120138i.get(0).m42678a(new r5h(9, true, 8, 1.0d, 0.0d));
    }
    public h500(wls wlsVar) { super(wlsVar); }
}
'''


class ParseTests(unittest.TestCase):
    def setUp(self):
        self.msgs = extract.parse_message_source(SAMPLE, FakeResolver())

    def test_message_header_and_count(self):
        self.assertEqual(len(self.msgs), 1)  # copy constructor `new wls(wlsVar)` ignored
        self.assertEqual((self.msgs[0]["name"], self.msgs[0]["number"]), ("record", 20))

    def test_field_attributes(self):
        f = self.msgs[0]["fields"][2]
        self.assertEqual(f["name"], "heart_rate")
        self.assertEqual((f["num"], f["base_type"], f["type_name"]), (3, "uint8", "uint8"))
        self.assertEqual((f["scale"], f["offset"], f["units"]), (1.0, 0.0, "bpm"))

    def test_symbolic_base_type_resolved(self):
        f = self.msgs[0]["fields"][0]
        self.assertEqual(f["base_type"], "uint32")
        self.assertEqual(f["type_name"], "date_time")

    def test_ppy_alias_tracks_reassignment(self):
        fields = self.msgs[0]["fields"]
        self.assertEqual(fields[1]["type_name"], "uint16")  # ppyVar = UINT16
        self.assertEqual(fields[2]["type_name"], "uint8")   # ppyVar2
        self.assertEqual(fields[3]["type_name"], "uint16")  # ppyVar still UINT16

    def test_symbolic_units_and_name_resolved(self):
        fields = self.msgs[0]["fields"]
        self.assertEqual(fields[3]["units"], "degrees")
        self.assertEqual(fields[4]["name"], "type")
        self.assertEqual(fields[4]["type_name"], "file")

    def test_components_attach_by_field_index(self):
        alt = self.msgs[0]["fields"][1]
        self.assertEqual(alt["components"], [
            {"dest": 78, "accumulate": False, "bits": 16, "scale": 5.0, "offset": 500.0}])

    def test_subfields_refs_and_components(self):
        sub = self.msgs[0]["fields"][4]["subfields"][0]
        self.assertEqual(sub["name"], "sub, one")  # comma inside a string literal
        self.assertEqual(sub["base_type"], "uint32")
        self.assertEqual(sub["units"], "reps")
        self.assertEqual(sub["refs"], [(6, 20), (5, 62)])
        self.assertEqual(sub["components"][0]["dest"], 9)
        self.assertTrue(sub["components"][0]["accumulate"])

    def test_unknown_base_type_fails_fast(self):
        bad = 'new wls("x", 1);\nwlsVar.m74138e(new l5h("a", 0, 99, 1.0d, 0.0d, "", false, ppy.UINT8));'
        with self.assertRaises(extract.ExtractError):
            extract.parse_message_source(bad, FakeResolver())


class LiteralTests(unittest.TestCase):
    def test_split_args_respects_strings_and_parens(self):
        self.assertEqual(extract.split_args('"a,b", f(1, 2), 3'), ['"a,b"', "f(1, 2)", "3"])

    def test_parse_literal(self):
        self.assertEqual(extract.parse_literal("0x10", None), 16)
        self.assertEqual(extract.parse_literal("20L", None), 20)
        self.assertEqual(extract.parse_literal("0.7111111d", None), 0.7111111)
        self.assertEqual(extract.parse_literal('"a\\"b"', None), 'a"b')

    def test_symbol_without_resolver_fails(self):
        with self.assertRaises(extract.ExtractError):
            extract.parse_literal("Foo.BAR", None)


class ResolverTests(unittest.TestCase):
    def test_resolves_constants_from_decompiled_classes(self):
        with tempfile.TemporaryDirectory() as d:
            os.makedirs(os.path.join(d, "com", "x"))
            with open(os.path.join(d, "com", "x", "TurfConstants.java"), "w") as fh:
                fh.write('class TurfConstants {\n  public static final String UNIT_DEGREES = "degrees";\n'
                         "  public static final int N = 0x86;\n}\n")
            r = extract.Resolver(d)
            self.assertEqual(r.resolve("TurfConstants.UNIT_DEGREES"), "degrees")
            self.assertEqual(r.resolve("TurfConstants.N"), 134)
            with self.assertRaises(extract.ExtractError):
                r.resolve("TurfConstants.MISSING")


FALLBACK_ENUM = '''
package defpackage;

public enum j6h extends java.lang.Enum<defpackage.j6h> {
    public final short a;

    static {
            j6h r1 = new j6h
            java.lang.String r0 = "DEVICE"
            r2 = 0
            r3 = 1
            r1.<init>(r0, r2, r3)
            j6h r2 = new j6h
            java.lang.String r0 = "SETTINGS"
            r4 = 2
            r2.<init>(r0, r3, r4)
            defpackage.j6h.b = r2
            j6h r5 = new j6h
            java.lang.String r0 = "ACTIVITY"
            r6 = r4
            r7 = 4
            r5.<init>(r0, r6, r7)
    }
}
'''


class ResolverImportTests(unittest.TestCase):
    def _make(self, d):
        for pkg, val in (("a/x", 1), ("b/y", 155)):
            os.makedirs(os.path.join(d, pkg))
            with open(os.path.join(d, pkg, "Desc.java"), "w") as fh:
                fh.write("class Desc { public static final int V = %d; }\n" % val)

    def test_import_selects_the_right_class(self):
        with tempfile.TemporaryDirectory() as d:
            self._make(d)
            r = extract.Resolver(d)
            r.set_context("import b.y.Desc;\nclass X {}")
            self.assertEqual(r.resolve("Desc.V"), 155)

    def test_ambiguous_without_import_fails(self):
        with tempfile.TemporaryDirectory() as d:
            self._make(d)
            r = extract.Resolver(d)
            r.set_context("class X {}")
            with self.assertRaises(extract.ExtractError):
                r.resolve("Desc.V")


class EnumTests(unittest.TestCase):
    def test_fallback_enum_names_and_values(self):
        cls, consts = extract.parse_fallback_enum(FALLBACK_ENUM)
        self.assertEqual(cls, "j6h")
        self.assertEqual(consts, [("DEVICE", 1), ("SETTINGS", 2), ("ACTIVITY", 4)])

    def test_attribution_requires_all_anchors(self):
        classes = {
            "a": [("ACTIVITY", 4), ("MONITORING_B", 32), ("SETTINGS", 2), ("SLEEP_DATA", 49)],
            "b": [("ACTIVITY", 4), ("SETTINGS", 2)],
        }
        enums, unattributed, ambiguous = extract.attribute_enums(classes)
        self.assertEqual(list(enums), ["file"])
        self.assertEqual(unattributed, ["b"])
        self.assertEqual(ambiguous, [])

    def test_mesg_num_chain(self):
        src = ('ckd.m9563k(0, map, "FILE_ID", 1, "CAPABILITIES");\n'
               'ckd.m9563k(TsExtractor.TS_STREAM_TYPE_SPLICE_INFO, map, "CSV_INFO", 135, "X");')
        got = extract.parse_mesg_num_source(src, FakeResolver())
        self.assertEqual(got, {0: "file_id", 1: "capabilities", 134: "csv_info", 135: "x"})


class RenderTests(unittest.TestCase):
    def test_render_is_deterministic_and_sorted(self):
        m = extract.parse_message_source(SAMPLE, FakeResolver())[0]
        msgs = {20: m, 5: {"name": "sport", "number": 5, "fields": []}}
        a, stats = extract.render_rust(msgs, {"file": [("ACTIVITY", 4)]}, {20: "record"})
        b, _ = extract.render_rust(msgs, {"file": [("ACTIVITY", 4)]}, {20: "record"})
        self.assertEqual(a, b)
        self.assertLess(a.index("global: 5,"), a.index("global: 20,"))
        self.assertIn('fe(3,"heart_rate",BaseType::Uint8,"uint8",None,None,Some("bpm"),false,&[],&[])', a)
        self.assertIn('("file", &[(4, "activity")])', a)
        self.assertIn("@generated", a)
        self.assertEqual(stats[0], 2)


if __name__ == "__main__":
    unittest.main()

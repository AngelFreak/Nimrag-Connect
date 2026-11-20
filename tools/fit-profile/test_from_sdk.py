"""Unit tests for from_sdk.py on a small synthetic SDK profile.

Run: python3 -I -m unittest discover -s tools/fit-profile
"""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import from_sdk  # noqa: E402

PROFILE = '''
import os  # never executed: the file is parsed, not imported
Profile = {
    'version': {'major': 21, 'minor': 222, 'patch': 0, 'type': "Release"},
    'messages': {
        20: {'num': 20, 'name': "record", 'messages_key': "record_mesgs", 'fields': {
            2: {'num': 2, 'name': "altitude", 'type': "uint16", 'base_type': "uint16", 'array': "false",
                'scale': [5], 'offset': [500], 'units': ["m"], 'bits': [16], 'components': [78],
                'is_accumulated': False, 'has_components': True, 'sub_fields': []},
            5: {'num': 5, 'name': "distance", 'type': "uint32", 'base_type': "uint32", 'array': "false",
                'scale': [100], 'offset': [0], 'units': "m", 'bits': [], 'components': [],
                'is_accumulated': True, 'has_components': False, 'sub_fields': []},
            8: {'num': 8, 'name': "compressed_speed_distance", 'type': "byte", 'base_type': "byte",
                'array': "true", 'scale': [100, 16], 'offset': [0, 0], 'units': ["m/s", "m"],
                'bits': [12, 12], 'components': [6, 5], 'is_accumulated': False, 'has_components': True,
                'sub_fields': []},
            78: {'num': 78, 'name': "enhanced_altitude", 'type': "uint32", 'base_type': "uint32",
                 'array': "false", 'scale': [5], 'offset': [500], 'units': "m", 'bits': [],
                 'components': [], 'is_accumulated': False, 'has_components': False, 'sub_fields': []},
        }},
        21: {'num': 21, 'name': "event", 'messages_key': "event_mesgs", 'fields': {
            3: {'num': 3, 'name': "data", 'type': "uint32", 'base_type': "uint32", 'array': "false",
                'scale': [1], 'offset': [0], 'units': "", 'bits': [], 'components': [],
                'is_accumulated': False, 'has_components': False, 'sub_fields': [
                    {'name': "timer_trigger", 'type': "timer_trigger", 'array': "", 'scale': [1],
                     'offset': [0], 'units': [""], 'bits': [], 'components': [], 'has_components': False,
                     'map': [{'num': 0, 'name': "event", 'raw_value': 0, 'value_name': "timer"}]}]},
        }},
    },
    'types': {'file': {4: 'activity', 32: 'monitoring_b'}, 'timer_trigger': {0: 'manual'}},
}
'''


class FromSdkTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.path = os.path.join(self.dir.name, "profile.py")
        with open(self.path, "w") as fh:
            fh.write(PROFILE)
        self.messages, self.enums, self.version = from_sdk.convert(from_sdk.load_profile(self.path))

    def tearDown(self):
        self.dir.cleanup()

    def field(self, mesg, num):
        return next(f for f in self.messages[mesg]["fields"] if f["num"] == num)

    def test_scale_applies_only_with_one_value(self):
        alt = self.field(20, 2)
        self.assertEqual((alt["scale"], alt["offset"], alt["units"]), (5, 500, "m"))
        csd = self.field(20, 8)
        self.assertEqual((csd["scale"], csd["offset"], csd["units"]), (1, 0, ""))
        self.assertTrue(csd["array"])

    def test_components_take_accumulate_from_their_target(self):
        csd = self.field(20, 8)["components"]
        self.assertEqual([(c["dest"], c["bits"], c["scale"], c["units"]) for c in csd],
                         [(6, 12, 100, "m/s"), (5, 12, 16, "m")])
        self.assertEqual([c["accumulate"] for c in csd], [False, True])

    def test_subfields_keep_type_name_and_parent_base(self):
        sub = self.field(21, 3)["subfields"][0]
        self.assertEqual((sub["type_name"], sub["base_type"], sub["refs"]), ("timer_trigger", "uint32", [(0, 0)]))

    def test_enums_and_version(self):
        self.assertEqual(self.enums["file"], {4: "activity", 32: "monitoring_b"})
        self.assertEqual(self.version, 21222)

    def test_overlay_fills_gaps_only(self):
        added = from_sdk.apply_overlay(self.messages, {"messages": {
            "20": {"fields": [[2, "replaced", "uint8", "uint8", 1, 0, ""], [99, "extra", "uint8", "uint8", 1, 0, ""]]},
            "411": {"name": "sleep_summary", "fields": [[0, "overall_sleep_score", "uint8", "uint8", 1, 0, ""]]},
        }})
        self.assertEqual(added, 2)
        self.assertEqual(self.field(20, 2)["name"], "altitude")
        self.assertEqual(self.field(20, 99)["name"], "extra")
        self.assertEqual(self.messages[411]["name"], "sleep_summary")
        with self.assertRaises(ValueError):
            from_sdk.apply_overlay(self.messages, {"messages": {"1": {"fields": [[0, "x", "nope", "x", 1, 0, ""]]}}})

    def test_render_is_rust_shaped(self):
        text = from_sdk.render_rust(self.messages, self.enums, self.version, "test")
        self.assertIn('arr(fe(8,"compressed_speed_distance"', text)
        self.assertIn("pub(super) static VERSION: Option<u32> = Some(21222);", text)
        self.assertIn('("file", &[(4, "activity"), (32, "monitoring_b")])', text)


if __name__ == "__main__":
    unittest.main()

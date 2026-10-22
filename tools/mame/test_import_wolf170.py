"""Synthetic format tests; no ROMs or downloaded recording data are needed."""

import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch
import zlib

import import_wolf170 as importer


def recording(ports=None, *, dips=(0xA1, 0x24), machine=b"digdug", emulator=b"MAME 0.170 Jan 27 2016",
              timestamps=None, defaults=None, body_suffix=b""):
    ports = [(0, 0, 0)] if ports is None else ports
    header = (b"MAMEINP\0" + struct.pack("<Q", 1) + b"\x03\x05\0\0"
              + machine.ljust(12, b"\0") + emulator.ljust(24, b"\0") + bytes(8))
    frames = []
    for index, controls in enumerate(ports):
        timestamp = index * importer.FRAME_ATTOSECONDS if timestamps is None else timestamps[index]
        seconds, attoseconds = divmod(timestamp, 10**18)
        frame_defaults = [15, 15, 15, 15, dips[0], 0, dips[1], 0] if defaults is None else defaults[index]
        digital = list(controls) + [0] * (8 - len(controls))
        frame = struct.pack("<iqI", seconds, attoseconds, 100 << 20)
        frame += b"".join(struct.pack("<II", default, bits) for default, bits in zip(frame_defaults, digital))
        frames.append(frame)
    return header + zlib.compress(b"".join(frames) + body_suffix)


class ImportTests(unittest.TestCase):
    def test_button_mapping_rle_and_first_frame_numbers(self):
        raw = recording([(0, 0, 0), (0, 1, 0), (0, 1, 0), (4, 0, 0),
                         (8, 0, 0), (1, 0, 5), (0x0D, 1, 15)])
        result = importer.decode(raw, "synthetic.inp")
        rows = [line for line in result.script.splitlines() if not line.startswith("#")]
        self.assertEqual(rows, ["00 1", "40 2", "20 1", "80 1", "15 1", "FF 1"])
        self.assertEqual(result.metadata["first_input_frames"],
                         {"coin": 2, "start1": 4, "start2": 5, "movement": 6, "fire": 6})
        self.assertEqual(result.metadata["input_records"], 7)
        self.assertEqual(result.metadata["rle_rows"], 6)
        self.assertEqual(result.metadata["script_start"], 1)

    def test_manifest_retains_source_hash_and_physical_dips(self):
        raw = recording(dips=(0x99, 0xDB))
        result = importer.decode(raw, "owned.inp")
        self.assertEqual(result.metadata["sha256"], hashlib.sha256(raw).hexdigest())
        self.assertEqual(result.metadata["source"], "owned.inp")
        self.assertEqual(result.metadata["source_bytes"], len(raw))
        self.assertEqual(result.metadata["physical_dip_a"], "99")
        self.assertEqual(result.metadata["physical_dip_b"], "DB")
        self.assertEqual(result.metadata["port_defaults"], [15, 15, 15, 15, 0x99, 0, 0xDB, 0])
        self.assertIn("no game-state or final-score validation", result.metadata["validation_scope"])

    def test_unknown_header_and_driver_are_rejected(self):
        good = recording()
        invalid = [b"", good[:63], b"BROKEN!\0" + good[8:],
                   good[:16] + b"\x03\x06\0\0" + good[20:],
                   good[:18] + b"\x01\0" + good[20:],
                   recording(machine=b"digdug1"), recording(emulator=b"MAME 0.183 Jan 01 2017"),
                   recording(emulator=b"MAME 0.1700 Jan 1 2016")]
        for raw in invalid:
            with self.subTest(header=raw[:64]), self.assertRaises(importer.InvalidRecording):
                importer.decode(raw, "bad.inp")

    def test_truncated_trailing_empty_and_partial_streams_are_rejected(self):
        good = recording()
        for raw in (good[:-1], good + b"trailing", good + zlib.compress(b"extra"),
                    recording(ports=[]), recording(body_suffix=b"\0"), good[:64] + b"not zlib"):
            with self.subTest(size=len(raw)), self.assertRaises(importer.InvalidRecording):
                importer.decode(raw, "bad.inp")

    def test_every_timestamp_must_follow_the_known_clock(self):
        delta = importer.FRAME_ATTOSECONDS
        for times in ([1], [0, delta + 1], [0, 0], [0, delta, delta * 3]):
            with self.subTest(times=times), self.assertRaises(importer.InvalidRecording):
                importer.decode(recording(ports=[(0, 0, 0)] * len(times), timestamps=times), "bad.inp")

    def test_changed_defaults_unknown_ports_and_nonbyte_dips_are_rejected(self):
        default = [15, 15, 15, 15, 0xA1, 0, 0x24, 0]
        for port, value in ((0, 14), (3, 0), (4, 256), (5, 1), (6, 256), (7, 1)):
            changed = default.copy()
            changed[port] = value
            with self.subTest(port=port), self.assertRaises(importer.InvalidRecording):
                importer.decode(recording(defaults=[changed]), "bad.inp")
        changed = default.copy()
        changed[4] ^= 1
        with self.assertRaises(importer.InvalidRecording):
            importer.decode(recording(ports=[(0, 0, 0)] * 2, defaults=[default, changed]), "bad.inp")

    def test_unsupported_controls_and_dynamic_dips_are_rejected(self):
        for port, value in ((0, 2), (1, 2), (2, 16), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1)):
            controls = [0] * 8
            controls[port] = value
            with self.subTest(port=port), self.assertRaises(importer.InvalidRecording):
                importer.decode(recording(ports=[controls]), "bad.inp")

    def test_input_and_decompressed_limits_are_enforced(self):
        raw = recording(ports=[(0, 0, 0)] * 2)
        with patch.object(importer, "MAX_SOURCE_BYTES", len(raw) - 1), self.assertRaises(importer.InvalidRecording):
            importer.decode(raw, "bad.inp")
        with patch.object(importer, "MAX_BODY_BYTES", importer.RECORD_BYTES), self.assertRaises(importer.InvalidRecording):
            importer.decode(raw, "bad.inp")

    def test_file_outputs_are_new_and_invalid_sources_create_nothing(self):
        with tempfile.TemporaryDirectory() as directory:
            source, output, manifest = [Path(directory) / name for name in ("test.inp", "script.txt", "manifest.json")]
            source.write_bytes(recording())
            importer.import_file(source, output, manifest)
            original = output.read_bytes(), manifest.read_bytes()
            self.assertEqual(json.loads(manifest.read_text())["input_records"], 1)
            with self.assertRaises(FileExistsError):
                importer.import_file(source, output, manifest)
            self.assertEqual((output.read_bytes(), manifest.read_bytes()), original)
            output.unlink()
            with self.assertRaises(FileExistsError):
                importer.import_file(source, output, manifest)
            self.assertFalse(output.exists())
            manifest.unlink()
            source.write_bytes(b"broken")
            with self.assertRaises(importer.InvalidRecording):
                importer.import_file(source, output, manifest)
            self.assertFalse(output.exists())
            self.assertFalse(manifest.exists())

    def test_shared_path_and_failed_second_creation_leave_no_partial_output(self):
        with tempfile.TemporaryDirectory() as directory:
            source, output = Path(directory) / "test.inp", Path(directory) / "script.txt"
            source.write_bytes(recording())
            with self.assertRaises(ValueError):
                importer.import_file(source, output, output)
            self.assertFalse(output.exists())
            with self.assertRaises(OSError):
                importer.import_file(source, output, Path(directory) / "missing" / "manifest.json")
            self.assertFalse(output.exists())


if __name__ == "__main__":
    unittest.main()

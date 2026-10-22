"""Import the known WolfMAME 0.170 Dig Dug Rev 2 input-recording layout.

This deliberately accepts one INP format, not arbitrary MAME recordings. Layout
and port order come from the official wolf170 sources:
https://github.com/mahlemiut/wolfmame/blob/wolf170/src/emu/ioport.cpp
https://github.com/mahlemiut/wolfmame/blob/wolf170/src/mame/drivers/galaga.cpp

Extraction validates input data; it does not establish successful game playback
or agreement between the native translation and the reference emulator.
"""

import argparse
from contextlib import ExitStack
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import re
import struct
import sys
import zlib


HEADER_BYTES = 64
RECORD_BYTES = 80
MAX_FRAMES = 1_000_000
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MAX_BODY_BYTES = MAX_FRAMES * RECORD_BYTES
FRAME_ATTOSECONDS = 16_499_999_999_932_416
PORT_NAMES = ("IN0L", "IN0H", "IN1L", "IN1H", "DSWA", "DSWA_HI", "DSWB", "DSWB_HI")
EVENT_BITS = (("coin", 0x40), ("start1", 0x20), ("start2", 0x80),
              ("movement", 0x0F), ("fire", 0x10))


class InvalidRecording(ValueError):
    """The source is outside the supported recording format."""


@dataclass
class ImportedRecording:
    script: str
    metadata: dict


def _header_string(field, name):
    value, separator, padding = field.partition(b"\0")
    if not separator or any(padding):
        raise InvalidRecording(f"invalid {name} header field")
    try:
        return value.decode("ascii")
    except UnicodeDecodeError as error:
        raise InvalidRecording(f"non-ASCII {name} header field") from error


def decode(raw, source_name):
    """Validate a complete INP and return its RLE script plus manifest."""
    if len(raw) > MAX_SOURCE_BYTES:
        raise InvalidRecording("source exceeds the 64 MiB input limit")
    if len(raw) < HEADER_BYTES or raw[:8] != b"MAMEINP\0":
        raise InvalidRecording("missing 64-byte MAMEINP header")
    if raw[16:20] != b"\x03\x05\0\0":
        raise InvalidRecording("only INP 3.5 with the known reserved bytes is supported")
    machine = _header_string(raw[20:32], "machine")
    emulator = _header_string(raw[32:56], "emulator")
    if machine != "digdug":
        raise InvalidRecording("only the digdug Rev 2 driver is supported")
    if not re.fullmatch(r"MAME 0\.170 [ -~]+", emulator):
        raise InvalidRecording("only the MAME 0.170 recording layout is supported")

    stream = zlib.decompressobj()
    try:
        body = stream.decompress(raw[HEADER_BYTES:], MAX_BODY_BYTES + 1)
    except zlib.error as error:
        raise InvalidRecording(f"invalid compressed input stream: {error}") from error
    if len(body) > MAX_BODY_BYTES or stream.unconsumed_tail:
        raise InvalidRecording("decompressed input exceeds the 1,000,000-frame limit")
    if not stream.eof:
        raise InvalidRecording("truncated compressed input stream")
    if stream.unused_data:
        raise InvalidRecording("unexpected data after the compressed input stream")
    if not body or len(body) % RECORD_BYTES:
        raise InvalidRecording("input must contain complete 80-byte frame records")

    defaults = None
    runs = []
    first_events = {}
    last_time = 0
    for index in range(len(body) // RECORD_BYTES):
        offset = index * RECORD_BYTES
        seconds, attoseconds, _speed = struct.unpack_from("<iqI", body, offset)
        timestamp = seconds * 10**18 + attoseconds
        if seconds < 0 or not 0 <= attoseconds < 10**18 or timestamp != index * FRAME_ATTOSECONDS:
            raise InvalidRecording(f"frame {index + 1}: expected the fixed Wolf170 frame timestamp")
        last_time = timestamp
        pairs = [struct.unpack_from("<II", body, offset + 16 + port * 8) for port in range(8)]
        frame_defaults = [pair[0] for pair in pairs]
        digital = [pair[1] for pair in pairs]
        if defaults is None:
            defaults = frame_defaults
            if (defaults[:4] != [15] * 4 or defaults[5] != 0 or defaults[7] != 0
                    or defaults[4] > 255 or defaults[6] > 255):
                raise InvalidRecording("unsupported eight-port defaults or DIP bank layout")
        if frame_defaults != defaults:
            raise InvalidRecording(f"frame {index + 1}: port defaults or physical DIP settings changed")
        if (digital[0] & ~0x0D or digital[1] & ~1 or digital[2] & ~0x0F
                or any(digital[port] for port in (3, 4, 5, 6, 7))):
            raise InvalidRecording(f"frame {index + 1}: unsupported controls or dynamic DIP data")

        buttons = digital[2]
        if digital[0] & 1:
            buttons |= 0x10
        if digital[0] & 4:
            buttons |= 0x20
        if digital[0] & 8:
            buttons |= 0x80
        if digital[1] & 1:
            buttons |= 0x40
        if runs and runs[-1][0] == buttons:
            runs[-1][1] += 1
        else:
            runs.append([buttons, 1])
        for label, bit in EVENT_BITS:
            if buttons & bit:
                first_events.setdefault(label, index + 1)

    dip_a, dip_b = defaults[4], defaults[6]
    checksum = hashlib.sha256(raw).hexdigest()
    script = ("# Imported WolfMAME 0.170 digdug recording; script-start 1.\n"
              f"# Physical DIP banks {dip_a:02X}/{dip_b:02X}; use these values with the native replay runner.\n"
              f"# Source SHA256 {checksum}\n"
              "# Input extraction does not prove successful or synchronized gameplay.\n"
              + "".join(f"{buttons:02X} {frames}\n" for buttons, frames in runs))
    metadata = {
        "source": str(source_name), "sha256": checksum, "source_bytes": len(raw),
        "header_format": "MAMEINP 3.5", "emulator": emulator, "machine": machine,
        "header_bytes": HEADER_BYTES, "record_bytes": RECORD_BYTES,
        "decompressed_bytes": len(body), "input_records": len(body) // RECORD_BYTES,
        "last_input_timestamp_attoseconds": last_time,
        "last_input_timestamp_seconds": last_time / 10**18,
        "frame_timestamp_delta_attoseconds": FRAME_ATTOSECONDS,
        "rle_rows": len(runs), "script_start": 1,
        "port_order": list(PORT_NAMES), "port_defaults": defaults,
        "physical_dip_a": f"{dip_a:02X}", "physical_dip_b": f"{dip_b:02X}",
        "first_input_frames": first_events, "unsupported_controls_present": False,
        "validation_scope": "Header, complete zlib stream, all frame timestamps, fixed port layout/defaults, "
                            "stable physical DIPs and supported controls; no game-state or final-score validation.",
    }
    return ImportedRecording(script, metadata)


def import_file(source, output, metadata):
    """Validate before creating either output; never replace an existing file."""
    source, output, metadata = Path(source), Path(output), Path(metadata)
    with source.open("rb") as inp:
        raw = inp.read(MAX_SOURCE_BYTES + 1)
    imported = decode(raw, source.name)
    if output.resolve() == metadata.resolve():
        raise ValueError("script and metadata must use different output paths")
    for path in (output, metadata):
        if path.exists() or path.is_symlink():
            raise FileExistsError(f"output already exists: {path}")

    created = []
    try:
        with ExitStack() as stack:
            script_file = stack.enter_context(output.open("x", encoding="ascii", newline="\n"))
            created.append(output)
            metadata_file = stack.enter_context(metadata.open("x", encoding="utf-8", newline="\n"))
            created.append(metadata)
            script_file.write(imported.script)
            metadata_file.write(json.dumps(imported.metadata, indent=2) + "\n")
    except BaseException:
        for path in reversed(created):
            path.unlink(missing_ok=True)
        raise
    return imported.metadata


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("source", type=Path, help="WolfMAME 0.170 digdug INP file")
    parser.add_argument("--output", type=Path, required=True, help="new native RLE script")
    parser.add_argument("--metadata", type=Path, required=True, help="new JSON manifest")
    args = parser.parse_args(argv)
    try:
        manifest = import_file(args.source, args.output, args.metadata)
    except (OSError, ValueError) as error:
        print(f"{parser.prog}: {error}", file=sys.stderr)
        return 64
    print(f"Imported {manifest['input_records']} frames in {manifest['rle_rows']} input runs; "
          f"physical DIP={manifest['physical_dip_a']}/{manifest['physical_dip_b']}; script-start 1.")
    print("Input data validated; successful reference playback and native gameplay agreement require separate checks.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

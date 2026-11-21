#!/usr/bin/env python3
"""Oculibre device report: what the phone's cameras expose to apps, without personal data.

Plug the phone in (USB debugging on), then from the repository root:

    python3 scripts/device-report.py                      # name taken from the phone
    python3 scripts/device-report.py --name "Galaxy S23 Ultra"
    ANDROID_SERIAL=<serial> python3 scripts/device-report.py   # several phones plugged

It writes devices/<brand>/<model>/camera-android<ver>-<build>.txt (sanitised dump) and,
the first time, devices/<brand>/<model>/report.md (summary + results to fill in).

What is KEPT: static camera characteristics (sizes, frame rates, capabilities, vendor tag
names). What is REMOVED: the camera usage log (which app opened the camera, when, PIDs),
dynamic/session info, HAL state dumps, and per-unit factory calibration data that could
identify this particular phone. The serial number is never read.

Standard library only (Python 3.8+).
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
from datetime import date
from pathlib import Path

TOOL_VERSION = "1"

# Sections of `dumpsys media.camera` worth keeping (everything else is dropped).
KEEP_SECTION = re.compile(r"static info|static information|Concurrent ID combinations|Vendor tags")
# From "Service global info", only these lines are kept.
KEEP_GLOBAL_LINE = re.compile(r"^(Number of .*camera devices.*|\s+Device \d+ maps to .*)$")

# Metadata entries whose values are per-unit factory data (fingerprinting risk): redacted.
# Standard keys, matched on the last component of the name (e.g. android.lens.distortion).
SENSITIVE_LAST = re.compile(
    r"^(distortion\w*|intrinsicCalibration\w*|poseRotation|poseTranslation|"
    r"colorTransform\d|forwardMatrix\d|calibrationTransform\d)$"
)
# Vendor keys: calibration blobs, EEPROM/OTP dumps, serials, module IDs. Word-ish boundaries
# so that e.g. "hotPixel" (h-otp-ixel) or "focusDistanceCalibration" (an enum) are not hit.
SENSITIVE_ANY = re.compile(
    r"(?<![a-z])(otp|eeprom|serial|uniqueid|unique_id|moduleid|module_id|moduleinfo|module_info|sensorid)"
    r"|dualcal|calibrationdata|\.calibration$",
    re.IGNORECASE,
)


def is_sensitive(key: str) -> bool:
    return bool(SENSITIVE_LAST.match(key.rsplit(".", 1)[-1]) or SENSITIVE_ANY.search(key))
# Opaque vendor blobs larger than this are redacted too.
MAX_BLOB_BYTES = 64

ENTRY = re.compile(r"^(?P<indent>\s+)(?P<key>[\w.]+) \((?P<tag>[0-9a-fA-F]+)\): (?P<type>\w+)\[(?P<count>\d+)\]\s*$")

# Last-resort safety net: lines that must never survive.
FORBIDDEN_LINE = re.compile(
    r"\b\d{2}-\d{2} \d{2}:\d{2}:\d{2}\b"  # event timestamps
    r"|\bPID\b|\bpid\b|client\.pid|package "
    r"|0x[0-9a-f]{8,}\)?\s*$"  # pointers at end of line
    , re.IGNORECASE,
)

PROPS = {
    "manufacturer": "ro.product.manufacturer",
    "brand": "ro.product.brand",
    "model": "ro.product.model",
    "device": "ro.product.device",
    "marketname": "ro.product.marketname",
    "vendor_marketname": "ro.product.vendor.marketname",
    "release": "ro.build.version.release",
    "sdk": "ro.build.version.sdk",
    "build_id": "ro.build.id",
    "incremental": "ro.build.version.incremental",
    "soc_manufacturer": "ro.soc.manufacturer",
    "soc_model": "ro.soc.model",
    "platform": "ro.board.platform",
}


# --------------------------------------------------------------------------- adb


def find_adb() -> str:
    candidates = [os.environ.get("ADB"), shutil.which("adb")]
    for var in ("ANDROID_HOME", "ANDROID_SDK_ROOT"):
        if os.environ.get(var):
            candidates.append(str(Path(os.environ[var]) / "platform-tools" / "adb"))
    candidates.append(str(Path.home() / "Android" / "Sdk" / "platform-tools" / "adb"))
    for c in candidates:
        if c and Path(c).is_file():
            return c
    sys.exit("adb not found. Run scripts/setup-dev.sh or set ADB=/path/to/adb")


def adb(adb_path: str, *args: str) -> str:
    try:
        out = subprocess.run([adb_path, *args], capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        sys.exit(f"adb {' '.join(args)} timed out")
    if out.returncode != 0:
        sys.exit(f"adb {' '.join(args)} failed: {out.stderr.strip() or out.stdout.strip()}")
    return out.stdout


def read_props(adb_path: str) -> dict[str, str]:
    raw = adb(adb_path, "shell", "getprop")
    allprops = dict(re.findall(r"^\[([^\]]+)\]: \[([^\]]*)\]", raw, re.MULTILINE))
    return {name: allprops.get(prop, "").strip() for name, prop in PROPS.items()}


# --------------------------------------------------------------------------- sanitising


def sanitize(dump: str) -> tuple[str, int]:
    """Returns (sanitised text, number of redacted entries)."""
    out: list[str] = []
    redacted = 0
    section_keep = False
    in_global = False
    skip_values_indent: int | None = None

    for line in dump.splitlines():
        if line.startswith("== "):
            in_global = "Service global info" in line
            section_keep = bool(KEEP_SECTION.search(line))
            skip_values_indent = None
            if section_keep or in_global:
                out.append(line)
            continue

        if in_global:
            if KEEP_GLOBAL_LINE.match(line):
                out.append(line)
            continue
        if not section_keep:
            continue

        # Skip the value lines of a redacted entry (deeper indent, starting with '[').
        if skip_values_indent is not None:
            stripped = line.lstrip()
            indent = len(line) - len(stripped)
            if indent > skip_values_indent and stripped.startswith("["):
                continue
            skip_values_indent = None

        m = ENTRY.match(line)
        if m:
            key, typ, count = m["key"], m["type"], int(m["count"])
            if is_sensitive(key) or (typ == "byte" and count > MAX_BLOB_BYTES):
                out.append(line)
                out.append(f"{m['indent']}  [redacted by device-report.py: per-unit data]")
                skip_values_indent = len(m["indent"])
                redacted += 1
                continue

        if FORBIDDEN_LINE.search(line):
            continue
        out.append(line)

    return "\n".join(out) + "\n", redacted


# --------------------------------------------------------------------------- summary


def parse_cameras(text: str) -> list[dict]:
    """Per camera: id, facing, hardware level, fps ranges, high-speed configs."""
    cameras: list[dict] = []
    current: dict | None = None
    key: str | None = None
    values: list[str] = []

    def flush():
        if current is not None and key is not None:
            current["entries"][key] = values[:]

    for line in text.splitlines():
        if line.startswith("== "):
            flush()
            key, values = None, []
            m = re.search(r"/([^/\s]+) \(v[\d.]+\) static information", line)
            if m:
                current = {"id": m.group(1), "entries": {}}
                cameras.append(current)
            else:
                current = None
            continue
        if current is None:
            continue
        m = ENTRY.match(line)
        if m:
            flush()
            key, values = m["key"], []
            continue
        if key is not None and line.lstrip().startswith("["):
            values.extend(re.findall(r"[^\s\[\]]+", line))
    flush()

    for cam in cameras:
        e = cam.pop("entries")
        ints = lambda k: [int(v) for v in e.get(k, []) if re.fullmatch(r"-?\d+", v)]  # noqa: E731
        fps = ints("android.control.aeAvailableTargetFpsRanges")
        cam["fps_ranges"] = [(fps[i], fps[i + 1]) for i in range(0, len(fps) - 1, 2)]
        hs = ints("android.control.availableHighSpeedVideoConfigurations")
        cam["high_speed"] = [tuple(hs[i:i + 5]) for i in range(0, len(hs) - 4, 5)]
        cam["facing"] = {"0": "front", "1": "back", "2": "external"}.get(
            (e.get("android.lens.facing") or ["?"])[0], (e.get("android.lens.facing") or ["?"])[0]
        )
        cam["level"] = {"0": "LIMITED", "1": "FULL", "2": "LEGACY", "3": "LEVEL_3", "4": "EXTERNAL"}.get(
            (e.get("android.info.supportedHardwareLevel") or ["?"])[0],
            (e.get("android.info.supportedHardwareLevel") or ["?"])[0],
        )
        caps = e.get("android.request.availableCapabilities", [])
        cam["high_speed_capable"] = "9" in caps or "CONSTRAINED_HIGH_SPEED_VIDEO" in caps
    return cameras


def public_ids(text: str) -> set[str]:
    return set(re.findall(r'Device \d+ maps to "([^"]+)"', text))


def summary_table(cameras: list[dict], public: set[str]) -> str:
    rows = [
        "| Camera ID | Facing | HW level | Max normal fps | Fixed-rate ranges | High-speed (fixed) |",
        "|---|---|---|---|---|---|",
    ]
    for c in cameras:
        max_fps = max((hi for _, hi in c["fps_ranges"]), default=0)
        fixed = sorted({lo for lo, hi in c["fps_ranges"] if lo == hi})
        hs_fixed = sorted(
            {(w, h, hi) for (w, h, lo, hi, _b) in c["high_speed"] if lo == hi},
            key=lambda t: (-t[0] * t[1], t[2]),
        )
        hs_text = ", ".join(f"{w}×{h}@{fps}" for w, h, fps in hs_fixed) or "—"
        cam_id = c["id"] + ("" if not public or c["id"] in public else " (hidden)")
        rows.append(
            f"| {cam_id} | {c['facing']} | {c['level']} | **{max_fps or '?'}** | "
            f"{', '.join(map(str, fixed)) or '—'} | {hs_text} |"
        )
    return "\n".join(rows)


# --------------------------------------------------------------------------- output


def slugify(s: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-") or "unknown"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--name", help='Marketing name, e.g. "Galaxy S23 Ultra" (default: read from the phone)')
    ap.add_argument("--from-file", type=Path, help="Use an existing `dumpsys media.camera` output instead")
    ap.add_argument("--out", type=Path, default=Path(__file__).resolve().parent.parent / "devices")
    ap.add_argument("--no-adb", action="store_true", help="Don't query the phone (needs --from-file, --name, --brand)")
    ap.add_argument("--brand", help="Brand folder (default: from the phone)")
    ap.add_argument("--android", help="Android version (default: from the phone)")
    ap.add_argument("--build", help="Build ID (default: from the phone)")
    args = ap.parse_args()

    props = {k: "" for k in PROPS}
    adb_path = None
    if not args.no_adb:
        adb_path = find_adb()
        props = read_props(adb_path)

    dump = args.from_file.read_text(errors="replace") if args.from_file else adb(adb_path, "shell", "dumpsys", "media.camera")
    if "static information" not in dump:
        sys.exit("This does not look like `dumpsys media.camera` output (no static camera information).")

    clean, redacted = sanitize(dump)
    leftovers = [l for l in clean.splitlines() if FORBIDDEN_LINE.search(l)]
    assert not leftovers, f"sanitiser bug, refusing to write: {leftovers[:3]}"

    brand = args.brand or props["brand"] or props["manufacturer"] or "unknown"
    name = args.name or props["marketname"] or props["vendor_marketname"] or props["model"] or "unknown"
    android = args.android or props["release"] or "unknown"
    build = args.build or props["build_id"] or "unknown"

    folder = args.out / slugify(brand) / slugify(name)
    folder.mkdir(parents=True, exist_ok=True)
    dump_file = folder / f"camera-android{slugify(android)}-{slugify(build)}.txt"

    header = [
        "# Oculibre device report — sanitised `adb shell dumpsys media.camera`",
        f"# Tool: scripts/device-report.py v{TOOL_VERSION} · generated {date.today().isoformat()}",
        f"# Device: {brand} {name} (model {props['model'] or '?'}, codename {props['device'] or '?'})",
        f"# Android {android} (API {props['sdk'] or '?'}), build {build} {props['incremental']}".rstrip(),
        f"# SoC: {props['soc_manufacturer']} {props['soc_model'] or props['platform']}".rstrip(),
        f"# Removed: camera usage log, dynamic/session info, HAL state; {redacted} per-unit entries redacted.",
        "",
    ]
    dump_file.write_text("\n".join(header) + clean)

    cameras = parse_cameras(clean)
    table = summary_table(cameras, public_ids(clean))

    report = folder / "report.md"
    report_existed = report.exists()
    if not report_existed:
        report.write_text(
            f"""# {brand} {name}

| | |
|---|---|
| Model | {props['model'] or '?'} (codename {props['device'] or '?'}) |
| Android | {android} (API {props['sdk'] or '?'}), build {build} |
| SoC | {(props['soc_manufacturer'] + ' ' + (props['soc_model'] or props['platform'])).strip() or '?'} |

## Camera capabilities

Generated from [`{dump_file.name}`]({dump_file.name}). "Max normal fps" is the highest frame
rate an app can request in a regular session; "high-speed" modes are slow-motion sessions
(darker, see [docs/research/60fps.typ](../../../docs/research/60fps.typ)).

{table}

## Oculibre results

<!-- Fill in after testing. -->

- Oculibre version:
- Modes offered by the app:
- 720p30 / 1080p30 / 4K30:
- 60 fps (normal or high-speed) and picture quality:
- Latency (glass-to-glass), USB / Wi-Fi:
- Temperature after 30 min:
- Notes:
"""
        )

    rel = lambda p: os.path.relpath(p, Path.cwd())  # noqa: E731
    print(f"✓ {rel(dump_file)}  ({len(clean) // 1024} KiB, {redacted} entries redacted)")
    print(f"✓ {rel(report)}" + ("  (kept: already exists)" if report_existed else "  (created: fill in the results)"))
    print()
    print(table)


if __name__ == "__main__":
    main()

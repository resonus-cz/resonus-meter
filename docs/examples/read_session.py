#!/usr/bin/env python3
"""Independent reader for splmeter-session bundles (see docs/FILE_FORMAT.md).

Usage: read_session.py <bundle folder>
Prints a JSON summary. Uses the Python standard library only.
"""
import csv, json, math, re, sys
from pathlib import Path


def front_matter(text):
    """Returns the top-level scalars and the `files:` mapping of the front matter."""
    meta, files, section = {}, {}, None
    for line in text.split("---\n")[1].splitlines():
        key, _, value = line.strip().partition(":")
        value = value.strip()
        if not line.startswith(" "):
            section = key
            if value:
                meta[key] = json.loads(value) if value.startswith('"') else value
        elif section == "files":
            files[key] = value
    return meta, files


def flow_list(text, key):
    """Returns the items of a one-line YAML flow sequence such as `key: [1, 2]`."""
    items = re.search(rf"^{key}: \[(.*)\]$", text, re.M).group(1)
    return [item.strip() for item in items.split(",")]


def read_csv(folder, name):
    with open(folder / name, newline="", encoding="utf-8") as file:
        return list(csv.DictReader(file))


def combined_leq(segments, column):
    """Energy mean of segment Leq weighted by duration; empty segments are skipped."""
    rows = [(float(s["duration_s"]), float(s[column])) for s in segments if s[column]]
    energy = sum(duration * 10 ** (leq / 10) for duration, leq in rows)
    return 10 * math.log10(energy / sum(duration for duration, _ in rows))


def main(folder):
    folder = Path(folder)
    text = (folder / "measurement.md").read_text(encoding="utf-8")
    meta, files = front_matter(text)
    if meta.get("format") != "splmeter-session":
        sys.exit("not an splmeter-session bundle")
    if int(str(meta["version"]).split(".")[0]) > 1:
        sys.exit(f"unsupported major version {meta['version']}")
    segments = read_csv(folder, files["segments"])
    summary = dict(re.findall(r"^([ACZ]): \{ Leq_db: (-?[\d.]+)", text, re.M))
    result = {
        "name": meta["name"],
        "version": str(meta["version"]),
        "log_rows": len(read_csv(folder, files["log"])),
        "markers": [m["kind"] for m in read_csv(folder, files["markers"])],
        "LAeq_combined": round(combined_leq(segments, "LAeq"), 2),
        "LAeq_summary": float(summary["A"]),
        "LAFmax": max(float(s["LAFmax"]) for s in segments if s["LAFmax"]),
    }
    if "rta_segments" in files:  # file format 1.2, band levels recorded
        bands_hz = flow_list(text, "  bands_hz")
        band_summary = dict(zip(bands_hz, flow_list(text, "rta_LZeq_db")))
        band_segments = read_csv(folder, files["rta_segments"])
        result["rta_bands_hz"] = bands_hz
        result["rta_log_rows"] = len(read_csv(folder, files["rta_log"]))
        result["LZeq_1000_combined"] = round(combined_leq(band_segments, "LZeq_1000"), 2)
        result["LZeq_1000_summary"] = float(band_summary["1000"])
    print(json.dumps(result))


if __name__ == "__main__":
    main(sys.argv[1])

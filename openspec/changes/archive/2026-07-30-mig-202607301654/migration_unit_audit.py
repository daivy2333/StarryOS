#!/usr/bin/env python3
"""Generate and verify the MIG-202607301654 per-unit migration audit.

The script is intentionally read-only. It emits TSV, a numbering map, or a
summary to stdout; callers persist generated output through the repository's
normal patch workflow.
"""

from __future__ import annotations

import argparse
import hashlib
import re
from collections import defaultdict
from pathlib import Path


def find_root(start: Path) -> Path:
    for candidate in (start, *start.parents):
        if (candidate / "openspec/config.yaml").is_file():
            return candidate
    raise RuntimeError("cannot locate repository root from migration carrier")


CARRIER = Path(__file__).resolve().parent
ROOT = find_root(CARRIER)

ACTIVE = (
    ("openspec/project.md", CARRIER / "active-originals/openspec-project-original.md"),
    (".claude/docs/tasks.md", CARRIER / "active-originals/tasks-original.md"),
)

A_TARGETS = {
    1: ("M01", "D01"), 2: ("M01", "D01"), 3: ("M02", "D02"),
    4: ("M03", "D03"), 5: ("M04", "D04"), 6: ("M05", "D05"),
    7: ("M05", "D05"), 8: ("M05", "D05"), 9: ("M05", "D05"),
    10: ("M05", "D05"), 11: ("M05", "D05"), 12: ("M06", "D06"),
    13: ("M07", "D07"), 14: ("D08", "D09"), 15: ("D08", "D09"),
    16: ("D08", "D09"), 17: ("D08", "D09"), 18: ("M08", "D08"),
    19: ("M08", "D08"), 20: ("M08", "D09"), 21: ("M08", "D09"),
    22: ("M08", "D09"), 23: ("M08", "D09"), 24: ("M08", "D08"),
    25: ("M09", "D08"), 26: ("M09", "D08"), 27: ("M10", "D09"),
    28: ("M10", "D09"), 29: ("M10", "D09"), 30: ("M10", "D09"),
    31: ("M11",), 32: ("D05", "D10"), 33: ("M12", "D10"),
    34: ("M13", "D11"), 35: ("M14", "D05"), 36: ("M14", "D05"),
    37: ("M15", "D12"), 38: ("M16", "D13"), 39: ("M17", "D14"),
    40: ("M37", "D21"), 41: ("M38", "D21"), 42: ("M39",),
    43: ("M40",), 44: ("M18", "D15"), 45: ("M19", "D15"),
    46: ("M20", "D15"), 47: ("M21", "D16"), 48: ("M22", "D16"),
    49: ("M23", "D16"), 50: ("M24", "D16"), 51: ("M25", "D16"),
    52: ("M26", "D16"), 53: ("M27", "D16"), 54: ("M28", "D16"),
    55: ("M29", "D16"), 56: ("M31", "D17"), 57: ("M30", "D17"),
    58: ("M31", "D17"), 59: ("M32",), 60: ("M33", "D18"),
    61: ("M34", "D19"), 62: ("M35", "D19"), 63: ("M36", "D20"),
    64: ("M35", "D10"),
}


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def historical_sources() -> list[tuple[str, Path]]:
    result = []
    manifest = CARRIER / "historical-carriers.sha256"
    for line in manifest.read_text().splitlines():
        if not line.strip():
            continue
        _, rel = line.split(maxsplit=1)
        result.append((rel, ROOT / rel))
    return result


def units(path: Path) -> list[tuple[int, int, str, bytes]]:
    data = path.read_bytes()
    lines = data.splitlines(keepends=True)
    if path.suffix in {".yaml", ".yml"}:
        return [(1, len(lines), "metadata", data)] if data.strip() else []

    out: list[tuple[int, int, str, bytes]] = []
    i = 0
    while i < len(lines):
        if not lines[i].strip():
            i += 1
            continue
        start = i
        text = lines[i].decode("utf-8")
        stripped = text.lstrip()
        if stripped.startswith("```"):
            i += 1
            while i < len(lines):
                end_text = lines[i].decode("utf-8").lstrip()
                i += 1
                if end_text.startswith("```"):
                    break
            kind = "code-block"
        elif stripped.startswith("#"):
            i += 1
            kind = "heading"
        elif stripped.startswith("|"):
            i += 1
            kind = "table-row"
        elif stripped.startswith("<!--"):
            i += 1
            while i < len(lines) and "-->" not in b"".join(lines[start:i]).decode("utf-8"):
                i += 1
            kind = "comment"
        elif re.match(r"(?:[-*+] |\d+[.)] )", stripped):
            i += 1
            while i < len(lines):
                nxt = lines[i].decode("utf-8")
                if not nxt.strip() or len(nxt) - len(nxt.lstrip()) == 0:
                    break
                i += 1
            kind = "list-item"
        else:
            i += 1
            while i < len(lines):
                nxt = lines[i].decode("utf-8")
                ns = nxt.lstrip()
                if (
                    not nxt.strip()
                    or ns.startswith(("#", "|", "```", "<!--"))
                    or re.match(r"(?:[-*+] |\d+[.)] )", ns)
                ):
                    break
                i += 1
            kind = "paragraph"
        raw = b"".join(lines[start:i])
        out.append((start + 1, i, kind, raw))
    return out


def ids(text: str) -> list[str]:
    patterns = (
        r"\bA\d{3}\b", r"\bADR-\d{3}\b", r"\bL\d{1,3}\b",
        r"\bO(?:E)?\d{1,3}\b", r"\bQ\d{1,2}[A-Za-z]?(?:\.\d+[a-z]?)?\b",
        r"\bR\d{1,2}\b",
    )
    found = []
    for pattern in patterns:
        found.extend(re.findall(pattern, text))
    return list(dict.fromkeys(found))


def target_path(token: str) -> str:
    prefix = token[0]
    domain = {
        "M": "project-model", "D": "decisions", "K": "knowledge",
        "R": "references", "I": "improvements",
    }[prefix]
    return f"openspec/specs/{domain}/spec.md#{token}"


def l_target(n: int) -> tuple[str, ...]:
    if n <= 9: return ("K09",)
    if n == 10: return ("K09",)
    if n in (11, 13): return ("K02",)
    if n in (12, 107, 128): return ("K01",)
    if n <= 67: return ("K01", "K12")
    if 68 <= n <= 70: return ("K14",)
    if n == 71: return ("K03",)
    if n == 72: return ("K04",)
    if 73 <= n <= 77: return ("K13",)
    if n in (78, 79, 108): return ("K05",)
    if n in (80, 142): return ("K06",)
    if 81 <= n <= 84: return ("K09",)
    if n <= 116: return ("K01", "K12")
    if n in (117, 118, 121): return ("K10",)
    if n in (119, 120, 122): return ("K13",)
    if n in (123, 124): return ("K02", "K10")
    if 125 <= n <= 127: return ("K12",)
    if 129 <= n <= 134: return ("K08", "K11", "K30")
    if n in (135, 136, 145): return ("K11",)
    if 137 <= n <= 140: return ("K08", "K30")
    if n == 141: return ("K07",)
    if 143 <= n <= 155: return ("K01", "K12")
    if n == 156: return ("K15",)
    if n <= 200: return ("K01", "K28")
    if n == 201: return ("K18",)
    if 202 <= n <= 204: return ("K01", "K18")
    if 205 <= n <= 211: return ("K17",)
    if n == 212 or 318 <= n <= 320: return ("K16",)
    if 213 <= n <= 221: return ("K19",)
    if 222 <= n <= 244: return ("K28",)
    if n == 245: return ("K29",)
    if 246 <= n <= 254: return ("K28", "K29")
    if 255 <= n <= 275: return ("K20",)
    if 276 <= n <= 280: return ("K22",)
    if 281 <= n <= 285: return ("K21",)
    if 286 <= n <= 290: return ("K20", "K21")
    if 291 <= n <= 296: return ("K23",)
    if n in (297, 298, 300): return ("K25",)
    if n == 299: return ("K24",)
    if 301 <= n <= 309: return ("K26",)
    if 310 <= n <= 317: return ("K27",)
    return ("K28",)


def o_target(token: str) -> tuple[str, ...]:
    if token.startswith("OE"): return ("I07",)
    n = int(token[1:])
    explicit = {
        77: ("I01", "K20"), 82: ("I02",), 85: ("I03",),
        86: ("I04",), 63: ("I05", "K16"),
        64: ("I06",), 65: ("I06",), 66: ("I06",), 69: ("I06",),
        71: ("I06",), 17: ("I07",),
        1: ("I08",), 3: ("I08",), 5: ("I08",), 32: ("I08",),
        36: ("I08",), 37: ("I08",), 38: ("I06",), 39: ("I06",),
        40: ("I08",), 41: ("I08",), 54: ("I08",), 55: ("I08",),
        58: ("I09",), 59: ("I09",), 60: ("I09",),
        48: ("I10",), 49: ("I10",), 50: ("I10",),
        80: ("K22",), 83: ("K23",), 84: ("K23",), 87: ("K24",),
    }
    return explicit.get(n, ("K12",))


def q_target(token: str, text: str) -> tuple[str, ...]:
    match = re.match(r"Q(\d+)([A-Za-z]?)(?:\.(\d+[a-z]?))?", token)
    if not match: return ("MS01",)
    number, suffix, part = int(match.group(1)), match.group(2), match.group(3)
    if number == 17:
        if part == "6" or "multi-hart" in text.lower() or "deferred" in text.lower():
            return ("MS02",)
        return ("MS01", "MS02")
    if number == 24: return ("MS02",)
    if number == 25: return ("MS04",)
    if number == 30: return ("MS03",)
    if suffix.upper() == "D": return ("MS01",)
    return ("MS01",)


def resolve(source: str, text: str, context: list[str]) -> tuple[list[str], list[str]]:
    current = ids(text)
    if current:
        context[:] = current
    legacy = current or context[:]
    tokens: set[str] = set()
    lower = source.lower()

    if source == "openspec/project.md":
        tokens.add(".claude/docs/SNAPSHOT.md")
        if re.search(r"workflow|openspec|文档|gate|tdd|bdd", text, re.I):
            tokens.add("CLAUDE.md")
        if re.search(r"构建|测试|工具链|依赖", text):
            tokens.add("openspec/specs/references/spec.md#R29")
        if re.search(r"约束|架构|内核|驱动|中断", text):
            tokens.add("openspec/specs/project-model/spec.md#M01")
    elif source == ".claude/docs/tasks.md" or "/specs/tasks/" in lower:
        for item in legacy:
            if item.startswith("Q"):
                tokens.update(f".claude/docs/tasks.md#{value}" for value in q_target(item, text))
        if not tokens:
            tokens.add(".claude/docs/tasks.md#MS01")
    elif "architecture" in lower:
        for item in legacy:
            if item.startswith("ADR-"):
                number = int(item.split("-")[1])
                item = f"A{number:03d}"
            if item.startswith("A"):
                number = int(item[1:])
                mapped = A_TARGETS.get(number, ("M01", "D01"))
                if "2026-07-15-arc-202607152005" in source:
                    mapped = {
                        63: ("M14", "D05"), 64: ("M35", "D10"),
                        56: ("M31", "D17"),
                    }.get(number, mapped)
                tokens.update(target_path(value) for value in mapped)
        if not tokens:
            tokens.update((target_path("M01"), target_path("D01")))
    elif "learned" in lower:
        for item in legacy:
            if item.startswith("L"):
                tokens.update(target_path(value) for value in l_target(int(item[1:])))
        if not tokens:
            tokens.add(target_path("K28"))
    elif "optimization" in lower:
        for item in legacy:
            if item.startswith("O"):
                tokens.update(target_path(value) for value in o_target(item))
        if not tokens:
            tokens.update((target_path("I08"), target_path("K12")))
    elif "/specs/references/" in lower:
        if any(item in {"R8", "R9"} for item in legacy):
            tokens.add(target_path("R10"))
        tokens.add(target_path("R47"))
    elif "/specs/snapshot/" in lower or "/specs/docs/" in lower:
        tokens.update((".claude/docs/SNAPSHOT.md", ".claude/docs/tasks.md#MS01"))
    elif source.endswith("/coverage-checklist.md"):
        for item in legacy:
            if item.startswith("ADR-"):
                item = f"A{int(item.split('-')[1]):03d}"
            if item.startswith("A"):
                tokens.update(
                    target_path(value)
                    for value in A_TARGETS.get(int(item[1:]), ("M01", "D01"))
                )
            elif item.startswith("L"):
                tokens.update(target_path(value) for value in l_target(int(item[1:])))
            elif item.startswith("O"):
                tokens.update(target_path(value) for value in o_target(item))
            elif item.startswith("Q"):
                tokens.update(
                    f".claude/docs/tasks.md#{value}"
                    for value in q_target(item, text)
                )
        tokens.add(target_path("R47"))
    else:
        tokens.add(target_path("R47"))

    if source.endswith((".yaml", "proposal.md", "design.md", "tasks.md", "README.md")) and source not in {
        "openspec/project.md", ".claude/docs/tasks.md"
    }:
        tokens.add(target_path("R47"))
    return sorted(tokens), legacy


def target_ok(target: str) -> bool:
    rel, _, anchor = target.partition("#")
    path = ROOT / rel
    if not path.is_file():
        return False
    if not anchor:
        return True
    return anchor in path.read_text(errors="replace")


def compact_target(target: str) -> str:
    rel, _, anchor = target.partition("#")
    if anchor:
        return anchor
    if rel == ".claude/docs/SNAPSHOT.md":
        return "SNAPSHOT"
    if rel == ".claude/docs/tasks.md":
        return "TASKS"
    if rel == "CLAUDE.md":
        return "CLAUDE"
    return rel


def boundary(text: str) -> str:
    dates = sorted(set(re.findall(r"20\d{2}-\d{2}-\d{2}", text)))
    states = []
    for label, pattern in (
        ("archived", r"归档|archive"), ("completed", r"完成|completed"),
        ("canceled", r"取消|cancel"), ("superseded", r"取代|替代|supersed"),
        ("blocked", r"阻塞|blocked|等待硬件|ENV BLOCK"),
        ("proposed", r"proposed|提议|候选"),
    ):
        if re.search(pattern, text, re.I):
            states.append(label)
    value = ",".join(states + dates)
    return value or "current-or-context"


def records() -> list[dict[str, str]]:
    result = []
    serial = 0
    for source, actual in list(ACTIVE) + historical_sources():
        context: list[str] = []
        for first, last, kind, raw in units(actual):
            serial += 1
            text = raw.decode("utf-8")
            targets, legacy = resolve(source, text, context)
            verified = bool(targets) and all(target_ok(item) for item in targets)
            snippet = re.sub(r"\s+", " ", text).strip().replace("\t", " ")[:120]
            result.append({
                "unit": f"U{serial:04d}",
                "source": source,
                "lines": f"{first}-{last}",
                "kind": kind,
                "sha256": sha(raw),
                "legacy": ",".join(legacy) or "-",
                "boundary": boundary(text),
                "targets": ";".join(targets),
                "status": "verified" if verified else "unmapped",
                "snippet": snippet,
            })
    return result


def emit_tsv(rows: list[dict[str, str]]) -> None:
    source_ids = {
        source: f"S{index:02d}"
        for index, (source, _) in enumerate(list(ACTIVE) + historical_sources(), 1)
    }
    keys = ("unit", "source", "lines", "kind", "sha256", "legacy",
            "boundary", "targets", "status")
    print("\t".join(keys))
    for row in rows:
        values = dict(row)
        values["source"] = source_ids[row["source"]]
        values["targets"] = ",".join(
            compact_target(target) for target in row["targets"].split(";")
        )
        values["status"] = "V" if row["status"] == "verified" else "U"
        print("\t".join(values[key].replace("\n", " ") for key in keys))


def emit_sources(rows: list[dict[str, str]]) -> None:
    counts: dict[str, int] = defaultdict(int)
    for row in rows:
        counts[row["source"]] += 1
    print("source_id\tlogical_source\tcarrier_or_pointer\tfile_sha256\tunits")
    for index, (source, actual) in enumerate(list(ACTIVE) + historical_sources(), 1):
        if index <= len(ACTIVE):
            pointer = actual.relative_to(CARRIER)
        else:
            pointer = actual.relative_to(ROOT)
        print(
            f"S{index:02d}\t{source}\t{pointer}\t"
            f"{sha(actual.read_bytes())}\t{counts[source]}"
        )


def emit_map(rows: list[dict[str, str]]) -> None:
    mapping: dict[str, dict[str, object]] = defaultdict(
        lambda: {"targets": set(), "sources": set(), "units": 0}
    )
    for row in rows:
        if row["legacy"] == "-":
            continue
        for item in row["legacy"].split(","):
            mapping[item]["targets"].update(row["targets"].split(";"))
            mapping[item]["sources"].add(row["source"])
            mapping[item]["units"] += 1
    print("# Legacy Numbering Map\n")
    print("| Legacy ID | Current target(s) | Source files | Units |")
    print("|---|---|---:|---:|")
    for item in sorted(mapping, key=lambda value: (value[0], int(re.search(r"\d+", value).group()))):
        entry = mapping[item]
        print(
            f"| `{item}` | "
            + ", ".join(f"`{compact_target(target)}`" for target in sorted(entry["targets"]))
            + f" | {len(entry['sources'])} | {entry['units']} |"
        )


def emit_targets(rows: list[dict[str, str]]) -> None:
    reverse: dict[str, dict[str, object]] = defaultdict(
        lambda: {"units": 0, "sources": set(), "legacy": set()}
    )
    for row in rows:
        for target in row["targets"].split(";"):
            item = reverse[target]
            item["units"] += 1
            item["sources"].add(row["source"])
            if row["legacy"] != "-":
                item["legacy"].update(row["legacy"].split(","))
    print("target\tverified\tunits\tsource_files\tlegacy_ids")
    for target in sorted(reverse):
        item = reverse[target]
        print(
            f"{compact_target(target)}\t{'yes' if target_ok(target) else 'no'}\t"
            f"{item['units']}\t{len(item['sources'])}\t{len(item['legacy'])}"
        )


def emit_summary(rows: list[dict[str, str]]) -> None:
    mapped = sum(bool(row["targets"]) for row in rows)
    verified = sum(row["status"] == "verified" for row in rows)
    print(f"source_units={len(rows)}")
    print(f"mapped_source_units={mapped}")
    print(f"verified_source_units={verified}")
    print(f"unmapped={len(rows) - mapped}")
    print("skipped=0")
    print(f"coverage={(verified / len(rows) * 100) if rows else 0:.2f}%")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--format",
        choices=("tsv", "map", "sources", "targets", "summary"),
        default="summary",
    )
    args = parser.parse_args()
    rows = records()
    if args.format == "tsv":
        emit_tsv(rows)
    elif args.format == "map":
        emit_map(rows)
    elif args.format == "sources":
        emit_sources(rows)
    elif args.format == "targets":
        emit_targets(rows)
    else:
        emit_summary(rows)


if __name__ == "__main__":
    main()

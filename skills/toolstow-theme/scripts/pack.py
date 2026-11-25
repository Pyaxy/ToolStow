#!/usr/bin/env python3
"""Validate and package a ToolStow theme directory into a clean flat .zip."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from typing import List, Optional

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate import (  # noqa: E402
    DEFAULT_MAX_ZIP_BYTES,
    build_theme_zip_bytes,
    print_result,
    validate_theme_dir,
)


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate a ToolStow theme directory and build a clean flat .zip."
    )
    parser.add_argument(
        "theme_dir",
        type=Path,
        help="Path to the theme directory containing theme.json",
    )
    parser.add_argument(
        "-o",
        "--output",
        type=Path,
        default=None,
        help="Output .zip path (default: ./toolstow-theme-<short>.zip)",
    )
    parser.add_argument(
        "--max-mb",
        type=float,
        default=DEFAULT_MAX_ZIP_BYTES / (1024 * 1024),
        help="Maximum allowed .zip size in MiB (default: 50)",
    )
    parser.add_argument(
        "--strict-dir-name",
        action="store_true",
        help="Treat directory name != theme.short as an error",
    )
    parser.add_argument(
        "--skip-js-check",
        action="store_true",
        help="Skip optional 'node --check' JS syntax verification",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail packaging if validation emits any warnings",
    )
    args = parser.parse_args(argv)

    res = validate_theme_dir(
        args.theme_dir,
        skip_js_check=args.skip_js_check,
        strict_dir_name=args.strict_dir_name,
    )
    if not print_result(res, strict=args.strict):
        return 1

    assert res.short is not None
    out_path = (
        args.output
        if args.output is not None
        else Path(f"toolstow-theme-{res.short}.zip")
    )
    max_bytes = int(args.max_mb * 1024 * 1024)

    try:
        zip_bytes, count = build_theme_zip_bytes(args.theme_dir, max_zip_bytes=max_bytes)
        out_path.parent.mkdir(parents=True, exist_ok=True)
        out_path.write_bytes(zip_bytes)
    except (OSError, ValueError) as exc:
        print(f"ERROR: failed to create ZIP archive: {exc}", file=sys.stderr)
        return 1

    print(f"PACKED: {out_path.resolve()} ({count} files, {len(zip_bytes):,} bytes)")
    print("NEXT:   Upload this .zip in /admin -> 主题管理 (theme uploads require Admin session).")
    return 0


if __name__ == "__main__":
    sys.exit(main())

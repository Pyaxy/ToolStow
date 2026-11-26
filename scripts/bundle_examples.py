#!/usr/bin/env python3
"""Discover, strictly validate, pack, and bundle ToolStow example themes & plugins.

Shared by both `.github/workflows/ci.yml` and `.github/workflows/release.yml` so
PR validation and Release packaging always use the exact same discovery and ZIP
packaging pipeline.

Outputs inside `--out-dir` (default: ./dist):
  - toolstow-themes.zip   (contains individual ready-to-upload toolstow-theme-*.zip)
  - toolstow-plugins.zip  (contains individual ready-to-upload toolstow-plugin-*.zip)
"""

from __future__ import annotations

import argparse
import io
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from typing import List, Optional, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent
THEME_PACK = REPO_ROOT / "skills" / "toolstow-theme" / "scripts" / "pack.py"
TOOL_PACK = REPO_ROOT / "skills" / "toolstow-tool" / "scripts" / "pack.py"
EXAMPLES_DIR = REPO_ROOT / "examples"


def discover_examples() -> Tuple[List[Path], List[Path]]:
    themes_root = EXAMPLES_DIR / "themes"
    tools_root = EXAMPLES_DIR / "tools"
    themes = sorted(
        p.parent
        for p in themes_root.glob("*/theme.json")
        if p.is_file() and not p.parent.name.startswith(".")
    )
    plugins = sorted(
        p.parent
        for p in tools_root.glob("*/manifest.json")
        if p.is_file() and not p.parent.name.startswith(".")
    )
    return themes, plugins


def run_pack(pack_script: Path, source_dir: Path, out_zip: Path) -> None:
    cmd = [
        sys.executable,
        str(pack_script),
        "--strict",
        "--strict-dir-name",
        str(source_dir),
        "-o",
        str(out_zip),
    ]
    proc = subprocess.run(cmd, check=False)
    if proc.returncode != 0:
        raise RuntimeError(f"pack failed ({proc.returncode}) for {source_dir}")


def write_outer_bundle(inner_zips: List[Path], bundle_path: Path) -> None:
    bundle_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(bundle_path, mode="w", compression=zipfile.ZIP_STORED) as zf:
        for inner in sorted(inner_zips, key=lambda p: p.name):
            info = zipfile.ZipInfo(inner.name)
            info.compress_type = zipfile.ZIP_STORED
            info.external_attr = 0o100644 << 16
            zf.writestr(info, inner.read_bytes())


def verify_bundle(bundle_path: Path, expected_names: List[str], inner_manifest: str) -> None:
    with zipfile.ZipFile(bundle_path, mode="r") as outer:
        bad = outer.testzip()
        if bad is not None:
            raise RuntimeError(f"corrupt entry '{bad}' in outer bundle {bundle_path.name}")
        actual = sorted(outer.namelist())
        expected = sorted(expected_names)
        if actual != expected:
            raise RuntimeError(
                f"bundle {bundle_path.name} entries mismatch: expected {expected}, got {actual}"
            )
        for name in actual:
            raw = outer.read(name)
            with zipfile.ZipFile(io.BytesIO(raw), mode="r") as inner:
                inner_bad = inner.testzip()
                if inner_bad is not None:
                    raise RuntimeError(
                        f"corrupt inner entry '{inner_bad}' inside {bundle_path.name}:{name}"
                    )
                if inner_manifest not in inner.namelist():
                    raise RuntimeError(
                        f"missing '{inner_manifest}' at root of {bundle_path.name}:{name}"
                    )


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Discover, validate, pack, and bundle ToolStow example themes and plugins."
    )
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=REPO_ROOT / "dist",
        help="Directory to write toolstow-themes.zip and toolstow-plugins.zip (default: ./dist)",
    )
    parser.add_argument(
        "--require-node",
        action="store_true",
        help="Fail fast if `node` is not installed on PATH (recommended in CI/Release)",
    )
    args = parser.parse_args(argv)

    if args.require_node and shutil.which("node") is None:
        print("ERROR: `node` is required on PATH (--require-node) for JS syntax checks", file=sys.stderr)
        return 1

    themes, plugins = discover_examples()
    if not themes:
        print("ERROR: no example themes found under examples/themes/*/theme.json", file=sys.stderr)
        return 1
    if not plugins:
        print("ERROR: no example plugins found under examples/tools/*/manifest.json", file=sys.stderr)
        return 1

    print(
        f"Discovered {len(themes)} themes ({', '.join(d.name for d in themes)}) and "
        f"{len(plugins)} plugins ({', '.join(d.name for d in plugins)})"
    )

    with tempfile.TemporaryDirectory(prefix="toolstow-bundle-") as tmp:
        tmp_path = Path(tmp)
        theme_zips: List[Path] = []
        plugin_zips: List[Path] = []

        try:
            for tdir in themes:
                out = tmp_path / f"toolstow-theme-{tdir.name}.zip"
                run_pack(THEME_PACK, tdir, out)
                theme_zips.append(out)

            for pdir in plugins:
                out = tmp_path / f"toolstow-plugin-{pdir.name}.zip"
                run_pack(TOOL_PACK, pdir, out)
                plugin_zips.append(out)

            themes_bundle = args.out_dir / "toolstow-themes.zip"
            plugins_bundle = args.out_dir / "toolstow-plugins.zip"

            write_outer_bundle(theme_zips, themes_bundle)
            write_outer_bundle(plugin_zips, plugins_bundle)

            verify_bundle(themes_bundle, [p.name for p in theme_zips], "theme.json")
            verify_bundle(plugins_bundle, [p.name for p in plugin_zips], "manifest.json")
        except (OSError, RuntimeError, zipfile.BadZipFile) as exc:
            print(f"ERROR: {exc}", file=sys.stderr)
            return 1

    print(
        f"BUNDLED: {themes_bundle.resolve()} ({len(theme_zips)} themes, "
        f"{themes_bundle.stat().st_size:,} bytes)"
    )
    print(
        f"BUNDLED: {plugins_bundle.resolve()} ({len(plugin_zips)} plugins, "
        f"{plugins_bundle.stat().st_size:,} bytes)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

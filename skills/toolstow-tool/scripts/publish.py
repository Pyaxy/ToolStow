#!/usr/bin/env python3
"""Validate, pack, and publish a ToolStow micro-tool via Bearer API (TOOLSTOW_API_KEY).

Defaults to `?overwrite=false` to prevent accidental overwrites. Pass `--overwrite`
to replace an existing tool with the same manifest.id.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
import uuid
from pathlib import Path
from typing import Dict, List, Optional, Tuple

SCRIPT_DIR = Path(__file__).resolve().parent
if str(SCRIPT_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPT_DIR))

from validate import (  # noqa: E402
    DEFAULT_MAX_ZIP_BYTES,
    build_tool_zip_bytes,
    print_result,
    validate_tool_dir,
)


def load_dotenv(env_path: Path) -> Dict[str, str]:
    values: Dict[str, str] = {}
    if not env_path.is_file():
        return values
    for raw_line in env_path.read_text(encoding="utf-8").splitlines():
        line = raw_line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        if line.startswith("export "):
            line = line[len("export ") :].strip()
        key, val = line.split("=", 1)
        key = key.strip()
        val = val.strip()
        if len(val) >= 2 and val[0] == val[-1] and val[0] in ("'", '"'):
            val = val[1:-1]
        if key:
            values[key] = val
    return values


def encode_multipart_file(field_name: str, filename: str, payload: bytes) -> Tuple[bytes, str]:
    boundary = f"----ToolStowBoundary{uuid.uuid4().hex}"
    safe_name = filename.replace('"', "").replace("\r", "").replace("\n", "")
    header = (
        f"--{boundary}\r\n"
        f'Content-Disposition: form-data; name="{field_name}"; filename="{safe_name}"\r\n'
        f"Content-Type: application/zip\r\n\r\n"
    ).encode("utf-8")
    footer = f"\r\n--{boundary}--\r\n".encode("utf-8")
    return header + payload + footer, f"multipart/form-data; boundary={boundary}"


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate, pack, and upload a ToolStow micro-tool using TOOLSTOW_API_KEY."
    )
    parser.add_argument(
        "target",
        type=Path,
        help="Path to a tool directory (recommended) or a pre-built .zip archive",
    )
    parser.add_argument(
        "--url",
        default=None,
        help="ToolStow base URL (default: $TOOLSTOW_URL or http://127.0.0.1:8080)",
    )
    parser.add_argument(
        "--env-file",
        type=Path,
        default=None,
        help="Optional .env file to load TOOLSTOW_URL and TOOLSTOW_API_KEY from",
    )
    parser.add_argument(
        "--overwrite",
        action="store_true",
        help="Allow overwriting an existing tool with the same ID (?overwrite=true)",
    )
    parser.add_argument(
        "--max-mb",
        type=float,
        default=DEFAULT_MAX_ZIP_BYTES / (1024 * 1024),
        help="Maximum allowed .zip size in MiB (default: 50)",
    )
    parser.add_argument(
        "--allow-standalone",
        action="store_true",
        help="Do not require /ui/v1/toolstow.css and /ui/v1/theme.js",
    )
    parser.add_argument(
        "--strict-dir-name",
        action="store_true",
        help="Treat directory name != manifest.id as an error",
    )
    parser.add_argument(
        "--skip-js-check",
        action="store_true",
        help="Skip optional 'node --check' JS syntax verification",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail publishing if validation emits any warnings",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=30,
        help="HTTP request timeout in seconds (default: 30)",
    )
    args = parser.parse_args(argv)

    dotenv = load_dotenv(args.env_file) if args.env_file else {}

    base_url = (
        args.url
        or os.environ.get("TOOLSTOW_URL")
        or dotenv.get("TOOLSTOW_URL")
        or "http://127.0.0.1:8080"
    ).rstrip("/")
    api_key = (
        os.environ.get("TOOLSTOW_API_KEY") or dotenv.get("TOOLSTOW_API_KEY") or ""
    ).strip()

    if not api_key:
        print(
            "ERROR: TOOLSTOW_API_KEY is not set. Export TOOLSTOW_API_KEY (never TOOLSTOW_ADMIN_KEY) "
            "or pass --env-file .env",
            file=sys.stderr,
        )
        return 1

    max_bytes = int(args.max_mb * 1024 * 1024)

    if args.target.is_dir():
        res = validate_tool_dir(
            args.target,
            allow_standalone=args.allow_standalone,
            skip_js_check=args.skip_js_check,
            strict_dir_name=args.strict_dir_name,
        )
        if not print_result(res, strict=args.strict):
            return 1
        assert res.tool_id is not None
        filename = f"{res.tool_id}.zip"
        try:
            zip_bytes, _ = build_tool_zip_bytes(args.target, max_zip_bytes=max_bytes)
        except ValueError as exc:
            print(f"ERROR: failed to build ZIP: {exc}", file=sys.stderr)
            return 1
    elif args.target.is_file() and args.target.suffix.lower() == ".zip":
        zip_bytes = args.target.read_bytes()
        if len(zip_bytes) > max_bytes:
            print(
                f"ERROR: ZIP archive exceeds {args.max_mb:.1f} MiB limit",
                file=sys.stderr,
            )
            return 1
        filename = args.target.name
    else:
        print(
            f"ERROR: target must be a tool directory or .zip file: {args.target}",
            file=sys.stderr,
        )
        return 1

    overwrite_flag = "true" if args.overwrite else "false"
    endpoint = f"{base_url}/api/admin/tools/upload?overwrite={overwrite_flag}"
    body, content_type = encode_multipart_file("file", filename, zip_bytes)

    req = urllib.request.Request(
        endpoint,
        data=body,
        method="POST",
        headers={
            "Authorization": f"Bearer {api_key}",
            "Content-Type": content_type,
            "Accept": "application/json",
        },
    )

    try:
        with urllib.request.urlopen(req, timeout=args.timeout) as resp:
            status_code = resp.getcode()
            raw_resp = resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as exc:
        status_code = exc.code
        raw_resp = exc.read().decode("utf-8", errors="replace")
        try:
            err_json = json.loads(raw_resp)
        except json.JSONDecodeError:
            err_json = None

        if status_code == 409 and isinstance(err_json, dict):
            existing = err_json.get("existing") or {}
            incoming = err_json.get("incoming") or {}
            print(
                f"CONFLICT (409 TOOL_EXISTS): tool '{incoming.get('id', filename)}' already exists.",
                file=sys.stderr,
            )
            if existing:
                print(
                    f"  existing: {existing.get('name')} v{existing.get('version')} "
                    f"(draft={existing.get('draft')})",
                    file=sys.stderr,
                )
            if incoming:
                print(
                    f"  incoming: {incoming.get('name')} v{incoming.get('version')} "
                    f"(draft={incoming.get('draft')})",
                    file=sys.stderr,
                )
            print("  Re-run with --overwrite to replace the existing tool.", file=sys.stderr)
            return 2

        msg = (
            err_json.get("message")
            if isinstance(err_json, dict) and err_json.get("message")
            else raw_resp.strip() or exc.reason
        )
        print(f"ERROR: upload failed (HTTP {status_code}): {msg}", file=sys.stderr)
        if status_code in (401, 403):
            print(
                "  Hint: verify TOOLSTOW_API_KEY is configured on the server "
                "(TOOLSTOW_ADMIN_KEY is rejected for Bearer API).",
                file=sys.stderr,
            )
        elif status_code == 404:
            print(
                "  Hint: check --url / TOOLSTOW_URL and verify TOOLSTOW_API_KEY is enabled on host.",
                file=sys.stderr,
            )
        return 1
    except urllib.error.URLError as exc:
        print(f"ERROR: failed to connect to {base_url}: {exc.reason}", file=sys.stderr)
        return 1

    try:
        data = json.loads(raw_resp)
    except json.JSONDecodeError:
        print(f"PUBLISHED (HTTP {status_code}): {raw_resp.strip()}")
        return 0

    tool = data.get("tool") if isinstance(data, dict) and isinstance(data.get("tool"), dict) else data
    tool_id = tool.get("id", "unknown") if isinstance(tool, dict) else "unknown"
    version = tool.get("version", "") if isinstance(tool, dict) else ""
    draft = tool.get("draft", False) if isinstance(tool, dict) else False
    verb = "CREATED" if status_code == 201 else "UPDATED"
    print(
        f"{verb} (HTTP {status_code}): '{tool_id}' v{version} "
        f"(draft={draft}) -> {base_url}/#tool={tool_id}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

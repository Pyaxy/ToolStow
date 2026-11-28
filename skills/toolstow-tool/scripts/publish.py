#!/usr/bin/env python3
"""Validate, pack, and publish a ToolStow micro-tool via Bearer API (TOOLSTOW_API_KEY).

Defaults to `?overwrite=false` to prevent accidental overwrites. Pass `--overwrite`
to replace an existing tool with the same manifest.id.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import sys
import urllib.error
import urllib.request
import uuid
import zipfile
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


VALID_VISIBILITIES = ("public", "private", "draft")
MAX_INSPECT_MANIFEST_BYTES = 10 * 1024 * 1024  # 10 MiB safety cap for uncompressed manifest.json


def resolve_tool_visibility(tool_obj: object) -> str:
    if isinstance(tool_obj, dict):
        raw_vis = tool_obj.get("visibility")
        if isinstance(raw_vis, str):
            vis = raw_vis.strip().lower()
            if vis in VALID_VISIBILITIES:
                return vis
        if tool_obj.get("draft") is True:
            return "draft"
    return "public"


def inspect_zip_manifest(
    zip_bytes: bytes,
    *,
    max_manifest_bytes: int = MAX_INSPECT_MANIFEST_BYTES,
) -> Tuple[Optional[Dict[str, object]], Optional[str]]:
    """Inspect actual ZIP bytes in memory and return (manifest_dict, error_reason).

    Supports both flat archives (`manifest.json` at root) and single top-level wrapper
    folder archives (`<dir>/manifest.json`), ignoring macOS `__MACOSX/` and `.DS_Store` junk.
    Fails closed by returning an error reason whenever `manifest.json` is missing,
    duplicated, a symlink, exceeds `max_manifest_bytes`, unreadable, or invalid JSON.
    """
    try:
        zf = zipfile.ZipFile(io.BytesIO(zip_bytes), "r")
    except (zipfile.BadZipFile, OSError) as exc:
        return None, f"invalid ZIP archive: {exc}"

    with zf:
        entries: List[Tuple[List[str], zipfile.ZipInfo]] = []
        for info in zf.infolist():
            if info.is_dir():
                continue
            normalized = info.filename.replace("\\", "/")
            parts = [p for p in normalized.split("/") if p and p != "."]
            if not parts or ".." in parts:
                continue
            if parts[0] == "__MACOSX" or parts[-1] == ".DS_Store" or parts[-1].startswith("._"):
                continue
            entries.append((parts, info))

        root_manifests = [info for parts, info in entries if parts == ["manifest.json"]]
        if len(root_manifests) > 1:
            return None, "duplicate manifest.json entries found in ZIP archive"
        manifest_info: Optional[zipfile.ZipInfo] = root_manifests[0] if root_manifests else None

        if manifest_info is None and entries and all(len(parts) >= 2 for parts, _ in entries):
            top_dirs = {parts[0] for parts, _ in entries}
            if len(top_dirs) == 1:
                only_top = next(iter(top_dirs))
                nested_manifests = [
                    info for parts, info in entries if parts == [only_top, "manifest.json"]
                ]
                if len(nested_manifests) > 1:
                    return None, "duplicate manifest.json entries found in ZIP archive"
                if nested_manifests:
                    manifest_info = nested_manifests[0]

        if manifest_info is None:
            return None, "manifest.json not found in ZIP archive"

        unix_mode = (manifest_info.external_attr >> 16) & 0o170000
        if unix_mode == 0o120000:
            return None, "manifest.json in ZIP archive is a symlink"

        if manifest_info.file_size > max_manifest_bytes:
            return (
                None,
                f"manifest.json uncompressed size ({manifest_info.file_size} bytes) "
                f"exceeds inspection limit ({max_manifest_bytes} bytes)",
            )

        try:
            with zf.open(manifest_info, "r") as handle:
                raw_bytes = handle.read(max_manifest_bytes + 1)
        except (KeyError, RuntimeError, zipfile.BadZipFile, OSError) as exc:
            return None, f"failed to read manifest.json from ZIP archive: {exc}"

        if len(raw_bytes) > max_manifest_bytes:
            return (
                None,
                f"manifest.json decompressed size exceeds inspection limit ({max_manifest_bytes} bytes)",
            )

        try:
            raw = json.loads(raw_bytes.decode("utf-8-sig"))
        except UnicodeDecodeError as exc:
            return None, f"manifest.json is not valid UTF-8: {exc}"
        except json.JSONDecodeError as exc:
            return None, f"manifest.json contains invalid JSON: {exc}"

        if not isinstance(raw, dict):
            return None, "manifest.json root must be a JSON object"
        return raw, None


def validate_manifest_visibility(
    manifest: Dict[str, object],
) -> Tuple[Optional[str], Optional[str]]:
    """Validate `visibility` and legacy `draft` fields, returning (resolved_visibility, error)."""
    raw_visibility = manifest.get("visibility")
    parsed_visibility: Optional[str] = None
    if raw_visibility is not None:
        if not isinstance(raw_visibility, str):
            return (
                None,
                "manifest 'visibility' must be a string ('public', 'private', or 'draft')",
            )
        vis = raw_visibility.strip().lower()
        if vis not in VALID_VISIBILITIES:
            return (
                None,
                f"invalid manifest 'visibility' {raw_visibility!r}: "
                "expected 'public', 'private', or 'draft'",
            )
        parsed_visibility = vis

    raw_draft = manifest.get("draft")
    parsed_draft: Optional[bool] = None
    if raw_draft is not None:
        if not isinstance(raw_draft, bool):
            return None, "manifest 'draft' must be a boolean"
        parsed_draft = raw_draft

    if parsed_visibility is not None and parsed_draft is not None:
        if (parsed_visibility == "draft") != parsed_draft:
            return (
                None,
                f"conflicting manifest fields: 'visibility': {parsed_visibility!r} "
                f"conflicts with legacy 'draft': {str(parsed_draft).lower()}",
            )
        return parsed_visibility, None

    if parsed_visibility is not None:
        return parsed_visibility, None
    if parsed_draft is not None:
        return ("draft" if parsed_draft else "public"), None
    return "public", None


def manifest_requires_visibility_support(
    manifest: Dict[str, object],
    resolved_visibility: Optional[str] = None,
) -> Tuple[bool, str]:
    """Return (requires_visibility_support, resolved_visibility).

    Legacy ToolStow servers (<= v0.3.5) ignore `"visibility"` and only inspect `"draft": true`,
    so uploading a `"private"` tool or a visibility-only `"draft"` tool (without `"draft": true`)
    would silently publish it as `public`.
    """
    if resolved_visibility is None:
        resolved_visibility, vis_err = validate_manifest_visibility(manifest)
        if vis_err is not None or resolved_visibility is None:
            raise ValueError(vis_err or "invalid manifest visibility")
    if resolved_visibility == "private":
        return True, resolved_visibility
    if resolved_visibility == "draft" and manifest.get("draft") is not True:
        return True, resolved_visibility
    return False, resolved_visibility


def check_server_visibility_capability(base_url: str, timeout: int, visibility: str) -> bool:
    """Verify GET /healthz advertises `"tool-visibility"` before uploading sensitive visibility."""
    health_url = f"{base_url}/healthz"
    req = urllib.request.Request(
        health_url,
        method="GET",
        headers={"Accept": "application/json"},
    )
    detected_version: Optional[str] = None
    failure_detail: Optional[str] = None

    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            raw_body = resp.read().decode("utf-8", errors="replace")
        health = json.loads(raw_body)
        if isinstance(health, dict):
            ver = health.get("version")
            if isinstance(ver, str) and ver.strip():
                detected_version = ver.strip()
            caps = health.get("capabilities")
            if isinstance(caps, list) and "tool-visibility" in caps:
                return True
            failure_detail = "/healthz does not advertise capability 'tool-visibility'"
        else:
            failure_detail = "/healthz returned non-object JSON"
    except urllib.error.HTTPError as exc:
        failure_detail = f"GET /healthz returned HTTP {exc.code}"
    except urllib.error.URLError as exc:
        failure_detail = f"failed to reach {health_url}: {exc.reason}"
    except (TimeoutError, OSError) as exc:
        failure_detail = f"failed to reach {health_url}: {exc}"
    except json.JSONDecodeError:
        failure_detail = "GET /healthz returned invalid JSON"

    pkg_desc = (
        "visibility='private'"
        if visibility == "private"
        else "visibility='draft' (without legacy 'draft': true)"
    )
    ver_note = f" (detected server version: {detected_version})" if detected_version else ""
    print(
        f"ERROR: aborting upload before sending package: target server at {base_url}{ver_note} "
        f"does not report 3-state visibility support ({failure_detail}).",
        file=sys.stderr,
    )
    print(
        f"  Refusing to upload a {pkg_desc} package because legacy ToolStow servers (<= v0.3.5) "
        "ignore 'visibility' and would silently expose this tool as 'public'.",
        file=sys.stderr,
    )
    print(
        "  Upgrade the ToolStow server to a version advertising 'tool-visibility' in /healthz "
        "before uploading.",
        file=sys.stderr,
    )
    return False


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
        try:
            zip_bytes = args.target.read_bytes()
        except OSError as exc:
            print(f"ERROR: failed to read ZIP archive '{args.target}': {exc}", file=sys.stderr)
            return 1
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

    # Always inspect the actual zip_bytes about to be uploaded and fail closed if unreadable/invalid.
    manifest_obj, inspect_err = inspect_zip_manifest(
        zip_bytes,
        max_manifest_bytes=MAX_INSPECT_MANIFEST_BYTES,
    )
    if inspect_err is not None or manifest_obj is None:
        print(
            "ERROR: aborting upload before sending package: failed to read/parse manifest.json "
            f"from ZIP archive to verify tool visibility ({inspect_err or 'unknown error'}).",
            file=sys.stderr,
        )
        return 1

    resolved_visibility, vis_err = validate_manifest_visibility(manifest_obj)
    if vis_err is not None or resolved_visibility is None:
        print(
            "ERROR: aborting upload before sending package: invalid visibility configuration "
            f"in manifest.json ({vis_err or 'unknown error'}).",
            file=sys.stderr,
        )
        return 1

    requires_visibility_support, resolved_visibility = manifest_requires_visibility_support(
        manifest_obj,
        resolved_visibility=resolved_visibility,
    )
    if requires_visibility_support and not check_server_visibility_capability(
        base_url, args.timeout, resolved_visibility
    ):
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
                existing_vis = resolve_tool_visibility(existing)
                print(
                    f"  existing: {existing.get('name')} v{existing.get('version')} "
                    f"(visibility={existing_vis})",
                    file=sys.stderr,
                )
            if incoming:
                incoming_vis = resolve_tool_visibility(incoming)
                print(
                    f"  incoming: {incoming.get('name')} v{incoming.get('version')} "
                    f"(visibility={incoming_vis})",
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
    visibility = resolve_tool_visibility(tool)
    verb = "CREATED" if status_code == 201 else "UPDATED"
    if visibility == "public":
        target_note = f"-> {base_url}/#tool={tool_id}"
    elif visibility == "private":
        target_note = f"(admin-only on /) -> {base_url}/#tool={tool_id} (requires /admin session)"
    else:
        target_note = f"(preview in /admin) -> {base_url}/admin"
    print(
        f"{verb} (HTTP {status_code}): '{tool_id}' v{version} "
        f"(visibility={visibility}) {target_note}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())

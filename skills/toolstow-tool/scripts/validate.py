#!/usr/bin/env python3
"""Validate a ToolStow static micro-tool directory.

Zero-dependency Python 3.9+ validator aligned with ToolStow's Rust backend
(src/manifest.rs and src/archive.rs) plus static security/SDK hygiene checks.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

MAX_ID_LEN = 64
MAX_FILES = 1000
MAX_UNCOMPRESSED_BYTES = 200 * 1024 * 1024  # 200 MiB (matches src/archive.rs)
DEFAULT_MAX_ZIP_BYTES = 50 * 1024 * 1024  # 50 MiB default upload limit

ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")

SANDBOX_ALLOWLIST = frozenset(
    {
        "allow-scripts",
        "allow-downloads",
        "allow-forms",
        "allow-modals",
        "allow-popups",
        "allow-popups-to-escape-sandbox",
    }
)

FORBIDDEN_SANDBOX_PREFIXES = ("allow-same-origin", "allow-top-navigation")

IGNORED_NAMES = frozenset({".DS_Store", "__MACOSX", ".git", ".gitignore", "Thumbs.db"})

# Patterns checked on comment-stripped JS
DANGEROUS_DOM_PATTERNS: Tuple[Tuple[str, re.Pattern[str]], ...] = (
    ("innerHTML assignment", re.compile(r"\.innerHTML\s*(?:=|\+=)")),
    ("outerHTML assignment", re.compile(r"\.outerHTML\s*(?:=|\+=)")),
    ("insertAdjacentHTML call", re.compile(r"\.insertAdjacentHTML\s*\(")),
    ("document.write call", re.compile(r"\bdocument\.write(?:ln)?\s*\(")),
)

STORAGE_RE = re.compile(
    r"\b(?:localStorage|sessionStorage)\b|\bdocument\.cookie\b"
)
TRY_RE = re.compile(r"\btry\s*\{")
ALLOW_SAME_ORIGIN_RE = re.compile(r"\ballow-same-origin\b", re.IGNORECASE)

CSS_COMMENT_RE = re.compile(r"/\*.*?\*/", re.DOTALL)
CSS_RULE_RE = re.compile(r"([^{}]+)\{([^{}]*)\}")
WB_SLOT_SELECTOR_RE = re.compile(
    r"\.ts-(?:workbench|wb-toolbar|wb-nav|wb-aside|wb-main)\b"
)
WB_LAYOUT_OVERRIDE_RE = re.compile(
    r"\b(?:grid-template-(?:areas|columns|rows)|grid-(?:area|column|row)(?:-(?:start|end))?|order)\s*:|"
    r"\bposition\s*:\s*(?:fixed|absolute)\b",
    re.IGNORECASE,
)


@dataclass
class ValidationResult:
    tool_dir: Path
    tool_id: Optional[str] = None
    name: Optional[str] = None
    version: str = "0.1.0"
    entry: str = "index.html"
    draft: bool = False
    file_count: int = 0
    total_bytes: int = 0
    js_checked_count: int = 0
    js_check_status: str = "not run"
    errors: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.errors

    def to_dict(self) -> Dict[str, Any]:
        return {
            "toolDir": str(self.tool_dir),
            "id": self.tool_id,
            "name": self.name,
            "version": self.version,
            "entry": self.entry,
            "draft": self.draft,
            "fileCount": self.file_count,
            "totalBytes": self.total_bytes,
            "jsCheckedCount": self.js_checked_count,
            "jsCheckStatus": self.js_check_status,
            "ok": self.ok,
            "errors": self.errors,
            "warnings": self.warnings,
        }


class _HtmlInspector(HTMLParser):
    """Extracts <link>, <script>, inline <script>, and inline <style> blocks from an HTML document."""

    def __init__(self) -> None:
        super().__init__(convert_charrefs=False)
        self.stylesheets: List[Dict[str, Optional[str]]] = []
        self.external_scripts: List[Dict[str, Optional[str]]] = []
        self.inline_scripts: List[Tuple[str, str]] = []  # (type_attr, source)
        self.inline_styles: List[str] = []
        self.iframe_sandbox_attrs: List[str] = []
        self.meta_csp_contents: List[str] = []
        self._in_script = False
        self._script_type = ""
        self._script_has_src = False
        self._script_chunks: List[str] = []
        self._in_style = False
        self._style_chunks: List[str] = []

    def handle_starttag(self, tag: str, attrs: List[Tuple[str, Optional[str]]]) -> None:
        tag_l = tag.lower()
        attr_map: Dict[str, Optional[str]] = {k.lower(): v for k, v in attrs}
        if tag_l == "link":
            rel = (attr_map.get("rel") or "").lower().split()
            if "stylesheet" in rel:
                self.stylesheets.append(attr_map)
        elif tag_l == "iframe":
            if "sandbox" in attr_map:
                self.iframe_sandbox_attrs.append(attr_map.get("sandbox") or "")
        elif tag_l == "meta":
            if (attr_map.get("http-equiv") or "").strip().lower() == "content-security-policy":
                self.meta_csp_contents.append(attr_map.get("content") or "")
        elif tag_l == "script":
            self._in_script = True
            self._script_type = (attr_map.get("type") or "").strip().lower()
            src = attr_map.get("src")
            self._script_has_src = src is not None and src.strip() != ""
            self._script_chunks = []
            if self._script_has_src:
                self.external_scripts.append(attr_map)
        elif tag_l == "style":
            self._in_style = True
            self._style_chunks = []

    def handle_data(self, data: str) -> None:
        if self._in_script and not self._script_has_src:
            self._script_chunks.append(data)
        elif self._in_style:
            self._style_chunks.append(data)

    def handle_endtag(self, tag: str) -> None:
        tag_l = tag.lower()
        if tag_l == "script" and self._in_script:
            if not self._script_has_src:
                src_text = "".join(self._script_chunks)
                if src_text.strip():
                    self.inline_scripts.append((self._script_type, src_text))
            self._in_script = False
            self._script_type = ""
            self._script_has_src = False
            self._script_chunks = []
        elif tag_l == "style" and self._in_style:
            style_text = "".join(self._style_chunks)
            if style_text.strip():
                self.inline_styles.append(style_text)
            self._in_style = False
            self._style_chunks = []


def is_ignored_name(name: str) -> bool:
    return name in IGNORED_NAMES or name.startswith("._")


def validate_id(value: str) -> Optional[str]:
    if not isinstance(value, str) or not value or len(value) > MAX_ID_LEN:
        return (
            f"invalid manifest 'id' {value!r}: length must be between 1 and "
            f"{MAX_ID_LEN} characters"
        )
    if not ID_RE.match(value):
        return (
            f"invalid manifest 'id' {value!r}: must match ^[a-z0-9]+(?:-[a-z0-9]+)*$"
        )
    return None


def validate_safe_rel_path(path_str: str) -> Tuple[Optional[str], Optional[str]]:
    """Mirror of src/manifest.rs::validate_safe_rel_path. Returns (posix_path, error)."""
    if not isinstance(path_str, str) or not path_str:
        return None, "relative path must not be empty"
    if any(ord(ch) == 0 or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in path_str):
        return None, f"invalid relative path {path_str!r}: contains control characters"
    if path_str.startswith("/") or path_str.endswith("/"):
        return None, f"invalid relative path {path_str!r}: must not start or end with '/'"
    if "\\" in path_str:
        return None, f"invalid relative path {path_str!r}: backslashes are not allowed"
    if ":" in path_str:
        return (
            None,
            f"invalid relative path {path_str!r}: drive prefixes and URI schemes are not allowed",
        )
    segments = path_str.split("/")
    for seg in segments:
        if not seg or seg in (".", ".."):
            return (
                None,
                f"invalid relative path {path_str!r}: contains disallowed segment {seg!r}",
            )
    return "/".join(segments), None


def is_path_like_icon(icon: str) -> bool:
    return any(ch in icon for ch in ("/", "\\", ".", ":"))


def ensure_regular_file_within(root: Path, rel_posix: str, field_name: str) -> Optional[str]:
    current = root
    for seg in rel_posix.split("/"):
        current = current / seg
        try:
            st = os.lstat(current)
        except FileNotFoundError:
            return f"{field_name} file '{rel_posix}' does not exist"
        except OSError as exc:
            return f"failed to inspect {field_name} path '{rel_posix}': {exc}"
        if os.path.islink(current):
            return f"{field_name} path '{rel_posix}' must not contain or point to a symlink"
        if seg != rel_posix.split("/")[-1]:
            if not Path(current).is_dir():
                return f"{field_name} path '{rel_posix}' has non-directory parent '{seg}'"
        else:
            import stat

            if not stat.S_ISREG(st.st_mode):
                return f"{field_name} path '{rel_posix}' must be a regular file"
    return None


def strip_js_comments(source: str) -> str:
    """Remove // and /* */ comments while preserving string/template literals."""
    out: List[str] = []
    i = 0
    n = len(source)
    state = "code"  # code, sq, dq, tpl, line_comment, block_comment
    while i < n:
        ch = source[i]
        nxt = source[i + 1] if i + 1 < n else ""
        if state == "code":
            if ch == "/" and nxt == "/":
                state = "line_comment"
                i += 2
                continue
            if ch == "/" and nxt == "*":
                state = "block_comment"
                i += 2
                continue
            if ch == "'":
                state = "sq"
                out.append(ch)
            elif ch == '"':
                state = "dq"
                out.append(ch)
            elif ch == "`":
                state = "tpl"
                out.append(ch)
            else:
                out.append(ch)
            i += 1
        elif state == "line_comment":
            if ch == "\n":
                state = "code"
                out.append("\n")
            i += 1
        elif state == "block_comment":
            if ch == "*" and nxt == "/":
                state = "code"
                i += 2
            else:
                if ch == "\n":
                    out.append("\n")
                i += 1
        elif state == "sq":
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(nxt)
                i += 2
                continue
            if ch == "'":
                state = "code"
            i += 1
        elif state == "dq":
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(nxt)
                i += 2
                continue
            if ch == '"':
                state = "code"
            i += 1
        elif state == "tpl":
            out.append(ch)
            if ch == "\\" and i + 1 < n:
                out.append(nxt)
                i += 2
                continue
            if ch == "`":
                state = "code"
            i += 1
    return "".join(out)


def check_js_syntax(node_bin: str, js_code: str, label: str, is_module: bool = False) -> Optional[str]:
    suffix = ".mjs" if is_module else ".js"
    tmp_path: Optional[str] = None
    try:
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", suffix=suffix, delete=False
        ) as handle:
            handle.write(js_code)
            tmp_path = handle.name
        proc = subprocess.run(
            [node_bin, "--check", tmp_path],
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode != 0:
            err_lines = (proc.stderr or proc.stdout or "syntax error").strip().splitlines()
            snippet = " | ".join(err_lines[:4])
            return f"JS syntax error in {label}: {snippet}"
        return None
    finally:
        if tmp_path:
            try:
                os.unlink(tmp_path)
            except OSError:
                pass


def iter_clean_files(root: Path) -> Tuple[List[Tuple[str, Path, int]], List[str]]:
    """Walk root without following symlinks; returns ([(rel_posix, full_path, size)], errors)."""
    files: List[Tuple[str, Path, int]] = []
    errors: List[str] = []
    seen_rel: set[str] = set()

    def walk_dir(current: Path, rel_parts: List[str]) -> None:
        try:
            entries = sorted(os.scandir(current), key=lambda e: e.name)
        except OSError as exc:
            errors.append(f"failed to list directory '{current}': {exc}")
            return

        for entry in entries:
            name = entry.name
            if is_ignored_name(name):
                continue
            if not rel_parts and name.startswith("."):
                # Top-level hidden files/dirs are reserved/ignored by ToolStow
                continue

            rel_posix = "/".join(rel_parts + [name])
            full_path = Path(entry.path)
            if entry.is_symlink():
                errors.append(f"symlink is not allowed: '{rel_posix}'")
                continue
            if entry.is_dir(follow_symlinks=False):
                walk_dir(full_path, rel_parts + [name])
            elif entry.is_file(follow_symlinks=False):
                _, path_err = validate_safe_rel_path(rel_posix)
                if path_err:
                    errors.append(f"unsafe file path '{rel_posix}': {path_err}")
                    continue
                if rel_posix in seen_rel:
                    errors.append(f"duplicate normalized path '{rel_posix}'")
                    continue
                seen_rel.add(rel_posix)
                try:
                    size = entry.stat(follow_symlinks=False).st_size
                except OSError as exc:
                    errors.append(f"failed to stat '{rel_posix}': {exc}")
                    continue
                files.append((rel_posix, full_path, size))
            else:
                errors.append(f"unsupported special file type: '{rel_posix}'")

    walk_dir(root, [])
    return files, errors


def _split_top_level_selectors(selector_list: str) -> List[str]:
    parts: List[str] = []
    start = 0
    paren_depth = 0
    bracket_depth = 0
    for idx, ch in enumerate(selector_list):
        if ch == "(":
            paren_depth += 1
        elif ch == ")" and paren_depth > 0:
            paren_depth -= 1
        elif ch == "[":
            bracket_depth += 1
        elif ch == "]" and bracket_depth > 0:
            bracket_depth -= 1
        elif ch == "," and paren_depth == 0 and bracket_depth == 0:
            parts.append(selector_list[start:idx])
            start = idx + 1
    parts.append(selector_list[start:])
    return parts


def _strip_has_pseudo(subject: str) -> str:
    out: List[str] = []
    i = 0
    n = len(subject)
    lower = subject.lower()
    while i < n:
        if lower.startswith(":has(", i):
            depth = 1
            j = i + 5
            while j < n and depth > 0:
                if subject[j] == "(":
                    depth += 1
                elif subject[j] == ")":
                    depth -= 1
                j += 1
            i = j
        else:
            out.append(subject[i])
            i += 1
    return "".join(out)


def _selector_targets_workbench_slot(selector_list: str) -> bool:
    """Return True if any comma-separated selector targets a workbench/slot as its subject."""
    for raw_sel in _split_top_level_selectors(selector_list):
        sel = raw_sel.strip()
        if not sel:
            continue
        paren_depth = 0
        bracket_depth = 0
        last_combinator = -1
        for idx, ch in enumerate(sel):
            if ch == "(":
                paren_depth += 1
            elif ch == ")" and paren_depth > 0:
                paren_depth -= 1
            elif ch == "[":
                bracket_depth += 1
            elif ch == "]" and bracket_depth > 0:
                bracket_depth -= 1
            elif paren_depth == 0 and bracket_depth == 0 and ch in " \t\n\r>+~":
                last_combinator = idx
        subject = _strip_has_pseudo(sel[last_combinator + 1 :])
        if WB_SLOT_SELECTOR_RE.search(subject):
            return True
    return False


def _scan_css_workbench_overrides(
    css_src: str,
    label: str,
    result: ValidationResult,
) -> None:
    stripped_css = CSS_COMMENT_RE.sub("", css_src)
    for sel_match in CSS_RULE_RE.finditer(stripped_css):
        selector = " ".join(sel_match.group(1).split())
        body = sel_match.group(2)
        if _selector_targets_workbench_slot(selector) and WB_LAYOUT_OVERRIDE_RE.search(body):
            result.warnings.append(
                f"{label}: layout positioning rule on '{selector}' "
                "(let themes control .ts-workbench and .ts-wb-* slot placement via ui.css)"
            )


def _scan_js_hygiene(
    js_code: str,
    label: str,
    result: ValidationResult,
    node_bin: Optional[str],
    is_module: bool = False,
) -> None:
    stripped = strip_js_comments(js_code)

    for desc, pattern in DANGEROUS_DOM_PATTERNS:
        if pattern.search(stripped):
            result.errors.append(
                f"{label}: disallowed {desc} (use textContent / DOM builders instead)"
            )

    if ALLOW_SAME_ORIGIN_RE.search(stripped):
        result.errors.append(f"{label}: references disallowed 'allow-same-origin'")

    if STORAGE_RE.search(stripped):
        if TRY_RE.search(stripped):
            result.warnings.append(
                f"{label}: references localStorage/sessionStorage/document.cookie "
                "(throws SecurityError in opaque sandbox unless guarded; prefer in-memory state)"
            )
        else:
            result.errors.append(
                f"{label}: unguarded localStorage/sessionStorage/document.cookie "
                "(throws SecurityError inside ToolStow's opaque sandbox)"
            )

    if node_bin:
        result.js_checked_count += 1
        syntax_err = check_js_syntax(node_bin, js_code, label, is_module=is_module)
        if syntax_err:
            result.errors.append(syntax_err)


def validate_tool_dir(
    tool_dir: Path,
    *,
    allow_standalone: bool = False,
    skip_js_check: bool = False,
    strict_dir_name: bool = False,
) -> ValidationResult:
    result = ValidationResult(tool_dir=tool_dir)

    if not tool_dir.exists():
        result.errors.append(f"tool directory does not exist: {tool_dir}")
        return result
    if tool_dir.is_symlink() or not tool_dir.is_dir():
        result.errors.append(
            f"tool path must be a regular directory (symlinks are not allowed): {tool_dir}"
        )
        return result

    manifest_path = tool_dir / "manifest.json"
    if not manifest_path.exists():
        result.errors.append("missing manifest.json in tool directory")
        return result
    if manifest_path.is_symlink() or not manifest_path.is_file():
        result.errors.append("manifest.json must be a regular file (symlinks are not allowed)")
        return result

    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        result.errors.append(f"invalid manifest.json: {exc}")
        return result

    if not isinstance(raw, dict):
        result.errors.append("manifest.json root must be a JSON object")
        return result

    # Required: id
    raw_id = raw.get("id")
    if not isinstance(raw_id, str):
        result.errors.append("manifest 'id' is required and must be a string")
    else:
        id_err = validate_id(raw_id)
        if id_err:
            result.errors.append(id_err)
        else:
            result.tool_id = raw_id
            resolved_name = tool_dir.resolve().name
            if resolved_name != raw_id:
                msg = (
                    f"manifest id '{raw_id}' does not match directory name '{resolved_name}' "
                    "(on-disk tools under data/tools/ require an exact match)"
                )
                if strict_dir_name:
                    result.errors.append(msg)
                else:
                    result.warnings.append(msg)

    # Required: name
    raw_name = raw.get("name")
    if not isinstance(raw_name, str) or not raw_name.strip():
        result.errors.append("manifest 'name' is required and must be a non-empty string")
    else:
        result.name = raw_name.strip()

    # Optional string fields
    for key in ("version", "description", "category"):
        val = raw.get(key)
        if val is not None and not isinstance(val, str):
            result.errors.append(f"manifest '{key}' must be a string")
    if isinstance(raw.get("version"), str) and raw["version"].strip():
        result.version = raw["version"].strip()

    # Optional tags
    raw_tags = raw.get("tags")
    if raw_tags is not None:
        if not isinstance(raw_tags, list) or not all(isinstance(t, str) for t in raw_tags):
            result.errors.append("manifest 'tags' must be an array of strings")

    # Optional booleans
    raw_draft = raw.get("draft")
    if raw_draft is not None:
        if not isinstance(raw_draft, bool):
            result.errors.append("manifest 'draft' must be a boolean")
        else:
            result.draft = raw_draft

    raw_theme_aware = raw.get("themeAware")
    if raw_theme_aware is not None and not isinstance(raw_theme_aware, bool):
        result.errors.append("manifest 'themeAware' must be a boolean")

    # Runtime
    raw_runtime = raw.get("runtime")
    if raw_runtime is not None:
        if not isinstance(raw_runtime, str):
            result.errors.append("manifest 'runtime' must be a string")
        else:
            rt = raw_runtime.strip() or "static"
            if rt == "script":
                result.errors.append(
                    "runtime 'script' is not supported (only runtime 'static' is supported)"
                )
            elif rt != "static":
                result.errors.append(f"unsupported runtime '{rt}': expected 'static'")

    # Entry file
    raw_entry = raw.get("entry")
    if raw_entry is None or (isinstance(raw_entry, str) and not raw_entry.strip()):
        entry_raw = "index.html"
    elif isinstance(raw_entry, str):
        entry_raw = raw_entry.strip()
    else:
        result.errors.append("manifest 'entry' must be a string")
        entry_raw = ""

    entry_rel: Optional[str] = None
    if entry_raw:
        entry_rel, entry_path_err = validate_safe_rel_path(entry_raw)
        if entry_path_err:
            result.errors.append(f"manifest 'entry': {entry_path_err}")
        elif entry_rel:
            entry_fs_err = ensure_regular_file_within(tool_dir, entry_rel, "entry")
            if entry_fs_err:
                result.errors.append(entry_fs_err)
            else:
                result.entry = entry_rel

    # Icon
    raw_icon = raw.get("icon")
    if raw_icon is not None:
        if not isinstance(raw_icon, str):
            result.errors.append("manifest 'icon' must be a string")
        else:
            icon_trimmed = raw_icon.strip()
            if icon_trimmed and is_path_like_icon(icon_trimmed):
                icon_rel, icon_path_err = validate_safe_rel_path(icon_trimmed)
                if icon_path_err:
                    result.errors.append(f"manifest 'icon': {icon_path_err}")
                elif icon_rel:
                    icon_fs_err = ensure_regular_file_within(tool_dir, icon_rel, "icon")
                    if icon_fs_err:
                        result.errors.append(icon_fs_err)

    # Sandbox tokens
    raw_sandbox = raw.get("sandbox")
    if raw_sandbox is not None:
        if not isinstance(raw_sandbox, list) or not all(isinstance(t, str) for t in raw_sandbox):
            result.errors.append("manifest 'sandbox' must be an array of strings")
        else:
            for token in raw_sandbox:
                t = token.strip()
                if not t:
                    continue
                if any(t.startswith(prefix) for prefix in FORBIDDEN_SANDBOX_PREFIXES):
                    result.errors.append(
                        f"manifest 'sandbox' includes forbidden token '{t}' "
                        "(stripped and blocked by ToolStow)"
                    )
                elif t not in SANDBOX_ALLOWLIST:
                    result.warnings.append(
                        f"manifest 'sandbox' token '{t}' is not in ToolStow's allowlist "
                        "and will be stripped"
                    )

    # Walk directory for symlinks, file limits, and source files
    clean_files, walk_errors = iter_clean_files(tool_dir)
    result.errors.extend(walk_errors)
    result.file_count = len(clean_files)
    result.total_bytes = sum(size for _, _, size in clean_files)

    if result.file_count == 0:
        result.errors.append("tool directory contains no packable files")
    if result.file_count > MAX_FILES:
        result.errors.append(
            f"too many files: {result.file_count} exceeds ToolStow limit of {MAX_FILES}"
        )
    if result.total_bytes > MAX_UNCOMPRESSED_BYTES:
        result.errors.append(
            f"uncompressed size {result.total_bytes} bytes exceeds 200 MiB limit"
        )
    elif result.total_bytes > DEFAULT_MAX_ZIP_BYTES:
        result.warnings.append(
            f"uncompressed size ({result.total_bytes / (1024 * 1024):.1f} MiB) exceeds "
            "default 50 MiB upload limit (compressed ZIP may still fit)"
        )

    node_bin = None if skip_js_check else shutil.which("node")
    if skip_js_check:
        result.js_check_status = "JS syntax skipped"
    elif node_bin is None:
        result.js_check_status = "node not found"
    else:
        result.js_check_status = "node"

    # Inspect entry HTML and any JS files
    if entry_rel and (tool_dir / entry_rel).is_file():
        try:
            html_text = (tool_dir / entry_rel).read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            result.errors.append(f"failed to read entry HTML '{entry_rel}' as UTF-8: {exc}")
            html_text = ""

        if html_text:
            inspector = _HtmlInspector()
            try:
                inspector.feed(html_text)
                inspector.close()
            except Exception as exc:  # pragma: no cover - HTMLParser rarely raises
                result.warnings.append(f"HTML parser warning on '{entry_rel}': {exc}")

            if any(ALLOW_SAME_ORIGIN_RE.search(val) for val in inspector.iframe_sandbox_attrs):
                result.errors.append(
                    f"{entry_rel}: <iframe sandbox> uses disallowed 'allow-same-origin'"
                )
            if any(ALLOW_SAME_ORIGIN_RE.search(val) for val in inspector.meta_csp_contents):
                result.errors.append(
                    f"{entry_rel}: CSP <meta> uses disallowed 'allow-same-origin'"
                )

            has_css = any(
                (attrs.get("href") or "").strip().split("?", 1)[0] == "/ui/v1/toolstow.css"
                for attrs in inspector.stylesheets
            )
            theme_js_tags = [
                attrs
                for attrs in inspector.external_scripts
                if (attrs.get("src") or "").strip().split("?", 1)[0] == "/ui/v1/theme.js"
            ]
            has_theme_js = bool(theme_js_tags)

            if not allow_standalone:
                if not has_css:
                    result.errors.append(
                        f"{entry_rel}: missing <link rel=\"stylesheet\" href=\"/ui/v1/toolstow.css\"> "
                        "(pass --allow-standalone for third-party non-SDK imports)"
                    )
                if not has_theme_js:
                    result.errors.append(
                        f"{entry_rel}: missing <script src=\"/ui/v1/theme.js\"></script> "
                        "(pass --allow-standalone for third-party non-SDK imports)"
                    )
                else:
                    for attrs in theme_js_tags:
                        if "defer" in attrs or "async" in attrs:
                            result.warnings.append(
                                f"{entry_rel}: /ui/v1/theme.js should load synchronously in <head> "
                                "without defer/async to prevent dark-mode FOUC"
                            )
                if raw_theme_aware is not True:
                    result.warnings.append(
                        "manifest 'themeAware' is not set to true "
                        "(recommended for plugins using /ui/v1/theme.js)"
                    )

            # External CDN warnings
            for attrs in inspector.stylesheets:
                href = (attrs.get("href") or "").strip()
                if href.startswith(("http://", "https://", "//")):
                    result.warnings.append(
                        f"{entry_rel}: external stylesheet '{href}' breaks offline/self-contained guarantee"
                    )
            for attrs in inspector.external_scripts:
                src = (attrs.get("src") or "").strip()
                if src.startswith(("http://", "https://", "//")):
                    result.warnings.append(
                        f"{entry_rel}: external script '{src}' breaks offline/self-contained guarantee"
                    )

            # Inline <style> blocks: warn if tool hardcodes layout positioning on workbench slots
            for idx, style_src in enumerate(inspector.inline_styles, start=1):
                _scan_css_workbench_overrides(style_src, f"{entry_rel}#style[{idx}]", result)

            # Inline <script> blocks
            for idx, (script_type, script_src) in enumerate(inspector.inline_scripts, start=1):
                if script_type in ("", "text/javascript", "application/javascript", "module"):
                    _scan_js_hygiene(
                        script_src,
                        f"{entry_rel}#script[{idx}]",
                        result,
                        node_bin,
                        is_module=(script_type == "module"),
                    )

    # Check all standalone .js / .mjs / .css files
    for rel_posix, full_path, _ in clean_files:
        lower = rel_posix.lower()
        if lower.endswith((".js", ".mjs")):
            try:
                js_text = full_path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError) as exc:
                result.errors.append(f"failed to read '{rel_posix}' as UTF-8: {exc}")
                continue
            _scan_js_hygiene(
                js_text,
                rel_posix,
                result,
                node_bin,
                is_module=lower.endswith(".mjs"),
            )
        elif lower.endswith(".css"):
            try:
                css_text = full_path.read_text(encoding="utf-8")
            except (OSError, UnicodeDecodeError) as exc:
                result.errors.append(f"failed to read '{rel_posix}' as UTF-8: {exc}")
                continue
            _scan_css_workbench_overrides(css_text, rel_posix, result)

    return result


def build_tool_zip_bytes(
    tool_dir: Path,
    *,
    max_zip_bytes: int = DEFAULT_MAX_ZIP_BYTES,
) -> Tuple[bytes, int]:
    """Pack a validated tool directory into a clean flat ZIP in memory."""
    clean_files, walk_errors = iter_clean_files(tool_dir)
    if walk_errors:
        raise ValueError("; ".join(walk_errors))

    buf = io.BytesIO()
    with zipfile.ZipFile(buf, mode="w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for rel_posix, full_path, _ in clean_files:
            info = zipfile.ZipInfo(rel_posix)
            info.compress_type = zipfile.ZIP_DEFLATED
            # Standard regular file permissions (0644)
            info.external_attr = 0o100644 << 16
            zf.writestr(info, full_path.read_bytes())

    data = buf.getvalue()
    if len(data) > max_zip_bytes:
        raise ValueError(
            f"ZIP archive size ({len(data) / (1024 * 1024):.2f} MiB) exceeds limit "
            f"({max_zip_bytes / (1024 * 1024):.1f} MiB)"
        )
    return data, len(clean_files)


def print_result(res: ValidationResult, *, strict: bool = False) -> bool:
    passed = res.ok and (not strict or not res.warnings)
    status = "OK" if passed else "FAIL"
    label = res.tool_id or res.tool_dir.name
    if res.js_check_status == "node":
        js_note = f"JS checked via node ({res.js_checked_count})"
    else:
        js_note = res.js_check_status
    print(
        f"[{status}] tool '{label}' ({res.tool_dir}) — "
        f"{res.file_count} files, {res.total_bytes:,} bytes | "
        f"{len(res.errors)} errors, {len(res.warnings)} warnings, {js_note}"
    )
    for warn in res.warnings:
        print(f"  WARN:  {warn}")
    for err in res.errors:
        print(f"  ERROR: {err}")
    return passed


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate one or more ToolStow static micro-tool directories."
    )
    parser.add_argument(
        "tool_dirs",
        nargs="+",
        type=Path,
        help="Path(s) to tool directory containing manifest.json",
    )
    parser.add_argument(
        "--allow-standalone",
        action="store_true",
        help="Do not require /ui/v1/toolstow.css and /ui/v1/theme.js (for third-party imports)",
    )
    parser.add_argument(
        "--strict-dir-name",
        action="store_true",
        help="Treat directory name != manifest.id as an error instead of a warning",
    )
    parser.add_argument(
        "--skip-js-check",
        action="store_true",
        help="Skip optional 'node --check' JS syntax verification",
    )
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Fail if any warnings are emitted",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        dest="as_json",
        help="Output machine-readable JSON",
    )
    args = parser.parse_args(argv)

    results = [
        validate_tool_dir(
            d,
            allow_standalone=args.allow_standalone,
            skip_js_check=args.skip_js_check,
            strict_dir_name=args.strict_dir_name,
        )
        for d in args.tool_dirs
    ]

    if args.as_json:
        payload = [r.to_dict() for r in results]
        print(json.dumps(payload[0] if len(payload) == 1 else payload, ensure_ascii=False, indent=2))
        all_ok = all(r.ok and (not args.strict or not r.warnings) for r in results)
        return 0 if all_ok else 1

    all_ok = True
    for res in results:
        if not print_result(res, strict=args.strict):
            all_ok = False
    return 0 if all_ok else 1


if __name__ == "__main__":
    sys.exit(main())

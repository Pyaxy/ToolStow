#!/usr/bin/env python3
"""Validate a ToolStow theme directory (skin-only or full custom shell).

Zero-dependency Python 3.9+ validator aligned with ToolStow's Rust backend
(src/themes.rs and src/archive.rs) plus SVG and full-shell hygiene checks.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
import zipfile
from dataclasses import dataclass, field
from html.parser import HTMLParser
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

BUILTIN_DEFAULT_THEME = "default"
MAX_SHORT_LEN = 64
MAX_FILES = 1000
MAX_UNCOMPRESSED_BYTES = 200 * 1024 * 1024  # 200 MiB
DEFAULT_MAX_ZIP_BYTES = 50 * 1024 * 1024  # 50 MiB

SHORT_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
IGNORED_NAMES = frozenset({".DS_Store", "__MACOSX", ".git", ".gitignore", "Thumbs.db"})

DANGEROUS_DOM_PATTERNS: Tuple[Tuple[str, re.Pattern[str]], ...] = (
    ("innerHTML assignment", re.compile(r"\.innerHTML\s*(?:=|\+=)")),
    ("outerHTML assignment", re.compile(r"\.outerHTML\s*(?:=|\+=)")),
    ("insertAdjacentHTML call", re.compile(r"\.insertAdjacentHTML\s*\(")),
    ("document.write call", re.compile(r"\bdocument\.write(?:ln)?\s*\(")),
)

ALLOW_SAME_ORIGIN_RE = re.compile(r"\ballow-same-origin\b", re.IGNORECASE)
ALLOW_TOP_NAV_RE = re.compile(r"\ballow-top-navigation(?:-[a-z-]+)?\b", re.IGNORECASE)
INLINE_ACCENT_OVERRIDE_RE = re.compile(
    r"""\.setProperty\s*\(\s*["']--ts-accent["']"""
)
JAVASCRIPT_SCHEME_RE = re.compile(r"^\s*javascript:", re.IGNORECASE)
EXTERNAL_URI_RE = re.compile(r"^\s*(?:https?:)?//", re.IGNORECASE)
CSS_EXTERNAL_URL_RE = re.compile(r"""url\s*\(\s*["']?\s*(?:https?:)?//""", re.IGNORECASE)
ES_MODULE_SYNTAX_RE = re.compile(r"(?m)^\s*(?:import|export)\b")


@dataclass
class ThemeValidationResult:
    theme_dir: Path
    short: Optional[str] = None
    name: Optional[str] = None
    version: str = "0.1.0"
    has_ui_css: bool = False
    has_custom_shell: bool = False
    shell_entry: Optional[str] = None
    file_count: int = 0
    total_bytes: int = 0
    svg_checked_count: int = 0
    js_checked_count: int = 0
    js_check_status: str = "no custom shell"
    errors: List[str] = field(default_factory=list)
    warnings: List[str] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.errors

    def to_dict(self) -> Dict[str, Any]:
        return {
            "themeDir": str(self.theme_dir),
            "short": self.short,
            "name": self.name,
            "version": self.version,
            "hasUiCss": self.has_ui_css,
            "hasCustomShell": self.has_custom_shell,
            "shellEntry": self.shell_entry,
            "fileCount": self.file_count,
            "totalBytes": self.total_bytes,
            "svgCheckedCount": self.svg_checked_count,
            "jsCheckedCount": self.js_checked_count,
            "jsCheckStatus": self.js_check_status,
            "ok": self.ok,
            "errors": self.errors,
            "warnings": self.warnings,
        }


class _HtmlInspector(HTMLParser):
    def __init__(self) -> None:
        super().__init__(convert_charrefs=False)
        self.stylesheets: List[Dict[str, Optional[str]]] = []
        self.external_scripts: List[Dict[str, Optional[str]]] = []
        self.inline_scripts: List[Tuple[str, str]] = []
        self._in_script = False
        self._script_type = ""
        self._script_has_src = False
        self._script_chunks: List[str] = []

    def handle_starttag(self, tag: str, attrs: List[Tuple[str, Optional[str]]]) -> None:
        tag_l = tag.lower()
        attr_map: Dict[str, Optional[str]] = {k.lower(): v for k, v in attrs}
        if tag_l == "link":
            rel = (attr_map.get("rel") or "").lower().split()
            if "stylesheet" in rel:
                self.stylesheets.append(attr_map)
        elif tag_l == "script":
            self._in_script = True
            self._script_type = (attr_map.get("type") or "").strip().lower()
            src = attr_map.get("src")
            self._script_has_src = src is not None and src.strip() != ""
            self._script_chunks = []
            if self._script_has_src:
                self.external_scripts.append(attr_map)

    def handle_data(self, data: str) -> None:
        if self._in_script and not self._script_has_src:
            self._script_chunks.append(data)

    def handle_endtag(self, tag: str) -> None:
        if tag.lower() == "script" and self._in_script:
            if not self._script_has_src:
                src_text = "".join(self._script_chunks)
                if src_text.strip():
                    self.inline_scripts.append((self._script_type, src_text))
            self._in_script = False
            self._script_type = ""
            self._script_has_src = False
            self._script_chunks = []


def is_ignored_name(name: str) -> bool:
    return name in IGNORED_NAMES or name.startswith("._")


def validate_theme_short(short: str) -> Optional[str]:
    if short == BUILTIN_DEFAULT_THEME:
        return "theme short 'default' is reserved for the built-in theme"
    if not isinstance(short, str) or not short or len(short) > MAX_SHORT_LEN:
        return (
            f"invalid theme short {short!r}: length must be between 1 and "
            f"{MAX_SHORT_LEN} characters"
        )
    if not SHORT_RE.match(short):
        return (
            f"invalid theme short {short!r}: must match ^[a-z0-9]+(?:-[a-z0-9]+)*$"
        )
    return None


def validate_safe_rel_path(path_str: str) -> Tuple[Optional[str], Optional[str]]:
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


def ensure_regular_file_within(root: Path, rel_posix: str, field_name: str) -> Optional[str]:
    parts = rel_posix.split("/")
    current = root
    for idx, seg in enumerate(parts):
        current = current / seg
        try:
            st = os.lstat(current)
        except FileNotFoundError:
            return f"{field_name} file '{rel_posix}' does not exist"
        except OSError as exc:
            return f"failed to inspect {field_name} path '{rel_posix}': {exc}"
        if os.path.islink(current):
            return f"{field_name} path '{rel_posix}' must not contain or point to a symlink"
        if idx < len(parts) - 1:
            if not stat.S_ISDIR(st.st_mode):
                return f"{field_name} path '{rel_posix}' has non-directory parent '{seg}'"
        else:
            if not stat.S_ISREG(st.st_mode):
                return f"{field_name} path '{rel_posix}' must be a regular file"
    return None


def validate_theme_url(raw_url: str) -> Optional[str]:
    trimmed = raw_url.strip()
    if not trimmed:
        return None
    if any(ch.isspace() or ord(ch) < 0x20 or ord(ch) == 0x7F for ch in trimmed):
        return f"invalid theme url {trimmed!r}: must not contain whitespace or control characters"
    if trimmed.startswith("https://"):
        rest = trimmed[len("https://"):]
    elif trimmed.startswith("http://"):
        rest = trimmed[len("http://"):]
    else:
        return f"invalid theme url {trimmed!r}: must start with 'http://' or 'https://'"
    if not rest:
        return f"invalid theme url {trimmed!r}: must include a host after the scheme"
    return None


def strip_js_comments(source: str) -> str:
    out: List[str] = []
    i = 0
    n = len(source)
    state = "code"
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


def validate_svg_file(svg_path: Path, rel_posix: str) -> Tuple[List[str], List[str]]:
    errors: List[str] = []
    warnings: List[str] = []
    try:
        raw_text = svg_path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as exc:
        return [f"failed to read SVG '{rel_posix}' as UTF-8: {exc}"], warnings

    try:
        root = ET.fromstring(raw_text)
    except ET.ParseError as exc:
        return [f"invalid XML in SVG '{rel_posix}': {exc}"], warnings

    def local_name(tag: str) -> str:
        if "}" in tag:
            return tag.rsplit("}", 1)[1].lower()
        return tag.lower()

    if local_name(root.tag) != "svg":
        errors.append(f"SVG '{rel_posix}' root element must be <svg>, found <{root.tag}>")

    saw_external_ref = False
    for elem in root.iter():
        lname = local_name(elem.tag)
        if lname == "script":
            errors.append(f"SVG '{rel_posix}' contains disallowed <script> element")
        if lname == "style" and elem.text and CSS_EXTERNAL_URL_RE.search(elem.text):
            saw_external_ref = True
        for attr_name, attr_val in elem.attrib.items():
            attr_local = local_name(attr_name)
            if attr_local.startswith("on"):
                errors.append(
                    f"SVG '{rel_posix}' contains disallowed inline event attribute '{attr_local}'"
                )
            if isinstance(attr_val, str) and JAVASCRIPT_SCHEME_RE.match(attr_val):
                errors.append(
                    f"SVG '{rel_posix}' contains disallowed 'javascript:' URI in '{attr_local}'"
                )
            if isinstance(attr_val, str):
                if attr_local in ("href", "src") and EXTERNAL_URI_RE.match(attr_val):
                    saw_external_ref = True
                elif attr_local == "style" and CSS_EXTERNAL_URL_RE.search(attr_val):
                    saw_external_ref = True

    if saw_external_ref:
        warnings.append(
            f"SVG '{rel_posix}' references external URL(s); SVGs are served under "
            "CSP sandbox so keep them self-contained"
        )

    return errors, warnings


def iter_clean_files(root: Path) -> Tuple[List[Tuple[str, Path, int]], List[str]]:
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
                continue

            rel_posix = "/".join(rel_parts + [name])
            full_path = Path(entry.path)
            if entry.is_symlink():
                errors.append(f"symlink is not allowed: '{rel_posix}'")
                continue
            if entry.is_dir(follow_symlinks=False):
                if name == "node_modules":
                    errors.append(
                        f"disallowed directory '{rel_posix}': never bundle node_modules/ "
                        "inside a theme package (output built static files into dist/ instead)"
                    )
                    continue
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


def _scan_shell_js(
    js_code: str,
    label: str,
    result: ThemeValidationResult,
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

    if ALLOW_TOP_NAV_RE.search(stripped):
        result.errors.append(f"{label}: references disallowed 'allow-top-navigation*'")

    if INLINE_ACCENT_OVERRIDE_RE.search(stripped):
        result.errors.append(
            f"{label}: inline setProperty('--ts-accent', ...) is forbidden "
            "(let ui.css own --ts-accent for light and dark modes)"
        )

    if node_bin:
        result.js_checked_count += 1
        syntax_err = check_js_syntax(node_bin, js_code, label, is_module=is_module)
        if syntax_err:
            result.errors.append(syntax_err)


def validate_theme_dir(
    theme_dir: Path,
    *,
    skip_js_check: bool = False,
    strict_dir_name: bool = False,
) -> ThemeValidationResult:
    result = ThemeValidationResult(theme_dir=theme_dir)

    if not theme_dir.exists():
        result.errors.append(f"theme directory does not exist: {theme_dir}")
        return result
    if theme_dir.is_symlink() or not theme_dir.is_dir():
        result.errors.append(
            f"theme path must be a regular directory (symlinks are not allowed): {theme_dir}"
        )
        return result

    manifest_path = theme_dir / "theme.json"
    if not manifest_path.exists():
        result.errors.append("missing theme.json in theme directory")
        return result
    if manifest_path.is_symlink() or not manifest_path.is_file():
        result.errors.append("theme.json must be a regular file (symlinks are not allowed)")
        return result

    try:
        raw = json.loads(manifest_path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as exc:
        result.errors.append(f"invalid theme.json: {exc}")
        return result

    if not isinstance(raw, dict):
        result.errors.append("theme.json root must be a JSON object")
        return result

    # Required: short
    raw_short = raw.get("short")
    if not isinstance(raw_short, str):
        result.errors.append("theme 'short' is required and must be a string")
    else:
        short_err = validate_theme_short(raw_short)
        if short_err:
            result.errors.append(short_err)
        else:
            result.short = raw_short
            resolved_name = theme_dir.resolve().name
            if resolved_name != raw_short:
                msg = (
                    f"theme short '{raw_short}' does not match directory name '{resolved_name}' "
                    "(on-disk themes under data/themes/ require an exact match)"
                )
                if strict_dir_name:
                    result.errors.append(msg)
                else:
                    result.warnings.append(msg)

    # Required: name
    raw_name = raw.get("name")
    if not isinstance(raw_name, str) or not raw_name.strip():
        result.errors.append("theme 'name' is required and must be a non-empty string")
    else:
        result.name = raw_name.strip()

    # Optional strings
    for key in ("version", "description", "author"):
        val = raw.get(key)
        if val is not None and not isinstance(val, str):
            result.errors.append(f"theme '{key}' must be a string")
    if isinstance(raw.get("version"), str) and raw["version"].strip():
        result.version = raw["version"].strip()

    # Optional URL
    raw_url = raw.get("url")
    if raw_url is not None:
        if not isinstance(raw_url, str):
            result.errors.append("theme 'url' must be a string")
        else:
            url_err = validate_theme_url(raw_url)
            if url_err:
                result.errors.append(url_err)

    # Optional preview
    raw_preview = raw.get("preview")
    if raw_preview is not None:
        if not isinstance(raw_preview, str):
            result.errors.append("theme 'preview' must be a string")
        elif raw_preview.strip():
            prev_rel, prev_path_err = validate_safe_rel_path(raw_preview.strip())
            if prev_path_err:
                result.errors.append(f"theme 'preview': {prev_path_err}")
            elif prev_rel:
                prev_fs_err = ensure_regular_file_within(theme_dir, prev_rel, "preview")
                if prev_fs_err:
                    result.errors.append(prev_fs_err)

    # Detect web root (dist/index.html > index.html) and ui.css
    dist_index = theme_dir / "dist" / "index.html"
    root_index = theme_dir / "index.html"
    ui_css = theme_dir / "ui.css"

    if dist_index.exists() or dist_index.is_symlink():
        err = ensure_regular_file_within(theme_dir, "dist/index.html", "shell entry")
        if err:
            result.errors.append(err)
        else:
            result.has_custom_shell = True
            result.shell_entry = "dist/index.html"
    elif root_index.exists() or root_index.is_symlink():
        err = ensure_regular_file_within(theme_dir, "index.html", "shell entry")
        if err:
            result.errors.append(err)
        else:
            result.has_custom_shell = True
            result.shell_entry = "index.html"

    if ui_css.exists() or ui_css.is_symlink():
        err = ensure_regular_file_within(theme_dir, "ui.css", "ui.css")
        if err:
            result.errors.append(err)
        else:
            result.has_ui_css = True

    if not result.has_custom_shell and not result.has_ui_css:
        result.errors.append(
            "theme must provide at least one of 'dist/index.html', 'index.html', or 'ui.css'"
        )

    # Walk files & check archive limits
    clean_files, walk_errors = iter_clean_files(theme_dir)
    result.errors.extend(walk_errors)
    result.file_count = len(clean_files)
    result.total_bytes = sum(size for _, _, size in clean_files)

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
            "default 50 MiB upload limit"
        )

    # Validate every SVG file in the theme (preview.svg, assets/*.svg, etc.)
    for rel_posix, full_path, _ in clean_files:
        if rel_posix.lower().endswith(".svg"):
            result.svg_checked_count += 1
            svg_errors, svg_warnings = validate_svg_file(full_path, rel_posix)
            result.errors.extend(svg_errors)
            result.warnings.extend(svg_warnings)

    # Inspect ui.css
    if result.has_ui_css:
        try:
            css_text = ui_css.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            result.errors.append(f"failed to read ui.css as UTF-8: {exc}")
            css_text = ""
        if css_text:
            if ":root" not in css_text:
                result.warnings.append("ui.css does not define ':root' light-mode tokens")
            if 'data-theme="dark"' not in css_text and "data-theme='dark'" not in css_text:
                result.warnings.append(
                    "ui.css does not define ':root[data-theme=\"dark\"]' dark-mode tokens"
                )

    # Inspect full custom shell HTML + JS
    if result.has_custom_shell and result.shell_entry:
        node_bin = None if skip_js_check else shutil.which("node")
        if skip_js_check:
            result.js_check_status = "JS syntax skipped"
        elif node_bin is None:
            result.js_check_status = "node not found"
        else:
            result.js_check_status = "node"
        shell_path = theme_dir / result.shell_entry
        combined_shell_code: List[str] = []

        try:
            html_text = shell_path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as exc:
            result.errors.append(f"failed to read '{result.shell_entry}' as UTF-8: {exc}")
            html_text = ""

        if html_text:
            combined_shell_code.append(html_text)
            if ALLOW_SAME_ORIGIN_RE.search(html_text):
                result.errors.append(
                    f"{result.shell_entry}: references disallowed 'allow-same-origin'"
                )

            inspector = _HtmlInspector()
            try:
                inspector.feed(html_text)
                inspector.close()
            except Exception as exc:  # pragma: no cover
                result.warnings.append(f"HTML parser warning on '{result.shell_entry}': {exc}")

            hrefs = [
                (attrs.get("href") or "").strip().split("?", 1)[0]
                for attrs in inspector.stylesheets
            ]
            if "/ui/v1/toolstow.css" not in hrefs:
                result.errors.append(
                    f"{result.shell_entry}: missing <link rel=\"stylesheet\" href=\"/ui/v1/toolstow.css\">"
                )
            elif hrefs and hrefs[0] != "/ui/v1/toolstow.css":
                result.warnings.append(
                    f"{result.shell_entry}: '/ui/v1/toolstow.css' should be loaded FIRST "
                    "before local theme stylesheets"
                )

            module_script_paths: set[str] = set()
            has_module_scripts = False
            shell_rel_dir = Path(result.shell_entry).parent
            for attrs in inspector.external_scripts:
                if (attrs.get("type") or "").strip().lower() == "module":
                    has_module_scripts = True
                    raw_src = (attrs.get("src") or "").strip().split("?", 1)[0].split("#", 1)[0]
                    if raw_src and not raw_src.startswith(("http://", "https://", "//", "/")):
                        normalized_src = os.path.normpath((shell_rel_dir / raw_src).as_posix())
                        if not normalized_src.startswith(".."):
                            module_script_paths.add(normalized_src)
                    elif raw_src.startswith("/") and not raw_src.startswith("//"):
                        normalized_src = os.path.normpath(
                            (shell_rel_dir / raw_src.lstrip("/")).as_posix()
                        )
                        if not normalized_src.startswith(".."):
                            module_script_paths.add(normalized_src)

            for idx, (script_type, script_src) in enumerate(inspector.inline_scripts, start=1):
                if script_type in ("", "text/javascript", "application/javascript", "module"):
                    combined_shell_code.append(script_src)
                    _scan_shell_js(
                        script_src,
                        f"{result.shell_entry}#script[{idx}]",
                        result,
                        node_bin,
                        is_module=(script_type == "module"),
                    )
        else:
            module_script_paths = set()
            has_module_scripts = False

        # Scan JS files belonging to the shell
        for rel_posix, full_path, _ in clean_files:
            lower = rel_posix.lower()
            if lower.endswith((".js", ".mjs")):
                try:
                    js_text = full_path.read_text(encoding="utf-8")
                except (OSError, UnicodeDecodeError) as exc:
                    result.errors.append(f"failed to read '{rel_posix}' as UTF-8: {exc}")
                    continue
                combined_shell_code.append(js_text)
                stripped_js = strip_js_comments(js_text)
                is_esm = (
                    lower.endswith(".mjs")
                    or rel_posix in module_script_paths
                    or (has_module_scripts and bool(ES_MODULE_SYNTAX_RE.search(stripped_js)))
                )
                # For bundled SPA dist/ assets, still check JS syntax & allow-same-origin,
                # while full DOM hygiene runs on all authored root scripts (and dist scripts).
                _scan_shell_js(
                    js_text,
                    rel_posix,
                    result,
                    node_bin,
                    is_module=is_esm,
                )

        merged = "\n".join(combined_shell_code)
        if "/api/me" not in merged:
            result.warnings.append(
                "custom shell does not appear to fetch '/api/me' (needed for siteTitle & isAdmin)"
            )
        if "/api/tools" not in merged:
            result.warnings.append(
                "custom shell does not appear to fetch '/api/tools'"
            )
        if "isAdmin" not in merged:
            result.warnings.append(
                "custom shell does not reference 'isAdmin' "
                "(remember to show /admin link strictly when me.isAdmin === true)"
            )
        if "toolstow:theme" not in merged:
            result.warnings.append(
                "custom shell does not appear to send 'toolstow:theme' postMessage to plugin iframes"
            )
        if "toolstow:ready" not in merged:
            result.warnings.append(
                "custom shell does not appear to listen for 'toolstow:ready' from plugin iframes"
            )

    return result


def build_theme_zip_bytes(
    theme_dir: Path,
    *,
    max_zip_bytes: int = DEFAULT_MAX_ZIP_BYTES,
) -> Tuple[bytes, int]:
    clean_files, walk_errors = iter_clean_files(theme_dir)
    if walk_errors:
        raise ValueError("; ".join(walk_errors))

    buf = io.BytesIO()
    with zipfile.ZipFile(buf, mode="w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zf:
        for rel_posix, full_path, _ in clean_files:
            info = zipfile.ZipInfo(rel_posix)
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            zf.writestr(info, full_path.read_bytes())

    data = buf.getvalue()
    if len(data) > max_zip_bytes:
        raise ValueError(
            f"ZIP archive size ({len(data) / (1024 * 1024):.2f} MiB) exceeds limit "
            f"({max_zip_bytes / (1024 * 1024):.1f} MiB)"
        )
    return data, len(clean_files)


def print_result(res: ThemeValidationResult, *, strict: bool = False) -> bool:
    passed = res.ok and (not strict or not res.warnings)
    status = "OK" if passed else "FAIL"
    label = res.short or res.theme_dir.name
    mode = "full-shell" if res.has_custom_shell else ("skin-only" if res.has_ui_css else "unknown")
    details = [
        f"{len(res.errors)} errors",
        f"{len(res.warnings)} warnings",
        f"{res.svg_checked_count} SVG checked",
    ]
    if res.has_custom_shell:
        if res.js_check_status == "node":
            details.append(f"JS checked via node ({res.js_checked_count})")
        else:
            details.append(res.js_check_status)
    print(
        f"[{status}] theme '{label}' [{mode}] ({res.theme_dir}) — "
        f"{res.file_count} files, {res.total_bytes:,} bytes | {', '.join(details)}"
    )
    for warn in res.warnings:
        print(f"  WARN:  {warn}")
    for err in res.errors:
        print(f"  ERROR: {err}")
    return passed


def main(argv: Optional[List[str]] = None) -> int:
    parser = argparse.ArgumentParser(
        description="Validate one or more ToolStow theme directories."
    )
    parser.add_argument(
        "theme_dirs",
        nargs="+",
        type=Path,
        help="Path(s) to theme directory containing theme.json",
    )
    parser.add_argument(
        "--strict-dir-name",
        action="store_true",
        help="Treat directory name != theme.short as an error instead of a warning",
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
        validate_theme_dir(
            d,
            skip_js_check=args.skip_js_check,
            strict_dir_name=args.strict_dir_name,
        )
        for d in args.theme_dirs
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

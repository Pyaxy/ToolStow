# ToolStow AI Tool Development & Publishing Skill

> **Moved**: This skill is now packaged as a self-contained Agent Skill folder at **[`skills/toolstow-tool/`](../skills/toolstow-tool/)**:
>
> - Specification: **[`skills/toolstow-tool/SKILL.md`](../skills/toolstow-tool/SKILL.md)**
> - Companion scripts (zero-dependency Python 3.9+):
>   - `skills/toolstow-tool/scripts/validate.py` — validate `manifest.json`, sandbox/XSS hygiene, `/ui/v1/*` SDK usage, archive limits, and JS syntax
>   - `skills/toolstow-tool/scripts/pack.py` — validate and package a clean flat `.zip`
>   - `skills/toolstow-tool/scripts/publish.py` — validate, pack, and upload via `TOOLSTOW_API_KEY` (with `409` overwrite protection)
>
> You can copy the entire `skills/toolstow-tool/` folder directly into your AI coding agent's skills directory.

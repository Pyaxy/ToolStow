# ToolStow

> AI 原生、单二进制、零数据库、热插拔的个人微工具自托管宿主

---

## 项目介绍

写代码或和 AI 对话时，经常会随手生成各种实用小网页——命令速查表、格式转换器、图标生成器、日期计算器……它们通常散落在本地临时文件夹和聊天记录里，用完即丢。

**ToolStow** 不仅把这些轻量化静态微工具统一收进一个极简、可自托管的个人工具箱，更原生面向 **AI Coding Agent 生产闭环**而设计：

- **AI 通过 Skill 自主开发并上架工具（核心亮点）**：仓库内置自包含的 [Agent Skills](skills/toolstow-tool/SKILL.md)（含规范与零依赖校验/打包/发布脚本）。把 `skills/toolstow-tool/` 交给 AI Agent 后，只需一句话描述需求，**AI 就能自主完成「编写工具 → 自动校验安全与语法 → 打包 → 携 `TOOLSTOW_API_KEY` 一键发布到你的 ToolStow」全流程**（也可先以 `"draft": true` 草稿上架待你审核）；主题开发同理，交给 [skills/toolstow-theme/](skills/toolstow-theme/SKILL.md) 即可自动生成并校验打包自定义主题。
- **单二进制、零数据库**：一个可执行文件开箱即用，所有数据集中在 `--data-dir`（默认 `./data`）下，文件系统即唯一事实来源（`tools/` + `themes/` + `settings.json`），支持文件变更自动热加载。
- **热插拔与双密钥安全隔离**：支持 AI 携 `TOOLSTOW_API_KEY` 自动发布、在 `/admin` 网页拖拽上传 `.zip` 或直接放入 `data/tools/` 目录，均无需重启；管理员网页登录（`TOOLSTOW_ADMIN_KEY`）与机器发布密钥严格隔离，未配置密钥时管理入口默认关闭（`404`）。
- **草稿预览与双重沙箱**：支持 Hugo 风格 `"draft": true` 草稿（仅管理员可见）；所有子工具运行在 `<iframe sandbox>` + `Content-Security-Policy: sandbox` 双重隔离中（强制剥离 `allow-same-origin`），ZIP 解包防路径穿越、拒符号链接并做体积/文件数防炸弹限制。
- **统一 UI SDK 与多主题换肤**：内置 `/ui/v1/toolstow.css` 与 `/ui/v1/theme.js`，主站与所有子工具自动同步明暗模式与主题皮肤；支持在 `/admin` 上传、预览并热切换多套前台主题，而 `/admin*`、`/api/*`、`/t/*`、`/ui/*` 始终由后端锁定。

---

## 如何使用

### 1. 从 GitHub Releases 直接下载（推荐）

前往 [Releases](https://github.com/Pyaxy/ToolStow/releases) 下载对应平台的压缩包：

| 平台 | 文件名 |
| :--- | :--- |
| Linux x86_64（静态 musl） | `toolstow-vX.Y.Z-linux-amd64.tar.gz` |
| Linux ARM64（静态 musl） | `toolstow-vX.Y.Z-linux-arm64.tar.gz` |
| macOS Apple Silicon | `toolstow-vX.Y.Z-macos-arm64.tar.gz` |
| Windows x64 | `toolstow-vX.Y.Z-windows-amd64.zip` |

可选：下载同页的 `SHA256SUMS.txt` 校验文件完整性（`sha256sum -c SHA256SUMS.txt --ignore-missing`）。

解压后启动服务（默认监听 `127.0.0.1:8080`，数据目录 `./data`）：

```bash
# Linux / macOS
tar -xzf toolstow-vX.Y.Z-linux-amd64.tar.gz
./toolstow

# Windows (PowerShell)
# 解压后直接运行 .\toolstow.exe
```

### 2. 配置管理密钥（可选，不配置则以纯只读模式运行）

如需启用 `/admin` 网页管理后台或 AI 自动发布接口，将 `.env.example` 复制为 `.env` 并写入两把独立随机密钥：

```bash
cp .env.example .env
openssl rand -hex 32   # 填入 TOOLSTOW_ADMIN_KEY（/admin 网页单密钥登录：管理工具、主题与站点设置）
openssl rand -hex 32   # 填入 TOOLSTOW_API_KEY（AI / 脚本 Bearer 发布：仅限 /api/admin/tools*）
chmod 600 .env
```

启动后访问：
- **公开工具箱**：`http://127.0.0.1:8080`
- **管理后台**：`http://127.0.0.1:8080/admin`（配置 `TOOLSTOW_ADMIN_KEY` 后可用；登录态下回到首页顶栏会自动显示「管理 / 设置」入口）

> **附带示例合集包**：Releases 同时提供两个可直接体验的合集包，解压后把里面单个 `.zip` 拖进 `/admin` 即可上传——
> - `toolstow-plugins.zip`：内置示例工具 `docker-cheatsheet`（Docker 命令速查）、`dynamic-favicon`（随主题同步的动态 Favicon 生成器）。
> - `toolstow-themes.zip`：内置示例主题 `ascii-probe`（终端 TUI）、`serverstatus`（运维表格控制台）、`rakugaki`（手绘笔记本涂鸦）、`paper-warm`（羊皮纸纯皮肤换肤）。

### 3. 使用 Docker / Docker Compose 运行（支持 `linux/amd64` / `linux/arm64`）

下载仓库中的 [compose.yml](compose.yml)（如需启用管理密钥，可在同目录放一份基于 [.env.example](.env.example) 修改的 `.env`），然后启动：

```bash
# 拉取预构建镜像并启动（默认使用 compose.yml 中的 TOOLSTOW_HOST=0.0.0.0 与 /data 持久化卷）
docker compose up -d
```

或者直接 `docker run`（配置好密钥见上一步 `.env`）：

```bash
docker run -d \
  --name toolstow \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  --env-file .env \
  -e TOOLSTOW_HOST=0.0.0.0 \
  -e TOOLSTOW_DATA_DIR=/data \
  -v toolstow_data:/data \
  ghcr.io/pyaxy/toolstow:latest
```

更多挂载权限、本地源码镜像构建与反代细节见 [docs/deployment.md](docs/deployment.md)。

### 4. 从源码编译（可选）

需要 Rust 最新稳定版（Edition 2024）：

```bash
cargo build --release
./target/release/toolstow --version
```

生产环境 VPS 部署（`systemd` + Caddy HTTPS 反代）请参阅 [docs/deployment.md](docs/deployment.md)。

---

## 如何制作工具

每个 ToolStow 工具都是一个**零构建、自包含的静态网页目录**，放进 `data/tools/<id>/` 或打成 `.zip` 上传即可生效。

### 最快方式：直接把 Skill 交给 AI

仓库内置了完整的自包含 Agent Skill 目录 **[skills/toolstow-tool/](skills/toolstow-tool/SKILL.md)**（包含 `SKILL.md` 规范和零外部依赖 Python 校验/打包/发布脚本），可直接整夹拷贝进 Claude Code、Cursor、Antigravity 等 AI Coding Agent 的 Skills 目录，然后直接对 AI 说需求（例如 *"帮我做一个日期计算工具并打包"*）。

### 最小目录结构

```text
my-tool/
├── manifest.json   # 必填：至少包含 id 与 name
├── index.html      # 默认入口，在 <head> 引入 /ui/v1/toolstow.css 与 /ui/v1/theme.js
└── assets/         # 可选本地相对资源（避免外部 CDN）
```

最小 `manifest.json` 示例：

```json
{
  "id": "json-pretty",
  "name": "JSON 格式化",
  "version": "1.0.0",
  "category": "开发",
  "icon": "🧩",
  "themeAware": true,
  "draft": false
}
```

### 校验、打包与发布

```bash
# 1. 本地校验 manifest、沙箱安全（禁 innerHTML / allow-same-origin / 裸用 localStorage）与 JS 语法
python3 skills/toolstow-tool/scripts/validate.py ./my-tool

# 2. 打成纯净平铺 .zip（可在 /admin 网页拖拽上传）
python3 skills/toolstow-tool/scripts/pack.py ./my-tool -o ./my-tool.zip

# 3. 或直接用 TOOLSTOW_API_KEY 一键发布（默认 overwrite=false 防误覆盖，确认覆盖加 --overwrite）
export TOOLSTOW_URL="http://127.0.0.1:8080"
export TOOLSTOW_API_KEY="<your-api-key>"
python3 skills/toolstow-tool/scripts/publish.py ./my-tool
```

> 在 `manifest.json` 中设 `"draft": true` 可先以草稿状态上传，仅管理员在 `/admin` 预览无误后再一键公开。完整协议与 `.ts-*` / `window.ToolStow` 速查见 [skills/toolstow-tool/SKILL.md](skills/toolstow-tool/SKILL.md)。

---

## 如何制作主题

ToolStow 的视觉样式（配色、强调色、字体、圆角、暗黑模式与前台布局）**完全由主题掌控**，并且支持两种模式：

| 模式 | 所需文件 | 适用场景 |
| :--- | :--- | :--- |
| **纯皮肤换肤（最快）** | `theme.json` + `ui.css`（+ 可选 `preview.svg`） | 保留内置前台壳，仅通过覆盖 `--ts-*` 变量与 `.ts-*` 样式，**同时给主站和所有插件换肤**，零 HTML/JS |
| **整站自定义主题** | `theme.json` + `index.html`（或 `dist/index.html`）+ `ui.css` | 完全自定义公开首页布局（支持原生静态页或 Vite/React 静态构建产物），并经由 `/ui/v1/theme-assets/` 在主站与沙箱插件间共享字体/底纹 |

### 最快方式：使用主题 Skill 与实时热开发

同样内置了自包含 Agent Skill 目录 **[skills/toolstow-theme/](skills/toolstow-theme/SKILL.md)**（可直接交给 AI Agent）。本地开发时可将主题目录直接放在 `./data/themes/<short>/` 下配合运行中的 ToolStow 实时刷新调试：

```bash
# 1. 校验 theme.json（short 不能为 default）、全量 SVG 安全、整站契约与 JS 语法
python3 skills/toolstow-theme/scripts/validate.py ./my-theme

# 2. 打成纯净平铺 .zip
python3 skills/toolstow-theme/scripts/pack.py ./my-theme -o ./toolstow-theme-my-theme.zip
```

打包完成后，登录 **`/admin` → 主题管理** 上传 `.zip` 即可预览并一键热切换。

> **安全边界**：主题上传与切换仅限管理员网页会话（`TOOLSTOW_ADMIN_KEY`），不接受 `TOOLSTOW_API_KEY` Bearer 调用。完整开发契约、`/api/me` & `/api/tools` 结构与 iframe 主题同步代码见 [skills/toolstow-theme/SKILL.md](skills/toolstow-theme/SKILL.md)。

---

## 文档索引与本地开发

- **工具开发 Skill**：[skills/toolstow-tool/SKILL.md](skills/toolstow-tool/SKILL.md)
- **主题开发 Skill**：[skills/toolstow-theme/SKILL.md](skills/toolstow-theme/SKILL.md)
- **VPS 部署与运维**：[docs/deployment.md](docs/deployment.md)
- **完整需求与架构规格**：[docs/requirements.md](docs/requirements.md)
- **变更日志**：[CHANGELOG.md](CHANGELOG.md)

本地运行完整校验（与 CI 一致）：

```bash
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
node --check frontend/app.js admin/admin.js ui/v1/theme.js
python3 scripts/bundle_examples.py --require-node --out-dir /tmp/toolstow-bundles
```

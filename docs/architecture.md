# ToolStow 技术架构与开发参考

本文面向开发者与 AI Coding Agent，汇总从 `README.md` 拆出的系统架构、4 层 UI SDK、沙箱隔离边界、工具/主题开发速查及源码构建校验流程。

---

## 1. 核心架构与 4 层解耦 UI SDK

### 1.1 单二进制与文件系统事实源

- **无外部数据库**：服务端由单一 Rust 二进制（Axum）承载，静态前端、管理后台与共享 UI SDK 均编译内嵌其中。
- **统一数据目录（`--data-dir`，默认 `./data`）**：
  - `data/tools/`：已安装微工具目录；
  - `data/themes/`：已安装主题目录；
  - `data/settings.json`：站点标题、简介、当前启用主题 `activeTheme` 等持久化配置。
- **自动热加载**：通过内存注册表与文件系统监听（`notify`），无论在 `/admin` 上传、通过 API 收录，还是直接往目录增删文件，均可热生效无需重启。

### 1.2 4 层解耦 UI SDK（`/ui/v1/*`）

ToolStow 将主题外观与工具内部结构解耦，核心资源统一由后端锁定路径提供：

- **`/ui/v1/toolstow.css`**：基础极简设计令牌（`--ts-*`）与基础组件原语（`.ts-*`）。当启用了含 `ui.css` 的主题时，后端自动将其追加拼在基础样式之后级联生效，同时换肤主站与所有子工具。
- **`.ts-workbench` 语义化插槽**：子工具通过标准类名（`.ts-wb-toolbar`、`.ts-wb-nav`、`.ts-wb-aside`、`.ts-wb-main`、`.ts-collection`）声明页面结构而无需关心当前主题，主题可在 `ui.css` 中自由重排菜单栏、工具栏与卡片流的位置（如左侧终端目录栏+底部命令条，或右侧手绘书签栏）。
- **`/ui/v1/theme.js`**：防 FOUC（无闪烁）主题同步、嵌入态检测（`data-embed`）、`toolstow:ready` / `toolstow:theme` 握手，并向子工具暴露 `window.ToolStow`（`getTheme()`、`onThemeChange(fn)`、`copy()`、`toast()` 等）。
- **`/ui/v1/host.js`（无头宿主 SDK，`window.ToolStowHost`）**：封装 `/api/me` 与 `/api/tools` 数据拉取、筛选过滤、Hash 深链接、`<iframe sandbox>` 挂载、URL 安全校验（`isSafeToolUrl` 同时校验 `/t/*` 与 `/tp/{grant}/*`）、可见性归一化（`normalizeVisibility`）与主题同步逻辑，供自定义完整主题复用。
- **`/ui/v1/theme-assets/*`**：跨沙箱静态资产接口，附带 CORS（`Access-Control-Allow-Origin: *`）及 SVG/HTML 沙箱响应头，使主题自带字体与背景纹理可在公开首页 `/` 与 `Origin: null` 沙箱插件 iframe 内一致加载。
- **受保护的后端路由边界**：`/admin*`、`/api/*`、`/t/*`、`/tp/*` 与 `/ui/*` 始终由 Rust 后端强制接管，外置主题无法覆盖管理面板、共享 SDK 或绕过沙箱隔离。

---

## 2. 安全边界与沙箱隔离

- **双密钥严格隔离**：
  - `TOOLSTOW_ADMIN_KEY`：仅用于 `/admin` 网页登录态，管理全部工具、主题（`/api/admin/themes*`）与站点设置；
  - `TOOLSTOW_API_KEY`：仅供 AI / 自动化脚本通过 Bearer Token 调用 `/api/admin/tools*`（上传/修改/删除工具），无法访问主题或站点设置接口；
  - 两个密钥均未配置时，`/admin` 与所有 `/api/admin/*` 自动关闭并返回 `404`（纯只读模式）。
- **子工具双重沙箱**：所有工具均运行在 `<iframe sandbox>` 与响应头 `Content-Security-Policy: sandbox` 的双重隔离中，后端强制剥离危险权限（禁止 `allow-same-origin`、`allow-top-navigation*` 等），子工具处于不透明源 `Origin: null`，无法读取同域 Cookie 或管理员会话。
- **ZIP 与单文件 HTML 收录防护**：
  - `.zip` 解包拦截路径穿越（Zip Slip）、符号链接、特殊文件，并校验证明体积与文件数量上限（防解压炸弹）；替换安装采用同盘原子替换；
  - 单文件 `.html` 通过 `/admin` 预览/收录（可见性默认选中「自动（新建公开 / 覆盖保留原状态）」）或调用 `POST /api/admin/tools/import/html`（默认 `overwrite=false`，同 ID 返回 `409` 防误覆盖），支持自动向合法 `<head>` 边界注入 SDK 引用。
- **三态可见性与会话绑定能力 URL（`/tp/{grant}/{id}/*`）**：
  - `manifest.json` 支持 `"visibility": "public" | "private" | "draft"`（默认 `"public"`，兼容旧版 `"draft": true / false`，语义冲突时拒绝返回 `422`；写盘持久化 `draft` 工具时——包括显式 `"visibility": "draft"` 的 `.zip` 上传、单文件 HTML 导入及 `PATCH` 更新——均会自动规范化落盘 `"visibility": "draft"` 与 `"draft": true` 以保障 `<= v0.3.5` 安全降级兼容，写入 `public` / `private` 时保留 `"visibility"` 并移除旧版 `"draft"`）：
    - `public`：对所有访客开放；
    - `private`：个人私有工具，对匿名访客隐藏（`GET /api/tools` 省略、`/t/{id}/*` 返回 `404`），仅已登录 Admin Session 在 `/`（返回 `public` + `private`）与 `/admin` 可见，**不受 `--include-drafts` 影响**；
    - `draft`：开发中预览工具，对匿名隐藏且默认不在 `/` 展示（仅在 `/admin` 管理与新标签页预览，或显式开启 `--include-drafts` 时出现在 `/`）。
  - **覆盖更新时的可见性保留**：当通过 `.zip` 上传或单文件 HTML 导入覆盖已存在的同 ID 工具且输入中同时省略了 `visibility` 与 `draft` 时，后端会直接读取原目录 `manifest.json` 的可见性状态并予以保留（即便原工具其他文件已损坏仍可正常继承；若原 `manifest.json` 本身损坏或不可读，则 fail-closed 拒绝并返回 `422`，避免将非公开工具静默降级为 `public`）；若显式声明了 `visibility` 或 `draft`，则以显式声明为准。
  - **多文件非公开工具能力 URL 与日志脱敏**：为已登录 Admin Session 返回的非公开工具自动将 `entryUrl` / `iconUrl` 签发为会话绑定、按工具隔离的只读路径 `/tp/{grant}/{id}/...`，使相对路径子资源（`./assets/*.js`、`./style.css`、`fetch(...)`）在 `Origin: null` 沙箱 iframe 与新标签页中稳定加载，无需削弱 `SameSite=Strict` 或添加 `allow-same-origin`；登出或会话过期时全部 `grant` 立即吊销。后端 HTTP 追踪日志自动将 `/tp/{grant}/...` 脱敏为 `/tp/[REDACTED]/...`。
  - **健康检查能力探测与缓存防泄露响应头**：`GET /healthz` 返回 `capabilities: ["tool-visibility"]`，`publish.py` 在上传前始终直接检查实际待上传的 ZIP 字节流并 fail-closed 校验 `manifest.json`（无法读取、解析或存在无效/冲突可见性字段时立即中止，绝不默认按 `public` 放行），并在上传 `private` 或新格式 `draft` 包前探测服务端能力；`GET /api/tools` 始终返回 `Cache-Control: private, no-store` 与 `Vary: Cookie`；非公开工具经 `/t/{id}/*` 或 `/tp/{grant}/{id}/*` 提供服务时附加 `Cache-Control: private, no-store`、`Referrer-Policy: no-referrer` 与 `Content-Security-Policy: sandbox ...`。
  - *降级注意*：若回退到 `<= v0.3.5` 旧版二进制，旧版不识别 `manifest.json` 中的 `"visibility"` 字段——不仅 `private` 工具会被当作公开，任何仅含 `"visibility": "draft"` 而缺少 `"draft": true` 的草稿工具同样会被旧版公开；降级前请先移出 `private` 工具并确保所有 `draft` 工具带有 `"draft": true`。

---

## 3. 工具与主题开发速查

### 3.1 开发 ToolStow 微工具

工具是一个零构建、自包含的静态网页（支持单文件 `.html` 直接收录，或包含 `manifest.json` 的目录包）。完整规范、安全规则、`.ts-*` 与 `window.ToolStow` API 见 **[skills/toolstow-tool/SKILL.md](../skills/toolstow-tool/SKILL.md)**。

最小目录结构：

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
  "visibility": "public"
}
```

校验、打包与 API 自动发布：

```bash
# 1. 本地校验 manifest、沙箱安全（禁 innerHTML / allow-same-origin / 裸用 localStorage）与 JS 语法
python3 skills/toolstow-tool/scripts/validate.py ./my-tool

# 2. 打成纯净平铺 .zip（可在 /admin 网页拖拽上传）
python3 skills/toolstow-tool/scripts/pack.py ./my-tool -o ./my-tool.zip

# 3. 或携带 TOOLSTOW_API_KEY 一键发布（默认 overwrite=false 防误覆盖，确认覆盖加 --overwrite）
export TOOLSTOW_URL="http://127.0.0.1:8080"
export TOOLSTOW_API_KEY="<your-api-key>"
python3 skills/toolstow-tool/scripts/publish.py ./my-tool
```

### 3.2 开发 ToolStow 主题

完整开发契约、`/api/me` & `/api/tools` 协议、`/ui/v1/host.js` 用法及 `ui.css` 变量清单见 **[skills/toolstow-theme/SKILL.md](../skills/toolstow-theme/SKILL.md)**。

| 模式 | 所需文件 | 适用场景 |
| :--- | :--- | :--- |
| **纯皮肤换肤（最快）** | `theme.json` + `ui.css`（+ 可选 `preview.svg`） | 保留内置极简前台壳，仅覆盖 `--ts-*` 变量与 `.ts-*` 样式，**同时给主站与所有插件换肤**，零 HTML/JS |
| **整站自定义主题** | `theme.json` + `index.html`（或 `dist/index.html`）+ `ui.css` | 完全接管 `/` 首页布局（支持原生静态页或 Vite/React 构建产物），并可重排插件 `.ts-workbench` 插槽 |

校验与打包（本地开发可将主题目录直接放在 `./data/themes/<short>/` 下热调试）：

```bash
# 1. 校验 theme.json（short 不能为 default）、全量 SVG 安全、整站契约与 JS 语法
python3 skills/toolstow-theme/scripts/validate.py --strict --strict-dir-name ./my-theme

# 2. 打成纯净平铺 .zip（登录 /admin -> 主题管理 上传并热切换）
python3 skills/toolstow-theme/scripts/pack.py --strict --strict-dir-name ./my-theme -o ./toolstow-theme-my-theme.zip
```

> 主题上传与切换仅限管理员网页会话（`TOOLSTOW_ADMIN_KEY`），不接受 `TOOLSTOW_API_KEY` Bearer 调用。

---

## 4. 从源码编译与本地全量校验

环境要求：Rust 最新稳定版（Edition 2024）、Python 3.9+、Node.js（用于前端与 Skill JS 语法检查）。

```bash
# 构建 Release 单二进制
cargo build --release --locked
./target/release/toolstow --version

# 运行与 CI 完全一致的本地全量校验
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
for f in frontend/app.js admin/admin.js ui/v1/theme.js ui/v1/host.js; do
  node --check "$f"
done
python3 -m py_compile skills/toolstow-tool/scripts/*.py skills/toolstow-theme/scripts/*.py scripts/bundle_examples.py
python3 scripts/bundle_examples.py --require-node --out-dir /tmp/toolstow-bundles
```

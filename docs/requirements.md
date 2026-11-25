# ToolStow 项目需求与技术规格书（v0.2.2）

## 1. 项目背景与定位

在 AI 辅助编程成为日常后，个人开发者会高频产出大量一次性或半持久的轻量化小网页工具，例如：

- **命令与脚本速查页**：将自写脚本用法、Docker / 运维常用命令封装成带搜索和一键复制的纯前端页面。
- **纯前端交互小工具**：例如可随页面主题动态调整强调色并打包下载的 Icon / Favicon 生成器、格式转换器等。
- **未来可能的轻后端小工具**：例如运行简单计算脚本（如资源残值计算）或通过受控代理调用外部 REST API。

当前痛点在于：这些工具散落在各个 AI 会话记录与本地临时目录中，每次复用都要翻找 `index.html`；而传统工具站（如 [IT-Tools](https://github.com/corentinth/it-tools)、[OmniTools](https://github.com/iib0011/omni-tools)）属于单体前端项目，每新增一个工具都需要修改源码并重新构建。

**ToolStow 定位**：打造一个**面向纯 VPS 自托管场景的单二进制、零数据库、零构建热插拔动态微工具宿主**。不向 Cloudflare Workers / Vercel 等纯静态托管形态妥协，充分利用 VPS 本地文件系统与常驻进程优势：通过一份轻量统一的 `manifest.json` 协议解耦“AI 生产工具”与“宿主运行工具”——任何符合规范的工具目录放入 `data/tools/`（或通过 `/admin` 拖拽上传 / 携带 API Key 调用 HTTP API / 未来 MCP 发布 `.zip`），无需重新编译或重启服务，即可在极简黑白风格的工具箱界面中检索、沙箱隔离运行，并自动同步明暗主题与强调色。

### 1.1 与现有开源方案的差异化与灵感来源

| 参考项目 | 现有定位 | ToolStow 的差异化与借鉴 |
| :--- | :--- | :--- |
| [monitor-probe/monitor](https://github.com/monitor-probe/monitor) | 极简黑白自托管探针面板，支持轻量化单二进制部署与干净的界面语言 | 借鉴其**极简黑白（Monochrome B&W）高对比美学**与纯 VPS 动态自托管思路，支持外置静态主题覆盖而无需重编后端 |
| [canvas-drop](https://github.com/markpasternak/canvas-drop) | 面向团队协作与权限管理的静态产物发布台 | 去除多租户与复杂账号体系，专注个人工具箱导航与零数据库文件系统事实源 |
| [artifact-site](https://github.com/lexmount/artifact-site) | 面向页面/PDF 归档分享的静态工作台 | 强化“工具箱”交互（`Cmd+K` 唤起、分类与标签筛选、主站与子工具动态强调色联动、草稿预览），采用 Rust 单二进制零依赖部署 |
| [smallweb](https://github.com/pomdtr/smallweb) | 基于泛域名与 Deno 的多站点运行底座 | 无需泛域名配置，开箱自带统一工具画廊、内置单密钥管理台与目录热扫描，同时保留未来扩展轻后端的协议位 |

---

## 2. 功能范围与演进阶段

### 2.1 v0.2 · 纯 VPS 动态宿主（当前基线）

1. **统一 `--data-dir` 数据目录与多主题热插拔管理**
   - 统一通过 `--data-dir`（默认 `./data`，环境变量 `TOOLSTOW_DATA_DIR`）组织所有运行时持久化状态，依然**零数据库**：
     - `data/tools/`：存放已安装微工具及同盘暂存目录 `.staging/`（可由 `--tools-dir` / `TOOLSTOW_TOOLS_DIR` 单独覆盖）。
     - `data/themes/`：多主题包目录（可由 `--themes-dir` / `TOOLSTOW_THEMES_DIR` 单独覆盖），每个子目录 `data/themes/<short>/` 包含 `theme.json` 及可选的 `index.html`（或 `dist/index.html`）、`ui.css`、`preview.svg`；支持在 `/admin` 面板上传 `.zip`（含 `409 THEME_EXISTS` 覆盖确认）、预览、一键切换或删除。同时保留 `data/theme/`（`--theme-dir` / `TOOLSTOW_THEME_DIR`）用于 `activeTheme = "default"` 时的单目录向后兼容。`/admin*`、`/api/*`、`/t/*`、`/ui/*` 路由始终由后端强制锁定，外置主题无法覆盖或劫持。
     - `data/settings.json`：持久化站点标题（`siteTitle`）、站点副标题描述（`siteDescription`）、默认强调色（`defaultAccent`）与当前启用主题（`activeTheme`，默认 `"default"`），文件缺失或损坏时自动回退至安全默认值。
2. **目录自动扫描与热更新**
   - 启动时扫描工具目录（不存在则自动创建）下所有非隐藏子目录，解析合法的 `manifest.json` 构建内存索引。
   - 通过 `notify` 监听文件系统事件（新增、修改、删除工具目录或 `manifest.json`），防抖后热刷新内存索引，无需重启宿主。
   - 容错隔离：单个工具目录缺失入口或 `manifest.json` 校验失败时，仅记录结构化警告日志并跳过该工具，绝不阻塞或中断整个服务。
3. **Hugo 风格草稿可见性（`draft: true / false`）**
   - `manifest.json` 支持可选字段 `"draft": false`（默认 `false` 即已发布）。
   - 标记为 `"draft": true` 的工具对匿名公网访客完全不可见：自动从公开 `GET /api/tools` 过滤，直接访问 `/t/{id}/*` 时返回 `404 Not Found`（不泄露草稿工具是否存在）。
   - 仅当访问者持有**已登录的 Admin Session Cookie**（用于在管理台预览未发布工具），或本地开发调试显式开启 `--include-drafts` / `TOOLSTOW_INCLUDE_DRAFTS=true` 时可见。
4. **双密钥 fail-closed 鉴权体系（人类 Session 与机器 API 分离）**
   - 全部密钥通过 `.env` 文件或系统环境变量注入；**未设置对应密钥时默认 fail-closed（关闭该鉴权通道）**，确保裸跑公网时不会暴露无保护的写入或管理入口。
   - **`TOOLSTOW_ADMIN_KEY`（人类管理员单密钥）**：
     - 访问 `/admin` 提供单密钥登录卡片（无用户名输入，参考 CPA `management.html` 极简交互）。
     - `POST /api/admin/login` 使用常量时间比较校验密钥，并执行单客户端/单 IP 失败限流（5 分钟内失败 5 次触发 `429 Too Many Requests`）。
     - 登录成功后在服务端内存签发 256-bit 随机十六进制 Session Token，通过 `toolstow_admin_session` Cookie 下发（`HttpOnly; SameSite=Strict; Path=/; Max-Age=86400`，在 HTTPS 或 `TOOLSTOW_COOKIE_SECURE=true` 时附加 `Secure`；24 小时 TTL）。
     - 拥有完整管理权限：上传/覆盖 `.zip` 工具包、切换工具 `draft` 状态、预览草稿工具、删除工具、管理多主题（上传/切换/删除 `data/themes/`），以及读写站点设置（`GET/PUT /api/admin/settings`）。
   - **`TOOLSTOW_API_KEY`（机器 / AI / `curl` / 未来 MCP 专用密钥）**：
     - 通过标准请求头 `Authorization: Bearer <token>` 认证，仅授权操作 `/api/admin/tools*`（列出全部含草稿工具、上传/覆盖 `.zip`、切换 `draft`、删除工具）。
     - **职责严格隔离**：`TOOLSTOW_API_KEY` **不能**登录 `/admin`、**不能**换取浏览器 Session、**不能**读取或修改站点设置（`/api/admin/settings`），也**不能**访问多主题管理接口（`/api/admin/themes*`）；反之 `TOOLSTOW_ADMIN_KEY` 也不能作为 Bearer Token 使用。
5. **极简黑白公共只读前端与内嵌管理面板**
   - **公共前端（`/`）**：采用参考 `monitor-probe/monitor` 的极简黑白（Monochrome B&W）美学，**100% 只读**，不包含任何上传或删除操作。支持分类侧边栏（移动端抽屉）、标签筛选、`Cmd+K` / `Ctrl+K` 全局搜索、URL Hash 状态联动、明暗模式切换与强调色 `postMessage` 联动。
   - 公共前端启动时请求 `GET /api/me`（返回 `siteTitle`、`siteDescription`、`defaultAccent`、`isAdmin`、`adminEnabled`、`activeTheme`、`themeSource`），动态渲染站点标题与默认强调色，且**仅当 `isAdmin === true` 时**在顶栏渲染简洁的「管理 / 设置」链接指向 `/admin`；匿名访客界面保持绝对纯净。
   - **内嵌管理面板（`/admin`）**：独立编译进单二进制，与公共前端共享同一极简黑白设计语言，包含单密钥登录、拖拽/选择上传 `.zip`、工具列表管理（全部/已发布/草稿筛选、切换草稿/发布、新标签页预览、删除确认）、**多主题管理卡片**（上传 `.zip` 主题包与 `409` 覆盖确认、预览卡片、一键启用切换、删除回退）、站点设置表单以及 AI API 速查卡片。
6. **静态资源托管与双重沙箱隔离**
   - 通过 `/t/{id}/{*path}` 托管已注册工具静态资源，修改磁盘文件后刷新即生效。
   - 前端外壳通过 `<iframe sandbox="...">` 加载子工具，且后端同步下发 `Content-Security-Policy: sandbox ...` 响应头，保证壳内与直链访问均处于不透明源（Opaque Origin）隔离环境。
7. **配套 AI 生产规范与示例工具**
   - 提供 `docs/ai-tool-skill.md` 作为 Agent Skill / Prompt 规范，涵盖目录结构、`manifest.json`（含 `draft` 字段）、沙箱约束、主题联动模板以及基于 `Authorization: Bearer $TOOLSTOW_API_KEY` 的发布与草稿切换命令。
   - 仓库附带开箱即用的示例工具（`docker-cheatsheet`、`dynamic-favicon`）与配置模板 `.env.example`。

### 2.2 Phase 2 · AI 原生 MCP 发布流（后续阶段）

基于官方 Rust MCP SDK（[rmcp](https://github.com/modelcontextprotocol/rust-sdk)），直接复用同一套工具注册表、归档落盘模块与 `TOOLSTOW_API_KEY` 权限边界，暴露 MCP 工具供本地或远程 AI 客户端无缝调用：

- `list_tools`：查询已有工具列表（含 `draft` 状态）与功能描述，避免 AI 重复造轮子。
- `get_tool_spec`：返回 `manifest.json` 协议规范与 `postMessage` 主题同步模板，供 AI 生成前按需读取。
- `publish_tool`：提交工具文件树或 `.zip` 完成校验发布/覆盖更新并即时生效（可先以 `draft: true` 发布预览）。
- `set_tool_draft`：切换指定工具的草稿/发布状态。
- `delete_tool`：删除指定工具。

### 2.3 Phase 3 · 轻后端扩展（远期预留，当前不实现）

- **受控 API 代理**：在 `manifest.json` 中声明受控代理标识，敏感 API Key 保存在宿主服务端环境变量中，子工具前端通过 `/api/proxy/{tool_id}/...` 安全转发请求。
- **轻量沙箱计算脚本**：激活 `runtime: "script"`，由宿主在受限子进程或沙箱中执行短生命周期脚本并返回 JSON。

### 2.4 明确的非目标（Non-Goals）

为防止过度设计与偏离个人 VPS 工具箱定位，以下内容**明确不做**：

- **不引入外部数据库**（无 SQLite / Postgres / MySQL）：文件系统 `data/` 目录（`tools/` + `settings.json` + 可选 `theme/`）即唯一事实来源（Source of Truth）。
- **不做静态托管平台妥协**：不为了兼容 Cloudflare Workers / Vercel 等纯静态或无本地磁盘环境而阉割热监听、原子落盘与动态管理能力，专注纯 VPS / 家庭服务器常驻运行。
- **不内置通用工具全家桶**：宿主本身是空壳与运行时，不复刻 IT-Tools 的数十个通用内置工具。
- **不做多用户账号体系、注册流程与复杂 RBAC**：仅提供面向个人的双密钥模型（`TOOLSTOW_ADMIN_KEY` 人类单密钥 + `TOOLSTOW_API_KEY` 机器密钥）；若需额外接入外围网关认证，仍可前置 Caddy / Nginx / Cloudflare Access / Tailscale。
- **当前不做多历史版本树、修订号锁或后端脚本执行器 / 工作流编排**。

---

## 3. 核心协议与接口契约

### 3.1 统一数据目录（`--data-dir`）与 ZIP 包结构

默认数据目录为 `./data`（可通过 `--data-dir` 或 `TOOLSTOW_DATA_DIR` 指定），结构如下：

```text
data/
├── settings.json             # 站点设置（siteTitle / siteDescription / defaultAccent / activeTheme）
├── themes/                   # 多主题管理目录（无需重编 Rust，可在 /admin 上传/切换/删除）
│   ├── .staging/             # 内部同盘上传暂存目录（自动忽略）
│   ├── ascii-probe/          # 示例：完整主题 + ui.css（含 theme.json / index.html / ui.css / preview.svg）
│   ├── serverstatus/         # 示例：高密度表格完整主题 + ui.css
│   └── paper-warm/           # 示例：纯皮肤主题（仅 theme.json / ui.css / preview.svg，无 index.html）
├── theme/                    # [可选，向后兼容] activeTheme="default" 时的单目录覆盖
│   ├── ui.css
│   └── index.html
└── tools/                    # 工具注册与静态托管根目录
    ├── .staging/             # 内部同盘上传暂存目录（自动忽略，禁止外部访问）
    ├── docker-cheatsheet/
    │   ├── manifest.json
    │   └── index.html
    └── dynamic-favicon/
        ├── manifest.json
        ├── index.html
        └── assets/
```

磁盘上每个工具自包含于 `tools/{id}/` 目录中；`.` 开头的目录或文件（如 `.staging/`、`.DS_Store`）视为系统保留或临时项，扫描与静态托管一律忽略。

**ZIP 打包兼容约定**：
- 标准格式为压缩包根目录直接包含 `manifest.json` 与入口文件（如 `index.html`）。
- **单层外包目录自动剥离**：为兼容人类或 AI 执行 `zip -r tool.zip my-tool/` 的常见习惯，若压缩包根目录不含 `manifest.json`，在忽略 `__MACOSX/` 与 `.DS_Store` 后若仅存在唯一顶层目录且其下包含 `manifest.json`，解包器自动剥离该顶层外壳目录。
- 解包落盘的目标目录名始终以校验通过的 `manifest.id` 为准（即 `tools/{manifest.id}/`）。

### 3.2 `manifest.json` 字段规范

设计原则为**“最小必填、合理默认”**：已有单文件 `index.html` 只需补充仅含 `id` 和 `name` 的两行 `manifest.json` 即可接入；需要深度集成时再按需声明分类、草稿状态、主题感知或沙箱权限。

```json
{
  "id": "dynamic-favicon",
  "name": "动态强调色 Favicon 生成器",
  "version": "1.0.0",
  "description": "随页面主题动态修改图标强调色并打包下载",
  "category": "设计",
  "tags": ["icon", "favicon", "svg"],
  "icon": "🎨",
  "entry": "index.html",
  "runtime": "static",
  "draft": false,
  "themeAware": true,
  "sandbox": ["allow-scripts", "allow-downloads", "allow-forms"]
}
```

| 字段 | 类型 | 必填 | 默认值 | 校验与语义说明 |
| :--- | :--- | :--- | :--- | :--- |
| `id` | `string` | 是 | - | 全局唯一标识，匹配 `^[a-z0-9]+(?:-[a-z0-9]+)*$`（长度 1–64）；从磁盘扫描时须与所在目录名严格一致 |
| `name` | `string` | 是 | - | 工具展示名称（去除首尾空白后非空） |
| `version` | `string` | 否 | `"0.1.0"` | 版本号字符串，仅作展示与排查参考 |
| `description` | `string` | 否 | `""` | 一句话功能简介，参与全局搜索与卡片展示 |
| `category` | `string` | 否 | `"未分类"` | 左侧导航分组名称（去除首尾空白，空串回退为 `"未分类"`） |
| `tags` | `string[]` | 否 | `[]` | 搜索与筛选标签数组（自动去重、忽略空串） |
| `icon` | `string` | 否 | `"🛠️"` | Emoji 字符，或工具目录内的安全相对图片路径（如 `"assets/icon.svg"`） |
| `entry` | `string` | 否 | `"index.html"` | 入口 HTML 的安全相对路径，禁止 `/` 开头、`..`、`\` 或协议头，且目标普通文件必须存在 |
| `runtime` | `"static" \| "script"` | 否 | `"static"` | 运行模式；当前仅支持 `"static"`，声明为 `"script"` 时拒绝加载并给出明确提示 |
| `draft` | `boolean` | 否 | `false` | Hugo 风格草稿标记；`true` 时对匿名公开列表隐藏且 `/t/{id}/*` 返回 `404`，仅已登录 Admin Session 或本地 `--include-drafts` 可见 |
| `themeAware` | `boolean` | 否 | `false` | 是否订阅主站的明暗模式与强调色 `postMessage` 同步消息 |
| `sandbox` | `string[]` | 否 | `["allow-scripts", "allow-downloads", "allow-forms"]` | 申请的 `iframe sandbox` 权限列表，由宿主按安全白名单过滤后生效 |

### 3.3 多主题包规范（`theme.json`）、共享 UI SDK（`/ui/v1/*`）与主题同步协议

1. **多主题包规范（`data/themes/<short>/theme.json`）**：
   - 每个安装在 `data/themes/<short>/` 的主题包均包含 `theme.json`：
     ```json
     {
       "short": "ascii-probe",
       "name": "ASCII Probe",
       "version": "1.0.0",
       "description": "Retro-modern terminal TUI theme",
       "author": "ToolStow",
       "url": "https://github.com/monitor-probe/themes",
       "preview": "preview.svg"
     }
     ```
   - `short` 须匹配 `^[a-z0-9]+(?:-[a-z0-9]+)*$`（1–64 字符）且禁止为保留值 `"default"`；`name` 非空；`url` 为空或 `http(s)://`；`preview` 为包内安全相对普通文件路径。
   - 主题包至少须包含 `index.html`（优先探测 `dist/index.html`，其次根目录 `index.html`）或 `ui.css` 之一（否则 `422`）：
     - **完整主题**：含 `index.html`（及可选 `ui.css`），激活后覆盖 `/` 公开前台；若有 `ui.css` 则同步追加至 `/ui/v1/toolstow.css`。
     - **纯皮肤主题**：仅含 `ui.css`（无 `index.html`），激活后保留内嵌默认公开前台壳，仅将 `ui.css` 追加至 `/ui/v1/toolstow.css` 统一换肤主站与插件。
   - 仓库 `examples/themes/` 内置 3 套参考主题：`ascii-probe`（终端 TUI 完整主题）、`serverstatus`（高密度表格完整主题）、`paper-warm`（羊皮纸纯皮肤主题）。
2. **共享样式表（`GET /ui/v1/toolstow.css`）与主题 `ui.css` 级联覆盖**：
   - 始终先输出内嵌的极简黑白基础设计令牌（`--ts-*`）与基础组件原语（`.ts-*`）。
   - 解析当前激活主题（`settings.activeTheme`）：若为已安装命名主题且含合法 `ui.css`，执行规范路径 Containment 校验后自动追加拼接输出（符号链接逃逸返回 `403 Forbidden`）；若该命名主题无 `ui.css`，仅返回基础样式（不混入旧版 `data/theme/ui.css`）；当 `activeTheme` 为 `"default"` 时，保持向后兼容检查 `<data-dir>/theme/ui.css`。
3. **主题桥接与辅助脚本（`GET /ui/v1/theme.js`）**：
   - 在 `<head>` 中同步加载，检测嵌入态设置 `data-embed="true" | "false"`（配合 `.ts-standalone-only` 避免 iframe 内重复顶栏），同步解析初始 `?theme=&accent=` 查询参数防止暗色首帧闪烁（FOUC），在独立打开时回退跟随系统 `prefers-color-scheme`。
   - 自动完成 `toolstow:ready` 握手与 `toolstow:theme` 监听（同步 `data-theme`、`--ts-accent` 及主站下发的安全 `--ts-*` 计算快照），并暴露 `window.ToolStow`（`getTheme()`、`onThemeChange(fn)`、`copy(text, fallbackEl?)`、`toast(msg, durationMs?)`）。
4. **底层 `postMessage` 协议**：
   - 当子工具声明 `themeAware: true` 或主动发送 `{ "type": "toolstow:ready" }` 时，主站调用 `iframe.contentWindow.postMessage(message, "*")` 发送：
     ```json
     {
       "type": "toolstow:theme",
       "payload": {
         "mode": "dark",
         "accent": "#18181b",
         "tokens": {
           "--ts-canvas": "#09090b"
         }
       }
     }
     ```
   - *注：由于沙箱子工具运行在不透明源（`"null"`），按 HTML 规范 `targetOrigin` 必须传 `"*"`。默认强调色源自站点设置 `defaultAccent`（默认 `#18181b`），用户可在主站自定义并持久化到主站 `localStorage`。*

### 3.4 HTTP 路由与 API 契约

#### 公开、共享 UI SDK 与工具运行路由

| 方法与路径 | 鉴权要求 | 说明 |
| :--- | :--- | :--- |
| `GET /`（及公共静态资源） | 无 | 返回公共只读前端外壳：优先解析 `activeTheme` 对应 `data/themes/<short>/` 的 `web_root`（`dist/` 或根目录 `index.html`）；若当前主题为纯皮肤包（无 `index.html`）则返回内嵌前端；当 `activeTheme="default"` 时兼容旧版 `data/theme/` 或回退内嵌前端。`/admin*`、`/api/*`、`/t/*`、`/ui/*` 始终由后端锁定 |
| `GET /ui/v1/toolstow.css`（及 `HEAD`） | 无 | 返回内嵌共享基础样式（`text/css; charset=utf-8`，`Cache-Control: no-cache`，`X-Content-Type-Options: nosniff`）；按当前 `activeTheme` 自动追加该主题 `ui.css`（或 `"default"` 时兼容旧版 `data/theme/ui.css`），符号链接逃逸返回 `403` |
| `GET /ui/v1/theme.js`（及 `HEAD`） | 无 | 返回内嵌共享主题桥接与辅助脚本（`text/javascript; charset=utf-8`，`Cache-Control: no-cache`，`X-Content-Type-Options: nosniff`） |
| `GET /ui/{*path}` | 无 | 严格锁定前缀兜底：未知共享资产路径一律返回 `404 Not Found` JSON，永不回退至外置主题目录 |
| `GET /api/me` | 无（读取当前 Cookie 状态） | 返回 `{ siteTitle, siteDescription, defaultAccent, isAdmin, adminEnabled, activeTheme, themeSource }`；`themeSource` 为 `"custom"` 或 `"embedded"`；公共前端仅当 `isAdmin === true` 时显示「管理 / 设置」入口 |
| `GET /api/tools` | 无 | 返回公开已发布工具（`draft: false`）的元数据 JSON 数组（按分类与名称稳定排序），支持 `?q=关键词&category=分类&tag=标签`；每项包含归一化清单字段（含 `draft`）及 `entryUrl`（如 `"/t/dynamic-favicon/index.html"`）。仅当本地显式开启 `--include-drafts` 时包含草稿 |
| `GET /t/{id}/{*path}` | 公开工具无需鉴权；草稿工具需 Admin Session 或 `--include-drafts` | 托管已注册工具 `{id}` 目录下的静态文件（访问 `/t/{id}` 或 `/t/{id}/` 重定向至 `/t/{id}/{entry}`）。若目标工具为 `draft: true` 且访客无权限，返回 `404` |

#### 管理台与会话认证路由

| 方法与路径 | 鉴权要求 | 说明 |
| :--- | :--- | :--- |
| `GET /admin`（及 `/admin/*`） | 无（页面内校验会话；需配置 `TOOLSTOW_ADMIN_KEY`） | 当配置 `TOOLSTOW_ADMIN_KEY` 时返回编译期内嵌的极简黑白单密钥管理后台静态资源（始终由后端锁定）；未配置 `TOOLSTOW_ADMIN_KEY` 时 fail-closed 返回 `404` |
| `GET /api/admin/session` | 无（读取当前 Cookie 状态） | 返回 `{ authenticated, adminEnabled }`，供 `/admin` 判断渲染登录卡片或控制台 |
| `POST /api/admin/login` | `TOOLSTOW_ADMIN_KEY` | 提交 JSON `{ "key": "..." }`；未配置 `TOOLSTOW_ADMIN_KEY` 返回 `404`，密钥错误返回 `401`，单 IP 5 分钟内失败 5 次返回 `429`；成功在内存创建 256-bit Session 并下发 `Set-Cookie: toolstow_admin_session=...; HttpOnly; SameSite=Strict; Path=/; Max-Age=86400`（HTTPS 或 `TOOLSTOW_COOKIE_SECURE=true` 时附加 `Secure`） |
| `POST /api/admin/logout` | 无（携带 Cookie 则销毁） | 销毁当前内存 Session 并清除 `toolstow_admin_session` Cookie |

#### 管理写操作 API（工具管理 & 站点设置）

| 方法与路径 | 允许凭证 | 说明 |
| :--- | :--- | :--- |
| `GET /api/admin/tools` | Admin Session **或** `Bearer TOOLSTOW_API_KEY` | 返回全部工具（含 `draft: true` 草稿），支持 `?q=&category=&tag=` |
| `POST /api/admin/tools/upload` | Admin Session **或** `Bearer TOOLSTOW_API_KEY` | `multipart/form-data`（字段 `file`）上传 `.zip` 工具包；校验通过后原子写入/覆盖 `tools/{id}/` 并刷新内存索引（新建 `201`，覆盖更新 `200`）；校验失败返回结构化错误 JSON（`400` / `413` / `422`） |
| `PATCH /api/admin/tools/{id}` | Admin Session **或** `Bearer TOOLSTOW_API_KEY` | 提交 JSON `{ "draft": true \| false }`，原子写回 `tools/{id}/manifest.json` 并立即同步内存索引，返回更新后的工具元数据 JSON（不存在返回 `404`） |
| `DELETE /api/admin/tools/{id}` | Admin Session **或** `Bearer TOOLSTOW_API_KEY` | 删除指定工具目录并同步移除内存索引，成功返回 `204 No Content`，不存在返回 `404` |
| `GET /api/admin/settings` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 读取当前站点设置 `{ siteTitle, siteDescription, defaultAccent, activeTheme }` |
| `PUT /api/admin/settings` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 更新站点设置（支持全量或部分字段），校验通过后原子写入 `<data-dir>/settings.json` 并刷新内存状态：`siteTitle` 非空最长 80 字符、`siteDescription` 最长 240 字符、`defaultAccent` 为合法 `#rgb` / `#rrggbb` 十六进制色值 |
| `GET /api/admin/themes` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 返回内置 `default` 与 `data/themes/` 下全部合法已安装主题数组（含 `short`、`name`、`version`、`description`、`author`、`url`、`builtin`、`active`、`hasCustomShell`、`hasUiCss`、`previewUrl`） |
| `GET /api/admin/themes/{short}/preview` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 读取指定主题声明的 `preview` 文件并附带 `Content-Security-Policy: sandbox;`、`X-Content-Type-Options: nosniff`、`Cache-Control: no-cache` 返回 |
| `POST /api/admin/themes/upload` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | `multipart/form-data`（字段 `file`）上传 `.zip` 主题包；支持 `?overwrite=false\|true`（默认 `true`，`false` 时同名冲突返回 `409 Conflict` `THEME_EXISTS` 含 `existing`/`incoming`）；同盘 `.staging/` 安全解包校验并原子替换（新建 `201`，覆盖 `200`） |
| `PUT /api/admin/themes/active` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 提交 JSON `{ "short": "<short>" }` 切换当前激活主题（`"default"` 或已安装合法主题，否则 `404`/`422`），原子持久化至 `settings.json`，返回 `200 { "activeTheme": "<short>" }` |
| `DELETE /api/admin/themes/{short}` | **仅 Admin Session**（`TOOLSTOW_API_KEY` 拒绝） | 删除指定已安装主题目录（禁止删除 `"default"` 返回 `400`，不存在返回 `404`）；若删除的恰为当前激活主题则自动回退 `activeTheme` 至 `"default"`，成功返回 `204 No Content` |

> 所有错误响应统一采用 `{ "code": "...", "error": "..." }` JSON 结构，涵盖 `400`（`BAD_REQUEST`）、`401`（`unauthorized`）、`403`（`FORBIDDEN`）、`404`（`NOT_FOUND`）、`409`（`TOOL_EXISTS` / `THEME_EXISTS`）、`413`（`PAYLOAD_TOO_LARGE`）、`422`（`UNPROCESSABLE_ENTITY`）、`429`（`too_many_requests`）、`500`（`INTERNAL_ERROR`）。

---

## 4. 安全边界与隔离模型

1. **严格防路径穿越与解包炸弹（Zip Slip & Zip Bomb 防护）**
   - `manifest.id` 严格匹配小写字母、数字与连字符正则；`entry` 与相对路径型 `icon` 禁止任何越界段（`.`、`..`、反斜杠、驱动器前缀）。
   - ZIP 解压在阻塞线程池（`tokio::task::spawn_blocking`）中执行，逐条目使用 `ZipFile::enclosed_name()` 校验路径；**拒绝符号链接（Symlink）**与路径归一化后冲突的重复文件。
   - 资源配额硬限制：单次上传请求体默认上限 **50 MB**（可通过 `--max-upload-mb` 调整），解压后总字节数上限 **200 MB**，单包文件总数上限 **1,000 个**。超限立即中止并清理暂存目录。
2. **同盘暂存与原子替换**
   - 上传解包统一在 `tools/.staging/{uuid}/`（与 `tools/` 同文件系统）内完成全部文件写出与 `manifest.json` 校验。
   - 仅当校验 100% 通过后，再执行目录原子就位（若目标 `tools/{id}` 已存在，先将其移至临时备份路径，再将新目录 `rename` 就位，最后清理旧备份；若切换失败则回滚旧目录）。`settings.json` 与 `PATCH` 写回 `manifest.json` 同样采用同目录临时文件 + 原子 `rename` 策略。
3. **沙箱白名单与直链同级隔离**
   - 宿主后端对 `manifest.sandbox` 实施严格白名单过滤，仅允许：`allow-scripts`、`allow-downloads`、`allow-forms`、`allow-modals`、`allow-popups`、`allow-popups-to-escape-sandbox`。
   - **绝对禁止 `allow-same-origin` 与 `allow-top-navigation*`**：未知或高危权限项一律在后端剥离并记录警告，防止子工具读取主站 Cookie / `localStorage` 或篡改顶层窗口。
   - `/t/{id}/{*path}` 仅对内存索引中已合法注册且当前访问者可见的 `{id}` 开放访问（阻断探测 `.staging`、未通过校验目录与匿名探测 `draft: true` 草稿），并返回 `Content-Security-Policy: sandbox <白名单权限>;`、`X-Content-Type-Options: nosniff` 与 `Cache-Control: no-cache`。
   - 公共前端与管理后台渲染所有动态字段（工具名称、描述、标签、站点标题、错误提示等）一律使用纯文本绑定（`textContent`）或受控 DOM 属性，严禁拼接未转义 `innerHTML`。
4. **写接口防沙箱跨源伪造（CSRF 防护）**
   - 尽管沙箱子工具处于不透明源（`Origin: null`），浏览器仍允许其发起无需预检的表单或 `POST` 请求。
   - 所有状态变更接口（`/api/admin/login`、`/api/admin/logout`、`/api/admin/tools/upload`、`PATCH/DELETE /api/admin/tools/{id}`、`PUT /api/admin/settings`、`POST /api/admin/themes/upload`、`PUT /api/admin/themes/active`、`DELETE /api/admin/themes/{short}`）必须校验来源：拒绝 `Origin: null` 或跨源 `Sec-Fetch-Site: cross-site` 请求；浏览器端管理后台统一附带 `X-ToolStow-Client: 1` 请求头触发 CORS 预检；无 `Origin` 头的 `curl` / Agent CLI 请求在携带有效 Bearer API Key 时正常放行。
5. **双密钥 fail-closed、防爆破与外置主题路由锁定**
   - `TOOLSTOW_ADMIN_KEY` 与 `TOOLSTOW_API_KEY` 未配置时分别关闭对应登录/API 通道（fail-closed）；两者均使用常量时间比较函数 `constant_time_eq` 校验，抵御时序侧信道攻击。
   - 对失败的 Admin 登录与 Bearer 校验实施单客户端/单 IP 滑动窗口限流：**5 分钟内失败 5 次即返回 `429 Too Many Requests`**；登录成功后清除该客户端失败计数。
   - 若配置的密钥长度不足 16 字符，启动时记录 `WARN` 级提示日志，**任何日志中绝不打印密钥明文或 Session Token**。
   - 严禁通过 URL Query 参数传递任何密钥；`TOOLSTOW_API_KEY` 仅接受 `Authorization: Bearer` 头且不可访问 `/admin` 登录、站点设置接口与 `/api/admin/themes*` 多主题管理接口。
   - 外置主题目录（`<data-dir>/themes/<short>/` 与向后兼容的 `<data-dir>/theme/`）仅用于匹配公共前端静态资源及可选的 `ui.css` 级联覆盖（均强制执行规范路径 Containment 校验，符号链接逃逸返回 `403`）；`/admin*`、`/api/*`、`/t/*`、`/ui/*` 路由始终由后端最高优先级锁定，避免自定义主题误配或污染核心控制面与共享 SDK。

---

## 5. 技术栈与工程架构（Rust 单二进制）

本项目兼顾**纯 VPS 自托管极简运维**与**清晰的 Rust 工程实践**，坚持依赖克制、零 `unsafe` 配置加载、模块边界明确。

### 5.1 技术选型与配置项

| 模块 | 选型 | 说明 |
| :--- | :--- | :--- |
| 语言与异步运行时 | Rust Stable (Edition 2024) + `tokio` | 单 Cargo 工程、单静态资源内嵌二进制，提交 `Cargo.lock` |
| Web 框架与中间件 | `axum` + `tower-http` | 路由、`Multipart` 上传、`DefaultBodyLimit`、安全响应头与静态文件服务 |
| 序列化与校验 | `serde` + `serde_json` | `manifest.json`、`settings.json` 反序列化与显式业务规则校验 |
| 前端资源内嵌 | `rust-embed` + `mime_guess` | 将共享 UI SDK `ui/`、默认公共前端 `frontend/` 与管理后台 `admin/` 编译进单二进制；公共前端与 `/ui/v1/toolstow.css` 支持 `<data-dir>/themes/<short>/` 多主题热切换与 `ui.css` 级联追加 |
| 目录热监听 | `notify` | 监听 `<data-dir>/tools/` 文件系统事件，配合防抖（Debounce）触发索引热更新 |
| 归档解压 | `zip` + `tempfile` | 阻塞任务中安全流式解压、同盘暂存目录管理与原子替换 |
| CLI 与 `.env` 配置 | `clap` (derive) + 内置零依赖 `.env` 解析器 | 优先级：`CLI 参数 > 系统环境变量 > .env 文件 > 默认值`（零 `unsafe`） |
| 错误与日志 | `anyhow` / `thiserror` + `tracing` / `tracing-subscriber` | 统一 HTTP 状态码 JSON 映射与结构化日志输出 |
| 前端界面 | 原生 HTML + CSS + ES Module JS | 零前端构建链，极简黑白（`monitor-probe/monitor` 风格），桌面与移动端自适应 |
| MCP Server（Phase 2） | `rmcp` | 官方 Rust MCP SDK |

**运行参数与环境变量对照**：

| CLI 参数 | 环境变量 | 默认值 | 说明 |
| :--- | :--- | :--- | :--- |
| `--host` | `TOOLSTOW_HOST` | `127.0.0.1` | 监听地址（公网或容器反向代理按需设为 `0.0.0.0`） |
| `--port` | `TOOLSTOW_PORT` | `8080` | 监听端口 |
| `--data-dir` | `TOOLSTOW_DATA_DIR` | `./data` | 统一数据目录（含 `tools/`、可选 `theme/`、`settings.json`） |
| `--tools-dir` | `TOOLSTOW_TOOLS_DIR` | `<data-dir>/tools` | 工具目录独立覆盖路径 |
| `--themes-dir` | `TOOLSTOW_THEMES_DIR` | `<data-dir>/themes` | 多主题包目录独立覆盖路径 |
| `--theme-dir` | `TOOLSTOW_THEME_DIR` | `<data-dir>/theme` | 向后兼容单主题覆盖目录路径 |
| `--max-upload-mb` | `TOOLSTOW_MAX_UPLOAD_MB` | `50` | 单次 `.zip` 上传体积上限（MB） |
| `--include-drafts` | `TOOLSTOW_INCLUDE_DRAFTS` | `false` | 本地调试时在公开视图直接显示 `draft: true` 草稿工具 |
| `--cookie-secure` | `TOOLSTOW_COOKIE_SECURE` | `false` | 强制为 Admin Session Cookie 附加 `Secure` 属性（HTTPS 反代推荐开启） |
| — | `TOOLSTOW_ADMIN_KEY` | 未设置（关闭） | 人类 `/admin` 单密钥登录密钥（仅通过环境变量 / `.env` 注入） |
| — | `TOOLSTOW_API_KEY` | 未设置（关闭） | 机器 / AI `Authorization: Bearer` 密钥（仅管理 `/api/admin/tools*`） |

### 5.2 项目目录结构

```text
toolstow/
├── Cargo.toml                # Crate 清单（v0.2.2，Rust Edition 2024）
├── Cargo.lock
├── README.md                 # 项目总览与快速开始
├── CHANGELOG.md              # 版本变更记录
├── .env.example              # 环境变量与双密钥生成示例模板
├── .gitignore                # 忽略 /target, /tools, /data, .env 等运行产物
├── docs/
│   ├── requirements.md       # 完整项目需求与技术规格书
│   ├── deployment.md         # VPS 部署、反代 HTTPS 与自定义主题指南
│   └── ai-tool-skill.md      # 供 AI / Agent 读取的工具开发、草稿与 API 发布规范
├── ui/                       # 共享 UI SDK（内嵌进二进制，/ui/v1/* 始终后端锁定）
│   └── v1/
│       ├── toolstow.css      # 统一极简黑白设计令牌（--ts-*）与基础组件原语（.ts-*）
│       └── theme.js          # 防 FOUC 主题同步、嵌入态检测与 ToolStow 辅助 API
├── frontend/                 # 极简黑白只读公共前端（内嵌进二进制，可被 data/theme/ 覆盖）
│   ├── index.html
│   ├── app.js
│   └── style.css
├── admin/                    # 极简黑白单密钥管理面板（内嵌进二进制，/admin 始终后端锁定）
│   ├── index.html
│   ├── admin.js
│   └── admin.css
├── examples/                 # 开箱验证示例工具与内置主题包
│   ├── docker-cheatsheet/
│   │   ├── manifest.json
│   │   └── index.html
│   ├── dynamic-favicon/
│   │   ├── manifest.json
│   │   └── index.html
│   └── themes/
│       ├── ascii-probe/      # 复古终端 TUI 完整主题 + ui.css
│       ├── serverstatus/     # 高密度运维表格完整主题 + ui.css
│       └── paper-warm/       # 温润羊皮纸纯皮肤主题（仅 ui.css）
└── src/
    ├── main.rs               # CLI/.env 加载、日志初始化、组装 AppState、启动 Axum 与目录 Watcher
    ├── lib.rs                # 模块导出
    ├── config.rs             # CLI + 环境变量 + .env 解析与数据目录派生
    ├── error.rs              # 统一业务错误类型（含 401/409/429）与 Axum IntoResponse JSON 映射
    ├── manifest.rs           # Manifest 数据结构（含 draft）、默认值填充、路径与沙箱白名单校验
    ├── settings.rs           # data/settings.json 持久化读写与校验（含 activeTheme）
    ├── themes.rs             # data/themes/ 多主题校验、列表、预览解析、安全 ZIP 解包与删除
    ├── auth.rs               # 双密钥常量时间校验、256-bit 内存 Session、Cookie 构造与失败限流
    ├── registry.rs           # 内存注册表、草稿过滤查询、全量扫描与 notify 防抖监听
    ├── archive.rs            # ZIP 解包、防穿越/防炸弹、同盘原子替换与 draft 原子更新
    └── routes.rs             # 公开/多主题解析/管理 API 路由、双密钥鉴权、CSRF 防护与沙箱托管
```

### 5.3 并发与工程硬约束

1. **锁作用域最小化**：内存注册表、站点设置与认证会话表使用 `std::sync::RwLock` / `Mutex`，任何锁守卫（Guard）**严禁跨越 `.await` 点**；读取时克隆快照后立即释放锁，磁盘 I/O 与解包全部在锁外完成。
2. **写操作串行化与双通道一致性**：针对工具目录的上传、草稿切换与删除通过写锁串行协调；HTTP 管理操作落盘成功后**立即同步更新内存注册表**（确保紧随其后的查询强一致），同时 `notify` 监听器负责兜底捕获外部直接对 `tools/` 目录的手工增删改。

---

## 6. 渐进式实施里程碑与验收标准

### 6.1 演进里程碑

1. **v0.1（已完成）· 核心单二进制闭环**
   - 完成 `manifest.rs` 校验与白名单过滤、`registry.rs` 扫描与 `notify` 热监听、`archive.rs` 安全解包与同盘原子替换、双重沙箱托管、基础前端壳与示例工具。
2. **v0.2（当前目标）· 纯 VPS 动态宿主升级**
   - 落地统一 `--data-dir`（`tools/` + 可选 `theme/` + `settings.json`）与 `.env` 零依赖配置加载。
   - 落地双密钥 fail-closed 鉴权（`TOOLSTOW_ADMIN_KEY` 人类 Session + `TOOLSTOW_API_KEY` 机器 Bearer）、5 次 / 5 分钟 `429` 限流与职责隔离。
   - 落地 Hugo 风格 `draft` 草稿可见性控制与 `PATCH /api/admin/tools/{id}` 原子切换。
   - 重构公共只读前端与内嵌管理台为 `monitor-probe/monitor` 极简黑白美学，公共前端根据 `GET /api/me` 仅对管理员展示「管理 / 设置」入口。
3. **Phase 2 · AI 原生 MCP 发布流**
   - 接入 `rmcp`，复用注册表、归档模块与 API Key 权限模型实现 AI 客户端直连发布与草稿管理。

### 6.2 v0.2 核心验收矩阵

| 验收场景 | 通过条件 |
| :--- | :--- |
| **纯 VPS 零数据库启动** | `cargo build --release` 生成单一可执行文件，空目录启动自动创建 `./data/tools`，加载 `.env`（若存在），打开公共前端即见极简黑白界面 |
| **Fail-Closed 默认安全** | 未配置 `TOOLSTOW_ADMIN_KEY` 与 `TOOLSTOW_API_KEY` 时：`GET /admin`（及 `/admin/*`）、`/api/admin/login`、`/api/admin/tools*` 与 `/api/admin/settings` 均 fail-closed 返回 `404`，公共前端不显示管理入口 |
| **双密钥职责隔离与防爆破** | `TOOLSTOW_ADMIN_KEY` 可登录 `/admin` 换取 `HttpOnly; SameSite=Strict` Cookie 并管理工具与站点设置，但作为 Bearer Token 被拒绝；`TOOLSTOW_API_KEY` 可经 `Authorization: Bearer` 读写 `/api/admin/tools*`，但无法登录 `/admin` 或访问 `/api/admin/settings`；连续 5 次错误凭证后第 6 次触发 `429` |
| **Hugo 风格草稿可见性** | `draft: true` 工具在公开 `GET /api/tools` 中隐藏且匿名访问 `/t/{id}/*` 返回 `404`；登录 `/admin` 后可在列表看到、通过「预览」打开并通过 `PATCH /api/admin/tools/{id}` 一键发布上线；本地启动加 `--include-drafts` 时公开视图直接可见 |
| **外置静态主题热覆盖与路由锁定** | 在 `data/theme/` 放置自定义 `index.html` 后无需重编 Rust，访问 `/` 立即生效且 `/api/me` 返回 `themeSource: "custom"`；放置 `data/theme/ui.css` 后 `GET /ui/v1/toolstow.css` 自动追加该覆盖样式以统一换肤主站与插件（符号链接逃逸返回 `403`）；访问 `/admin`、`/api/tools`、`/t/{id}/*`、`/ui/v1/*` 仍由后端强制接管不被覆盖 |
| **公共只读前端与条件管理入口** | 公共前端完全只读无上传/删除按钮；未登录时无任何管理入口；登录 `/admin` 后返回 `/` 顶栏展示「管理 / 设置」链接；站点标题/描述/默认强调色修改后实时反映到 `/api/me` 与公共页 |
| **AI 生产闭环与沙箱安全边界** | 按 `docs/ai-tool-skill.md` 使用 `Authorization: Bearer $TOOLSTOW_API_KEY` 完成上传、覆写、草稿切换与删除；坏包/越界/符号链接/超限包被拒绝且无残留；子工具双重沙箱剥离 `allow-same-origin`，沙箱内伪造写请求被拦截 |

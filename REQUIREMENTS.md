# ToolStow 项目需求与技术规格书

## 1. 项目背景与定位

在 AI 辅助编程成为日常后，个人开发者会高频产出大量一次性或半持久的轻量化小网页工具，例如：

- **命令与脚本速查页**：将自写脚本用法、Docker / 运维常用命令封装成带搜索和一键复制的纯前端页面。
- **纯前端交互小工具**：例如可随页面主题动态调整强调色并打包下载的 Icon / Favicon 生成器、格式转换器等。
- **未来可能的轻后端小工具**：例如运行简单计算脚本（如资源残值计算）或通过受控代理调用外部 REST API。

当前痛点在于：这些工具散落在各个 AI 会话记录与本地临时目录中，每次复用都要翻找 `index.html`；而传统工具站（如 [IT-Tools](https://github.com/corentinth/it-tools)、[OmniTools](https://github.com/iib0011/omni-tools)）属于单体前端项目，每新增一个工具都需要修改源码并重新构建。

**ToolStow 定位**：打造一个**单二进制、零数据库、零构建热插拔**的自托管个人微工具宿主。核心思路是通过一份轻量统一的 `manifest.json` 协议解耦“AI 生产工具”与“宿主运行工具”——任何符合规范的工具目录放入 `tools/`（或通过网页拖拽 / HTTP API / MCP 上传 `.zip`），无需重新编译或重启服务，即可立即在统一的工具箱界面（桌面与移动端自适应）中检索、沙箱隔离运行，并自动同步主站明暗主题与强调色。

### 1.1 与现有开源方案的差异化

| 参考项目 | 现有定位 | ToolStow 的差异化切入点 |
| :--- | :--- | :--- |
| [canvas-drop](https://github.com/markpasternak/canvas-drop) | 面向团队协作与权限管理的静态产物发布台 | 去除多租户与管理后台复杂度，专注极简个人 IT-Tools 风格导航与统一设计语言 |
| [artifact-site](https://github.com/lexmount/artifact-site) | 面向页面/PDF 归档分享的静态工作台 | 强化“工具箱”交互（`Cmd+K` 唤起、分类与标签筛选、主站与子工具动态强调色联动），采用 Rust 单二进制零依赖部署 |
| [smallweb](https://github.com/pomdtr/smallweb) | 基于泛域名与 Deno 的多站点运行底座 | 无需泛域名配置，开箱自带统一工具画廊与目录热扫描，同时保留未来扩展轻后端的协议位 |

---

## 2. 功能范围与演进阶段

### 2.1 Phase 1 · MVP（当前开发目标）

1. **目录自动扫描与热更新**
   - 启动时扫描指定目录（默认 `./tools`，不存在则自动创建）下所有非隐藏子目录，解析合法的 `manifest.json` 构建内存索引。
   - 通过文件监听自动感知目录变更（新增、修改、删除工具目录或 `manifest.json`），防抖后热刷新内存索引，无需重启宿主。
   - 容错隔离：单个工具目录缺失文件或 `manifest.json` 校验失败时，仅记录结构化警告日志并跳过该工具，绝不阻塞或中断整个服务。
2. **静态资源托管与双重沙箱隔离**
   - 通过独立路径 `/t/{id}/{*path}` 托管对应工具目录下的静态文件（HTML / CSS / JS / SVG / 图片等），直接修改磁盘上的工具文件后刷新页面即可生效。
   - 前端外壳通过 `<iframe sandbox="...">` 加载子工具，且后端对静态工具页面同步下发 `Content-Security-Policy: sandbox ...` 响应头，确保无论在导航壳内打开还是通过直链单独打开，子工具均处于不透明源（Opaque Origin）隔离环境中。
3. **极简个人工具箱前端外壳**
   - 编译期内嵌进 Rust 单二进制，无需独立前端构建链或 Node.js 环境，兼顾桌面端与手机浏览器访问体验。
   - 支持左侧分类导航（移动端收起为抽屉）、标签筛选、全局即时搜索（支持 `Cmd+K` / `Ctrl+K` 快捷唤起，匹配名称、描述、标签、ID）。
   - 支持 URL 状态联动（如通过 Hash/Query 记录当前打开的工具 ID），支持一键刷新当前子工具、在新标签页打开独立地址。
   - 内置明暗模式切换（Light / Dark）与自定义强调色（Accent Color），并通过 `postMessage` 实时同步给声明 `themeAware: true` 的子工具。
4. **ZIP 工具包上传、覆盖更新与删除管理**
   - 提供 HTTP API 与前端拖拽上传入口，支持上传包含 `manifest.json` 和入口 HTML 的 `.zip` 工具包。
   - **支持同 ID 覆盖更新**：AI 高频迭代修补工具时，重新上传同 `id` 的 `.zip` 即可在校验通过后原子替换旧目录并刷新索引。
   - 支持通过前端管理操作或 `DELETE` 接口删除指定工具。
5. **配套 AI 生产规范与示例工具**
   - 提供一份可直接作为 Agent Skill / Prompt 使用的工具生成规范文档（`docs/ai-tool-skill.md`），明确目录结构、`manifest.json` 字段、沙箱限制（如无 `localStorage` 同源存储）与主题联动代码模板。
   - 仓库附带开箱即用的示例工具（如命令速查页、动态强调色 Favicon 生成器），用于功能演示与回归验收。

### 2.2 Phase 2 · AI 原生 MCP 发布流（后续阶段）

基于官方 Rust MCP SDK（[rmcp](https://github.com/modelcontextprotocol/rust-sdk)），直接复用同一套工具注册表与归档落盘模块，暴露 MCP 工具供本地或远程 AI 客户端无缝调用：

- `list_tools`：查询已有工具列表与功能描述，避免 AI 重复造轮子。
- `get_tool_spec`：返回 `manifest.json` 协议规范与 `postMessage` 主题同步模板，供 AI 生成前按需读取。
- `publish_tool`：直接提交工具文件树或 `.zip` 完成校验发布/覆盖更新并即时生效。
- `delete_tool`：删除指定工具。

### 2.3 Phase 3 · 轻后端扩展（远期预留，Phase 1 不实现）

- **受控 API 代理**：在 `manifest.json` 中声明受控代理标识，敏感 API Key 保存在宿主服务端环境变量中，子工具前端通过 `/api/proxy/{tool_id}/...` 安全转发请求。
- **轻量沙箱计算脚本**：激活 `runtime: "script"`，由宿主在受限子进程或沙箱中执行短生命周期脚本并返回 JSON。

### 2.4 明确的非目标（Non-Goals）

为防止过度设计与偏离个人工具箱定位，以下内容**明确不做**：

- **不引入外部数据库**（无 SQLite / Postgres / MySQL）：文件系统 `tools/` 目录即唯一事实来源（Source of Truth）。
- **不内置通用工具全家桶**：宿主本身是空壳与运行时，不复刻 IT-Tools 的数十个通用内置工具。
- **不做多用户、RBAC 与复杂内置鉴权体系**：默认监听本地回环地址（`127.0.0.1`）；若部署在公网或家庭服务器，访问认证统一交由前置反向代理（如 Caddy BasicAuth / Authelia / Cloudflare Access）或组网工具（Tailscale）处理。
- **Phase 1 不做多历史版本树与修订号锁，也不实现后端脚本执行器或工作流编排**。

---

## 3. 核心协议与接口契约

### 3.1 工具目录与 ZIP 包结构

磁盘上每个工具自包含于 `tools/{id}/` 目录中；`.` 开头的目录或文件（如 `tools/.staging/`、`.DS_Store`）视为系统保留或临时项，扫描与静态托管一律忽略：

```text
tools/
├── docker-cheatsheet/
│   ├── manifest.json
│   └── index.html
└── dynamic-favicon/
    ├── manifest.json
    ├── index.html
    └── assets/
```

**ZIP 打包兼容约定**：
- 标准格式为压缩包根目录直接包含 `manifest.json` 与入口文件（如 `index.html`）。
- **单层外包目录自动剥离**：为兼容人类或 AI 执行 `zip -r tool.zip my-tool/` 的常见习惯，若压缩包根目录不含 `manifest.json`，在忽略 `__MACOSX/` 与 `.DS_Store` 后若仅存在唯一顶层目录且其下包含 `manifest.json`，解包器自动剥离该顶层外壳目录。
- 解包落盘的目标目录名始终以校验通过的 `manifest.id` 为准（即 `tools/{manifest.id}/`）。

### 3.2 `manifest.json` 字段规范

设计原则为**“最小必填、合理默认”**：已有单文件 `index.html` 只需补充仅含 `id` 和 `name` 的两行 `manifest.json` 即可接入；需要深度集成时再按需声明分类、主题感知或沙箱权限。

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
| `runtime` | `"static" \| "script"` | 否 | `"static"` | 运行模式；Phase 1 仅支持 `"static"`，声明为 `"script"` 时拒绝加载并给出明确提示 |
| `themeAware` | `boolean` | 否 | `false` | 是否订阅主站的明暗模式与强调色 `postMessage` 同步消息 |
| `sandbox` | `string[]` | 否 | `["allow-scripts", "allow-downloads", "allow-forms"]` | 申请的 `iframe sandbox` 权限列表，由宿主按安全白名单过滤后生效 |

### 3.3 主站与子工具主题同步协议（`postMessage`）

1. **主站向子工具广播主题**：当 `themeAware: true` 的子工具 `iframe` 完成 `load`、子工具主动发送就绪事件，或用户在主站切换明暗模式 / 强调色时，主站调用 `iframe.contentWindow.postMessage(message, "*")` 发送：
   ```json
   {
     "type": "toolstow:theme",
     "payload": {
       "mode": "dark",
       "accent": "#2563EB"
     }
   }
   ```
   *注：由于沙箱子工具运行在不透明源（`"null"`），按 HTML 规范 `targetOrigin` 必须传 `"*"`。*
2. **子工具可选握手**：子工具脚本初始化完成后，可向父窗口发送 `{ "type": "toolstow:ready" }`，主站收到后立即重发一次当前主题状态，消除慢速资源加载导致的时序竞态。
3. **子工具样式约定**：子工具应在 `:root` 中预设默认 `--accent` 等 CSS 变量（确保直接访问 `/t/{id}/index.html` 时样式依然完好），并在 `window.addEventListener("message", ...)` 收到 `type === "toolstow:theme"` 时更新 `--accent` 与 `data-theme`。

### 3.4 HTTP 路由与 API 契约

| 方法与路径 | 说明 |
| :--- | :--- |
| `GET /`（及前端静态资源） | 返回内嵌的工具箱前端外壳页面与脚本样式 |
| `GET /api/tools` | 返回当前所有可用工具的元数据 JSON 数组（按分类与名称稳定排序），支持可选查询参数 `?q=关键词&category=分类&tag=标签`；每项包含归一化清单字段及 `entryUrl`（如 `"/t/dynamic-favicon/index.html"`） |
| `POST /api/tools/upload` | `multipart/form-data`（字段 `file`）上传 `.zip` 工具包；校验通过后原子写入/覆盖 `tools/{id}/`，立即同步内存索引并返回该工具元数据（新建返回 `201`，覆盖更新返回 `200`）；校验失败返回结构化错误 JSON（`400` / `413` / `422`） |
| `DELETE /api/tools/{id}` | 删除指定工具目录并同步移除内存索引，成功返回 `204 No Content`，不存在返回 `404` |
| `GET /t/{id}/{*path}` | 托管已注册工具 `{id}` 目录下的静态文件（访问 `/t/{id}` 或 `/t/{id}/` 重定向至 `/t/{id}/{entry}`） |

---

## 4. 安全边界与隔离模型

1. **严格防路径穿越与解包炸弹（Zip Slip & Zip Bomb 防护）**
   - `manifest.id` 严格匹配小写字母、数字与连字符正则；`entry` 与相对路径型 `icon` 禁止任何越界段（`.`、`..`、反斜杠、驱动器前缀）。
   - ZIP 解压在阻塞线程池（`tokio::task::spawn_blocking`）中执行，逐条目使用 `ZipFile::enclosed_name()` 校验路径；**拒绝符号链接（Symlink）**与路径归一化后冲突的重复文件。
   - 资源配额硬限制：单次上传请求体默认上限 **50 MB**（可通过启动参数调整），解压后总字节数上限 **200 MB**，单包文件总数上限 **1,000 个**。超限立即中止并清理暂存目录。
2. **同盘暂存与原子替换**
   - 上传解包统一在 `tools/.staging/{uuid}/`（与 `tools/` 同文件系统）内完成全部文件写出与 `manifest.json` 校验。
   - 仅当校验 100% 通过后，再执行目录原子就位（若目标 `tools/{id}` 已存在，先将其移至临时备份路径，再将新目录 `rename` 就位，最后清理旧备份；若切换失败则回滚旧目录）。任何坏包或中途失败均不产生半成品，也不破坏已有线上工具。
3. **沙箱白名单与直链同级隔离**
   - 宿主后端对 `manifest.sandbox` 实施严格白名单过滤，仅允许：`allow-scripts`、`allow-downloads`、`allow-forms`、`allow-modals`、`allow-popups`、`allow-popups-to-escape-sandbox`。
   - **绝对禁止 `allow-same-origin` 与 `allow-top-navigation*`**：未知或高危权限项一律在后端剥离并记录警告，防止子工具读取主站 `localStorage` / Cookie 或篡改顶层窗口。
   - `/t/{id}/{*path}` 仅对内存索引中已合法注册的 `{id}` 开放访问（阻断探测 `.staging` 或未通过校验的目录），并返回 `Content-Security-Policy: sandbox <白名单权限>;`、`X-Content-Type-Options: nosniff` 与 `Cache-Control: no-cache`（确保热修改即时可见且直链打开同样受沙箱约束）。
   - 前端外壳渲染子工具名称、描述、标签及相对路径图标时仅使用纯文本绑定或受控 `<img>` 属性，禁止拼接未转义 `innerHTML`。
4. **写接口防沙箱跨源伪造（CSRF 防护）**
   - 尽管沙箱子工具处于不透明源（`Origin: null`），浏览器仍允许其发起无需预检的 `multipart/form-data` `POST` 请求。
   - 因此所有状态变更接口（`POST /api/tools/upload`、`DELETE /api/tools/{id}`）必须校验请求来源：拒绝 `Origin: null` 或跨源 `Sec-Fetch-Site: cross-site` 请求（前端外壳统一附带自定义请求头如 `X-ToolStow-Client: 1` 触发浏览器 CORS 预检拦截沙箱请求；无 `Origin` 头的本地 `curl` / Agent CLI 调用正常放行）。

---

## 5. 技术栈与工程架构（Rust 单二进制）

本项目兼顾**个人自托管极简运维**与**清晰的 Rust 工程实践**，坚持依赖克制、模块边界明确、注释友好。

### 5.1 技术选型

| 模块 | 选型 | 说明 |
| :--- | :--- | :--- |
| 语言与异步运行时 | Rust Stable (Edition 2024) + `tokio` | 单 Cargo 工程、单静态资源内嵌二进制，提交 `Cargo.lock` |
| Web 框架与中间件 | `axum` + `tower-http` | 路由、`Multipart` 上传、`DefaultBodyLimit`、安全响应头与静态文件服务 |
| 序列化与校验 | `serde` + `serde_json` | `manifest.json` 反序列化与显式业务规则校验 |
| 前端资源内嵌 | `rust-embed` + `mime_guess` | Debug 模式下直接读取磁盘 `frontend/` 便于联调，Release 模式下编译进单二进制 |
| 目录热监听 | `notify` | 监听 `tools/` 文件系统事件，配合防抖（Debounce）触发索引热更新 |
| 归档解压 | `zip` + `tempfile` | 阻塞任务中安全流式解压与同盘暂存目录生命周期管理 |
| CLI 参数 | `clap` (derive) | 支持 `--host`（默认 `127.0.0.1`）、`--port`（默认 `8080`）、`--tools-dir`（默认 `./tools`）、`--max-upload-mb`（默认 `50`） |
| 错误与日志 | `anyhow` / `thiserror` + `tracing` / `tracing-subscriber` | 清晰的 HTTP 状态码错误映射与结构化日志输出 |
| 前端导航壳 | 原生 HTML + CSS + ES Module JS | 零前端打包构建链，轻量易读，桌面与移动端自适应 |
| MCP Server（Phase 2） | `rmcp` | 官方 Rust MCP SDK |

### 5.2 推荐项目目录结构

```text
toolstow/
├── Cargo.toml
├── Cargo.lock
├── REQUIREMENTS.md
├── README.md
├── docs/
│   └── ai-tool-skill.md      # 供 AI / Agent 读取的工具开发与打包规范（Skill 模板）
├── frontend/                 # 零构建前端导航外壳（Release 编译进二进制）
│   ├── index.html
│   ├── app.js
│   └── style.css
├── examples/                 # 开箱验证示例工具
│   ├── docker-cheatsheet/
│   │   ├── manifest.json
│   │   └── index.html
│   └── dynamic-favicon/
│       ├── manifest.json
│       └── index.html
└── src/
    ├── main.rs               # CLI 解析、日志初始化、组装 AppState、启动 Axum 与目录 Watcher
    ├── config.rs             # 启动参数与运行配置
    ├── error.rs              # 统一业务错误类型与 Axum IntoResponse JSON 映射
    ├── manifest.rs           # Manifest 数据结构、默认值填充、路径与沙箱白名单校验
    ├── registry.rs           # 内存注册表（Arc<RwLock<HashMap<String, ToolEntry>>>）、全量/增量扫描与 notify 防抖监听
    ├── archive.rs            # ZIP 解包、防穿越/防炸弹检查、单层目录剥离与同盘原子替换
    └── routes.rs             # API 路由、CSRF/Origin 校验、工具沙箱静态托管与内嵌前端路由
```

### 5.3 并发与工程硬约束

1. **锁作用域最小化**：内存注册表使用 `Arc<std::sync::RwLock<...>>`，任何读写锁守卫（Guard）**严禁跨越 `.await` 点**；读取时克隆所需快照后立即释放锁，磁盘 I/O 与解包全部在锁外完成。
2. **写操作串行化与双通道一致性**：同一时刻针对同一 `id` 的上传/删除通过细粒度互斥或写协调避免竞争；HTTP 上传与删除落盘成功后**立即同步更新内存注册表**（确保紧随其后的 `GET /api/tools` 强一致），同时 `notify` 监听器负责兜底捕获外部直接对 `tools/` 目录的手工增删改。

---

## 6. 渐进式实施里程碑与验收标准

按以下 4 个里程碑渐进交付，每一步均保证 `cargo test` 与 `cargo run` 可独立编译验证：

### 6.1 实施里程碑

1. **Milestone 1 · 最小可运行闭环**
   - 完成 `clap` 参数解析、`manifest.rs` 解析与白名单校验、`registry.rs` 启动扫描、`GET /api/tools` 过滤查询与 `GET /t/{id}/{*path}` 沙箱静态托管。
   - **阶段结果**：将示例工具放入 `./tools`，启动服务后 `/api/tools` 返回正确 JSON，访问 `/t/{id}/index.html` 可正常使用且携带 `CSP: sandbox` 响应头。
2. **Milestone 2 · 内嵌前端导航外壳**
   - 完成 `frontend/` 响应式工具箱界面（侧边栏分类、标签筛选、`Cmd+K` 全局搜索、沙箱 `iframe` 容器、明暗模式与强调色 `postMessage` 联动），并通过 `rust-embed` 内嵌。
   - **阶段结果**：`cargo build --release` 产出独立单二进制，复制到任意目录运行即可在桌面和手机浏览器中检索打开工具、实时切换主题与强调色。
3. **Milestone 3 · 目录热更新与 ZIP 上传管理**
   - 接入 `notify` 防抖监听实现手工改目录自动刷新；完成 `archive.rs` 安全解包与同盘原子替换；打通 `POST /api/tools/upload`、`DELETE /api/tools/{id}` 及前端拖拽上传与删除交互。
   - **阶段结果**：运行期往 `tools/` 放入新目录或通过网页/`curl` 上传 `.zip`（含同名覆盖更新），无需重启服务即可立即生效。
4. **Milestone 4 · AI 生产闭环与示例打磨**
   - 编写完善的 `docs/ai-tool-skill.md` 规范文档，补齐两个代表性示例工具（速查页 + 主题联动 Favicon 生成器）与集成测试；为后续接入 `rmcp`（Phase 2）预留清晰接口。

### 6.2 Phase 1 核心验收矩阵

| 验收场景 | 通过条件 |
| :--- | :--- |
| **单二进制零配置启动** | `cargo build --release` 生成单一可执行文件，在空目录启动自动创建 `./tools` 并正常打开前端空状态引导页 |
| **网页拖拽与 AI 上传闭环** | 将 `docs/ai-tool-skill.md` 提供给 AI 生成工具包，通过网页拖拽和 `POST /api/tools/upload` 均可发布；上传同 `id` 新包可原子覆盖更新 |
| **桌面与移动端检索使用** | 在桌面端（含 `Cmd+K` / `Ctrl+K`）与手机浏览器中，按分类、标签、名称、描述或 ID 均能检索到工具；速查页复制与 Favicon 生成下载均正常 |
| **主题与强调色实时同步** | 主站切换明暗模式或修改强调色时，`themeAware: true` 的子工具实时同步配色，无需刷新 `iframe` |
| **磁盘手工热修改与容错** | 运行期间手工新增/删除 `tools/{id}` 目录自动更新列表；修改工具内 HTML/CSS/JS 刷新即见新内容；放入损坏的 `manifest.json` 仅打印警告，不影响其它工具 |
| **安全边界与坏包隔离** | 含 `..` 路径穿越、符号链接、超限体积、非法 `id` 或缺失入口的 `.zip` 上传被拒绝且不留残余文件；子工具 `iframe` 及直链均无 `allow-same-origin`，且从沙箱内发起的伪造上传请求被服务端拒绝 |

# ToolStow

> 单二进制、零数据库、零构建热插拔的个人微工具自托管宿主（`v0.2.1`）

**ToolStow** 面向纯 VPS / 个人服务器自托管场景，把散落在各处 AI 对话与本地临时目录里的轻量化静态网页工具，统一收进一个极简黑白风格的工具箱宿主中：放入 `data/tools/` 目录、在 `/admin` 拖拽上传，或由 AI 携带 Bearer Key 调接口发布，**无需重新编译或重启服务即可即时上线**。

---

## 核心特性

- **单二进制、零数据库**：所有运行时状态集中在 `--data-dir`（默认 `./data`）下，文件系统即唯一事实来源（`tools/` + 可选 `theme/` + `settings.json`），支持 `notify` 防抖热加载。
- **前台主题解耦 + 锁定 `/admin`**：借鉴 [monitor-probe/monitor](https://github.com/monitor-probe/monitor) 的架构与极简黑白美学；将自定义静态前端放入 `data/theme/` 即可热覆写默认首页（免重编 Rust），同时 `/admin*`、`/api/*`、`/t/*` 始终由后端接管。
- **双密钥分层鉴权（默认安全关闭）**：
  - `TOOLSTOW_ADMIN_KEY`：人类在 `/admin` 单密钥登录（无用户名），换取 256-bit 内存 `HttpOnly; SameSite=Strict` 会话 Cookie，可管理工具、预览草稿、修改站点设置；登录后回到公开首页 `/` 顶栏自动显示「管理 / 设置」入口。
  - `TOOLSTOW_API_KEY`：机器 / AI / `curl` 通过 `Authorization: Bearer` 调用 `/api/admin/tools*`（列表、上传/覆盖、切换 `draft`、删除），无法登录 `/admin` 或修改站点设置。
  - 均支持常量时间比对与防爆破限流（单 IP 5 分钟失败 5 次触发 `429`）；未配置密钥时对应管理入口自动关闭返回 `404`。
- **Hugo 风格 `draft` 草稿可见性**：`manifest.json` 设 `"draft": true` 后，对公网列表与直链 `/t/{id}/*` 返回 `404`，管理员 Session 或本地 `--include-drafts` 可直接预览，支持 `PATCH /api/admin/tools/{id}` 原地一键上下架。
- **双重沙箱与解包安全边界**：前端 `<iframe sandbox>` 与后端 `Content-Security-Policy: sandbox` 响应头双重隔离（强制剥离 `allow-same-origin`）；ZIP 上传防 Zip Slip、拒符号链接、流式防炸弹配额限制并在 `tools/.staging/` 同盘原子替换。
- **主站与子工具主题联动**：内置明暗模式切换与自定义强调色，通过 `postMessage`（`toolstow:theme` / `toolstow:ready`）实时同步给 `themeAware: true` 子工具。

---

## 快速开始

### 1. 编译单二进制

```bash
cargo build --release
./target/release/toolstow --version
```

### 2. 配置 `.env`（可选，不配置则以纯只读模式运行）

```bash
cp .env.example .env
# 分别生成两把独立随机密钥写入 .env：
openssl rand -hex 32   # 填入 TOOLSTOW_ADMIN_KEY（用于 /admin 网页登录）
openssl rand -hex 32   # 填入 TOOLSTOW_API_KEY（用于 AI / curl Bearer 发布）
chmod 600 .env
```

### 3. 启动并体验示例工具

```bash
# 启动服务（默认监听 127.0.0.1:8080，数据目录 ./data）
./target/release/toolstow

# 可选：直接把仓库自带示例工具拷贝进 data/tools/ 热加载体验
cp -r examples/docker-cheatsheet examples/dynamic-favicon data/tools/
```

启动后访问：
- 公开工具箱：`http://127.0.0.1:8080`
- 管理后台：`http://127.0.0.1:8080/admin`（配置 `TOOLSTOW_ADMIN_KEY` 后可用）

---

## 常用 API 速览

| 接口 | 权限 | 说明 |
| :--- | :--- | :--- |
| `GET /api/me` | 公开 | 站点标题、描述、默认强调色、当前登录态（`isAdmin`）与主题来源 |
| `GET /api/tools` | 公开 | 已发布（`draft: false`）工具列表，支持 `?q=&category=&tag=` |
| `GET /t/{id}/{*path}` | 公开 / 管理员 | 沙箱静态托管（草稿工具仅管理员 Session 或 `--include-drafts` 可访问） |
| `GET /admin` | `TOOLSTOW_ADMIN_KEY` | 内置单密钥管理面板（未配置密钥时返回 `404`） |
| `GET /api/admin/tools` | Admin Session 或 API Key | 全量工具列表（含草稿），支持 `?q=&category=&tag=&draft=true\|false` |
| `POST /api/admin/tools/upload` | Admin Session 或 API Key | `multipart/form-data`（`file`）上传或同名覆盖 `.zip` 工具包 |
| `PATCH /api/admin/tools/{id}` | Admin Session 或 API Key | JSON `{"draft": true\|false}` 原地原子切换草稿/发布状态 |
| `DELETE /api/admin/tools/{id}` | Admin Session 或 API Key | 删除指定工具目录并移除内存索引 |
| `GET/PUT /api/admin/settings` | 仅 Admin Session | 读取或更新 `data/settings.json`（站点名称、描述、默认强调色） |

---

## 项目文档索引

- **需求与技术规格**：[docs/requirements.md](docs/requirements.md) — 完整架构设计、协议规范、安全模型与演进路线
- **VPS 部署与主题定制**：[docs/deployment.md](docs/deployment.md) — `data/` 目录规划、`systemd` 服务、Caddy HTTPS 反代与 `data/theme/` 热覆写
- **AI 工具开发与发布 Skill**：[docs/ai-tool-skill.md](docs/ai-tool-skill.md) — 可直接提供给 AI / Agent 的微工具制作、打包与 Bearer API 发布规范
- **变更日志**：[CHANGELOG.md](CHANGELOG.md) — 各版本更新记录

## 开发与测试

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

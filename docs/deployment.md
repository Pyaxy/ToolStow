# VPS 部署与自定义主题指南

ToolStow 采用**“单二进制 + 统一 `data/` 数据目录 + `0600` 环境文件”**的极简 VPS 部署模型，无外部数据库依赖，备份或迁移只需打包 `data/` 目录。

---

## 1. 推荐服务器目录结构

```text
/opt/toolstow/
├── toolstow              # cargo build --release 产出的单一可执行文件
├── .env                  # 权限 0600，仅存放密钥与运行参数
└── data/                 # 全部持久化状态（备份只需打包此目录）
    ├── tools/            # 已安装的微工具目录（含内部暂存 .staging/）
    ├── themes/           # 多主题目录（每个子目录 <short>/ 含 theme.json；含内部 .staging/）
    ├── theme/            # （可选，向后兼容）activeTheme="default" 时的单目录覆盖
    └── settings.json     # 站点名称、简介、默认强调色与当前启用主题 activeTheme
```

---

## 2. 配置 `.env` 与双密钥

```bash
mkdir -p /opt/toolstow/data
cp .env.example /opt/toolstow/.env
chmod 600 /opt/toolstow/.env
```

编辑 `/opt/toolstow/.env`（建议用 `openssl rand -hex 32` 分别生成两把完全不同的密钥）：

```dotenv
TOOLSTOW_HOST=127.0.0.1
TOOLSTOW_PORT=8080
TOOLSTOW_DATA_DIR=/opt/toolstow/data
TOOLSTOW_COOKIE_SECURE=true

# 人类管理员在 /admin 输入的密钥（切勿写入 AI 提示词）
TOOLSTOW_ADMIN_KEY=<你的 64 位随机十六进制 Admin Key>

# AI / 脚本调用 /api/admin/tools* 使用的 Bearer Token（可随时独立轮换）
TOOLSTOW_API_KEY=<另一把 64 位随机十六进制 API Key>
```

> **纯只读模式**：如果不填写 `TOOLSTOW_ADMIN_KEY` 与 `TOOLSTOW_API_KEY`，`/admin` 与所有 `/api/admin/*` 接口将自动返回 `404`，可通过 SSH / `rsync` / `git pull` 直接维护 `data/tools/` 触发热加载。

---

## 3. `systemd` 常驻服务示例

创建 `/etc/systemd/system/toolstow.service`：

```ini
[Unit]
Description=ToolStow Personal Micro-Tools Host
After=network.target

[Service]
Type=simple
WorkingDirectory=/opt/toolstow
ExecStart=/opt/toolstow/toolstow
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
```

启用并启动服务：

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now toolstow
sudo journalctl -u toolstow -f
```

---

## 4. Caddy HTTPS 反向代理示例

ToolStow 默认仅监听 `127.0.0.1:8080`，建议前置 Caddy 自动签发 TLS 证书并透传 `X-Forwarded-Proto`：

```caddyfile
tools.example.com {
    reverse_proxy 127.0.0.1:8080
}
```

- Caddy 默认会透传 `X-Forwarded-Proto: https` 和 `Host`，ToolStow 会自动为管理员 Session Cookie 附加 `Secure` 属性（同时配合 `.env` 中的 `TOOLSTOW_COOKIE_SECURE=true` 双保险）。
- 如需为 `/admin` 叠加额外访问控制（如 IP 白名单或 Zero Trust / OAuth），可直接对 `/admin*` 与 `/api/admin/*` 路径规则加保护，无需修改 ToolStow 源码。

---

## 5. 多主题管理与自定义皮肤（`data/themes/` 与 `/admin`）

ToolStow 支持借鉴 [monitor-probe/themes](https://github.com/monitor-probe/themes) 思路的**多主题热插拔管理**：管理员可在 `/admin` 的「主题管理」卡片直接上传 `.zip` 主题包、查看 16:9 预览图、一键切换当前启用主题或删除主题，全程**无需重启或重编二进制**。

### 5.1 多主题目录布局（`data/themes/<short>/`）

```text
/opt/toolstow/data/themes/
├── .staging/                  # 内部同盘原子解包暂存目录（自动忽略）
├── ascii-probe/               # 完整主题 + ui.css（自定义前台壳 + 统一样式覆盖）
│   ├── theme.json
│   ├── preview.svg
│   ├── ui.css
│   ├── index.html
│   ├── style.css
│   └── app.js
├── serverstatus/              # 完整主题 + ui.css（高密度运维表格视图）
│   ├── theme.json
│   ├── preview.svg
│   ├── ui.css
│   ├── index.html
│   ├── style.css
│   └── app.js
├── rakugaki/                  # 完整主题 + ui.css + assets/（手绘涂鸦笔记本）
│   ├── theme.json
│   ├── preview.svg
│   ├── ui.css
│   ├── index.html
│   ├── style.css
│   ├── app.js
│   └── assets/
│       ├── paper-grid-light.svg
│       └── paper-grid-dark.svg
└── paper-warm/                # 纯皮肤主题（仅 ui.css，无 index.html）
    ├── theme.json
    ├── preview.svg
    └── ui.css
```

> 可通过 CLI `--themes-dir` 或环境变量 `TOOLSTOW_THEMES_DIR` 覆盖默认 `<data-dir>/themes` 路径。当前激活主题持久化在 `data/settings.json` 的 `activeTheme` 字段（默认 `"default"`）。

### 5.2 `theme.json` 规范与两种主题形态

每个主题根目录必须包含 `theme.json`：

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

- `short`（必填）：主题唯一短名，匹配 `^[a-z0-9]+(?:-[a-z0-9]+)*$`（1–64 字符），禁止使用保留名 `"default"`，落盘目录名与 `short` 严格一致。
- `name`（必填）：主题展示名称（去首尾空白后非空）。
- `version` / `description` / `author`（可选）：展示用元信息。
- `url`（可选）：留空或合法 `http://` / `https://` 链接。
- `preview`（可选）：主题包内安全相对路径预览图（如 `preview.svg` / `preview.png`，禁止符号链接与路径穿越），在 `/admin` 通过 `GET /api/admin/themes/{short}/preview`（附带 `Content-Security-Policy: sandbox;` 与 `X-Content-Type-Options: nosniff`）安全展示。
- **内容硬约束**：主题包内必须至少包含 `index.html`（或构建产物 `dist/index.html`）或 `ui.css` 之一，否则校验返回 `422 Unprocessable Entity`。

两种形态：
1. **纯皮肤覆盖（Skin-only：仅 `ui.css`，不含 `index.html`）**：
   - 公开前台继续复用编译期内嵌的默认壳界面，后端在响应 `GET /ui/v1/toolstow.css` 时自动先输出基础样式、再追加该主题的 `ui.css`，一次覆盖 `--ts-*` 设计令牌与 `.ts-*` 原语，让**主站与全部插件同步换肤**。
2. **完整自定义前台（Full Custom Shell：`index.html` 或 `dist/index.html`，可选配 `ui.css`）**：
   - 公开前台 `/` 直接托管该主题静态文件（支持原生静态页或 Vite / React / Tailwind 构建后的 `dist/` 目录）；若同时包含 `ui.css`，也会自动追加至 `/ui/v1/toolstow.css` 使子工具与外壳保持视觉统一。
   - 自定义前台需遵循只读契约：先加载 `/ui/v1/toolstow.css`，请求 `GET /api/me` 仅当 `isAdmin === true` 时显示 `/admin` 管理入口，请求 `GET /api/tools` 并保持 `<iframe sandbox>` 安全白名单（永不授予 `allow-same-origin`）与 `toolstow:theme` / `toolstow:ready` 握手。

### 5.3 `/admin` 主题管理操作与 API（仅 Admin Session）

- **网页管理（`/admin`）**：登录后台后在「主题管理」卡片拖拽或选择 `.zip` 主题包上传。前端默认先携带 `?overwrite=false` 预检；若同 `short` 主题已存在，后端返回 `409 Conflict`（`THEME_EXISTS`，含 `existing` 与 `incoming` 对比元数据），弹窗经管理员确认后再携带 `?overwrite=true` 原子覆盖。点击卡片上的「启用主题」立即切换，点击「删除」删除磁盘目录（若删除的是当前启用主题，后端自动安全回退至内置 `"default"` 主题）。
- **HTTP 接口（仅允许 Admin Session Cookie，严格拒绝 `TOOLSTOW_API_KEY` Bearer，未配置 `TOOLSTOW_ADMIN_KEY` 时返回 `404`）**：
  - `GET /api/admin/themes`：列出内置 `default` 与所有已安装主题。
  - `GET /api/admin/themes/{short}/preview`：返回主题预览图（`CSP: sandbox;` + `nosniff`）。
  - `POST /api/admin/themes/upload`：上传 `.zip` 主题包（`multipart/form-data` 字段 `file`，支持 `?overwrite=false|true`）。
  - `PUT /api/admin/themes/active`：JSON `{ "short": "<short>" }` 切换当前启用主题并持久化 `settings.json`。
  - `DELETE /api/admin/themes/{short}`：删除已安装主题（拒绝删除 `"default"` 返回 `400`）。

### 5.4 快速体验仓库内置 4 套示例主题

仓库 `examples/themes/` 随附 4 套开箱即用的主题包：
- `examples/themes/ascii-probe/`：复古终端 TUI 完整主题 + `ui.css`（等宽排版、2px 框线、荧光绿强调色）。
- `examples/themes/serverstatus/`：高密度运维表格控制台完整主题 + `ui.css`（紧凑筛选条、表格视图、岩板蓝配色）。
- `examples/themes/rakugaki/`：手绘涂鸦笔记本完整主题 + `ui.css` + `assets/paper-grid-*.svg`（通过 `/ui/v1/theme-assets/` 让主站与所有插件共享网格纸/黑板底纹、不规则手绘墨线框与硬投影）。
- `examples/themes/paper-warm/`：温润羊皮纸社论风格纯皮肤包（仅 `theme.json` + `ui.css` + `preview.svg`，无 `index.html`）。

可直接拷贝至服务器 `data/themes/` 或打包 `.zip` 在 `/admin` 上传：

```bash
# 方式 A：直接拷贝目录到 data/themes/，刷新 /admin 即可切换
cp -r examples/themes/ascii-probe examples/themes/serverstatus examples/themes/rakugaki examples/themes/paper-warm /opt/toolstow/data/themes/

# 方式 B：打包为 .zip 后在 /admin「主题管理」拖拽上传
(cd examples/themes/rakugaki && zip -r /tmp/rakugaki.zip .)
```

### 5.5 路由安全锁定、`/ui/v1/theme-assets/` 与旧版 `data/theme/` 向后兼容

- `/admin*`、`/api/*`、`/t/*` 与 `/ui/*`（`/ui/v1/toolstow.css`、`/ui/v1/theme.js`、`/ui/v1/theme-assets/{*path}`，其余未知 `/ui/*` 路径一律返回 `404`）始终由 Rust 后端强制接管，外置主题无法覆盖管理面板、共享 SDK 或绕过沙箱边界。
- **跨沙箱主题静态资源（`GET /ui/v1/theme-assets/{*path}`）**：无论当前启用的是**纯皮肤主题**还是**完整自定义前台**（或 `default` 下的旧版 `data/theme/`），主题内自带的 `.woff2` 字体、背景图与 SVG 纹理均可通过 `/ui/v1/theme-assets/<rel-path>` 加载；后端固定返回 `Access-Control-Allow-Origin: *`、`Cache-Control: no-cache`、`X-Content-Type-Options: nosniff`（对 `.svg`/`.html` 额外附带 `Content-Security-Policy: sandbox;`），并拒绝点开头路径、越界路径、`theme.json` 与符号链接逃逸（`403 Forbidden`），确保字体与纹理在公开首页 `/` 与 `Origin: null` 沙箱插件 iframe 内一致生效。
- 后端对主题静态文件与 `ui.css` 均执行规范路径 Containment 校验，禁止通过符号链接逃逸出主题目录（越界返回 `403 Forbidden`）。
- **向后兼容**：当 `activeTheme` 为 `"default"` 时，仍保留旧版单目录 `/opt/toolstow/data/theme/`（`index.html` / `ui.css`）的自动覆盖逻辑；切换至任意已安装命名主题后，不再混入旧版 `data/theme/ui.css`。

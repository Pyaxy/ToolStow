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
    ├── theme/            # （可选）自定义前台静态文件（index.html）及共享样式覆盖（ui.css）
    └── settings.json     # 站点名称、简介与默认强调色（自动创建/更新）
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

## 5. 热覆盖公开前台与全局样式（`data/theme/`）

ToolStow 提供两档零重编自定义能力，可单独使用或组合使用：

### 5.1 仅换肤（推荐）：`data/theme/ui.css` 级联覆盖

ToolStow 在锁定路径 `/ui/v1/toolstow.css` 与 `/ui/v1/theme.js` 下提供共享设计系统，默认公开前台、`/admin` 管理台与所有原生微工具插件均加载 `/ui/v1/toolstow.css`。

只需创建 `/opt/toolstow/data/theme/ui.css` 覆盖 `--ts-*` 设计令牌或 `.ts-*` 组件样式，后端在响应 `GET /ui/v1/toolstow.css` 时会自动先输出基础样式、再追加拼接 `data/theme/ui.css`，无需修改任何单个插件即可让**主站与全部工具同步换肤**：

```css
/* /opt/toolstow/data/theme/ui.css */
:root {
  --ts-canvas: #fbfaf8;
  --ts-surface: #f4f1ec;
  --ts-border: #e2dcd3;
  --ts-accent: #0f766e;
  --ts-radius-sm: 4px;
  --ts-radius-md: 6px;
}

:root[data-theme="dark"] {
  --ts-canvas: #0c0d10;
  --ts-surface: #14161c;
  --ts-accent: #2dd4bf;
}
```

- 无需重启或重编 ToolStow，保存文件后刷新浏览器立即生效；删除 `data/theme/ui.css` 即瞬间恢复默认极简黑白主题。
- 后端对 `data/theme/ui.css` 执行规范路径检查，禁止通过符号链接指向 `data/theme/` 之外的文件（越界返回 `403 Forbidden`）。
- 仅放置 `data/theme/ui.css`（不含 `index.html`）时，公开前台仍使用内嵌壳界面，仅样式层被级联覆盖。

### 5.2 完整替换公开前台：`data/theme/index.html`

想完全替换默认的公开前台结构与交互（无论是原生静态页面，还是 Vite / React / Tailwind 打包的 `dist/` 产物），将静态文件放入 `data/theme/` 并保证根目录包含 `index.html`：

```text
/opt/toolstow/data/theme/
├── index.html
├── ui.css                # （可选）同时级联覆盖 /ui/v1/toolstow.css
└── assets/
    ├── app.js
    └── style.css
```

- 无需重新编译或重启 ToolStow，访问 `/` 立即生效，且 `GET /api/me` 会返回 `"themeSource": "custom"`。
- 自定义主题只需按只读契约调用 `GET /api/me`、`GET /api/tools` 与 `/t/{id}/{entry}`，也可直接 `<link rel="stylesheet" href="/ui/v1/toolstow.css">` 复用共享样式变量。
- `/admin*`、`/api/*`、`/t/*` 与 `/ui/*`（`/ui/v1/toolstow.css`、`/ui/v1/theme.js`，其余未知 `/ui/*` 路径一律返回 `404`）始终由 Rust 后端强制接管，外置主题无法覆盖管理面板、共享 SDK 或绕过沙箱边界。
- 删除或移走 `data/theme/index.html` 即可瞬间回退至二进制内嵌的默认极简黑白主题。

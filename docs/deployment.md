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
    ├── theme/            # （可选）自定义公开前台静态文件，存在 index.html 即生效
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

## 5. 热覆盖公开前台主题（`data/theme/`）

想替换默认的公开前台风格（无论是原生静态页面，还是 Vite / React / Tailwind 打包的 `dist/` 产物），只需将静态文件放入 `data/theme/` 并保证根目录包含 `index.html`：

```text
/opt/toolstow/data/theme/
├── index.html
└── assets/
    ├── app.js
    └── style.css
```

- 无需重新编译或重启 ToolStow，访问 `/` 立即生效，且 `GET /api/me` 会返回 `"themeSource": "custom"`。
- 自定义主题只需按只读契约调用 `GET /api/me`、`GET /api/tools` 与 `/t/{id}/{entry}`。
- `/admin*`、`/api/*`、`/t/*` 始终由 Rust 后端强制接管，外置主题无法覆盖管理面板或绕过沙箱边界。
- 删除或移走 `data/theme/index.html` 即可瞬间回退至二进制内嵌的默认极简黑白主题。

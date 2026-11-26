# ToolStow

> 把平时用 AI 写的好用小网页，统统收进自己的个人工具箱——点开即用、随手换肤、一个程序或一个 Docker 容器直接跑起来。

---

## 为什么用 ToolStow？

和 AI 对话时，经常会随手生成各种小网页工具：命令速查表、格式转换器、图标生成器、日期计算器……它们常常散落在聊天记录和临时文件夹里，下次要用时找不到。ToolStow 帮你把它们收在一个地方：

- **随手放进来就能用**：支持在网页里直接拖拽 `.html` 单文件或 `.zip` 包、粘贴网页代码，也可以让 AI 写完后自动帮您发布上线。
- **开箱即用、更好备份**：不需要额外装数据库，只有一个可执行文件（或一个 Docker 容器），所有工具、主题和设置都存在同一个 `data/` 文件夹里。
- **一键切换全站主题**：内置 5 套不同风格的示例主题，切换后首页和工具界面会同步换肤。
- **安全省心**：不配置密码就是纯展示模式，完全关闭管理入口；每个工具都在独立隔离环境中运行，互不干扰。

---

## 快速开始

### 方式一：使用 Docker（推荐）

准备仓库里的 [compose.yml](compose.yml)（如需开启后台管理，把 [.env.example](.env.example) 复制为 `.env` 并填入自己的管理密码）：

```bash
docker compose up -d
```

或者直接用 `docker run` 启动：

```bash
docker run -d \
  --name toolstow \
  --restart unless-stopped \
  -p 127.0.0.1:8080:8080 \
  -v toolstow_data:/data \
  ghcr.io/pyaxy/toolstow:latest
```

### 方式二：下载单文件直接运行

前往 [GitHub Releases](https://github.com/Pyaxy/ToolStow/releases) 下载对应系统的压缩包：

| 系统与架构 | 下载文件名 |
| :--- | :--- |
| Linux x64 | `toolstow-vX.Y.Z-linux-amd64.tar.gz` |
| Linux ARM64 | `toolstow-vX.Y.Z-linux-arm64.tar.gz` |
| macOS Apple Silicon | `toolstow-vX.Y.Z-macos-arm64.tar.gz` |
| Windows x64 | `toolstow-vX.Y.Z-windows-amd64.zip` |

解压后直接运行（默认打开 `http://127.0.0.1:8080`）：

```bash
# Linux / macOS
tar -xzf toolstow-vX.Y.Z-linux-amd64.tar.gz
./toolstow

# Windows (PowerShell)：解压后直接双击或运行 .\toolstow.exe
```

### 开启管理后台（可选）

如果希望在网页里上传工具、更换主题或设置站点标题，需要配置管理密码；不配置则只展示公开工具，后台管理页默认关闭。

```bash
cp .env.example .env
# 生成随机密码并填入 .env 中的 TOOLSTOW_ADMIN_KEY
openssl rand -hex 32
chmod 600 .env
```

重新启动后即可访问：
- **工具箱首页**：`http://127.0.0.1:8080`
- **管理后台**：`http://127.0.0.1:8080/admin`（登录后首页右上角也会出现入口）

> 如果你希望让 AI 自动帮你发布做好的工具，可在 `.env` 里再填一把不同的 `TOOLSTOW_API_KEY`。

---

## 放进工具与更换主题

- **体验示例包**：Releases 页同时提供两个合集包，解压后把里面的 `.zip` 拖进 `/admin` 即可体验：
  - `toolstow-plugins.zip`：内置示例工具（Docker 命令速查、动态 Favicon 生成器）；
  - `toolstow-themes.zip`：内置 5 套示例主题（`ascii-probe` 复古终端、`serverstatus` 运维表格、`rakugaki` 手绘涂鸦、`phosphor` 荧光暗黑终端、`paper-warm` 温润羊皮纸）。
- **收录自己的小网页**：登录 `/admin` →「工具管理」，直接拖入 `.html` / `.zip` 文件或粘贴 HTML 代码即可；也可以把 **[skills/toolstow-tool/](skills/toolstow-tool/SKILL.md)** 交给 AI，直接对它说需求（例如 *“帮我做一个日期计算工具并发布”*）。
- **定制新主题**：在 `/admin` →「主题管理」上传主题 `.zip` 并一键切换；或者把 **[skills/toolstow-theme/](skills/toolstow-theme/SKILL.md)** 交给 AI 帮你设计整套皮肤或页面风格。

---

## 更多文档

- **服务器部署、自动更新与反向代理**：[docs/deployment.md](docs/deployment.md)
- **技术架构、UI SDK、沙箱安全与源码编译校验**：[docs/architecture.md](docs/architecture.md)
- **工具开发规范（AI Skill）**：[skills/toolstow-tool/SKILL.md](skills/toolstow-tool/SKILL.md)
- **主题制作规范（AI Skill）**：[skills/toolstow-theme/SKILL.md](skills/toolstow-theme/SKILL.md)
- **完整需求规格书**：[docs/requirements.md](docs/requirements.md)
- **版本更新记录**：[CHANGELOG.md](CHANGELOG.md)

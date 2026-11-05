# multinteractor

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](./LICENSE)
[![Rust](https://img.shields.io/badge/Client-Rust_Workspace-orange.svg)](./client)
[![Python](https://img.shields.io/badge/Server-Python_3.11%2B-3776AB.svg)](./server)
[![Agent Skill](https://img.shields.io/badge/Agent_Skill-Native_CLI-success.svg)](./.agents/skills/multinteractor/SKILL.md)

**multinteractor** 是一套专为多设备协同办公、跨机研发与 AI 智能体（Agent）自动化设计的**跨设备云端协同与剪贴板/文件同步系统**。

系统采用 **Python 单节点云端中枢（Cloud Hub） + Rust 常驻客户端守护进程（Client Daemon） + 原生 Agent Skill** 架构，支持通过全局快捷键（`Alt+C` / `Alt+V`）、系统鼠标右键菜单（`【同步到... >】`）以及命令行/智能体技能（`mi` CLI）在多台计算机之间无缝广播、拉取与定向同步文本、图片、文件及多层级文件夹。

---

## 核心特性

- **三端多维触发（快捷键 + 右键菜单 + AI 智能体）**
  - **`Alt+C` / `Alt+V` 全局热键**：`Alt+C` 自动识别当前活动窗口选中的文本、图片或资源管理器（Explorer / Finder）选中的文件/目录并向订阅设备广播；`Alt+V` 根据当前焦点窗口环境智能路由（在编辑器内直接注入文本/图片，在文件管理器中直接落盘文件/文件夹，或弹出另存为对话框）。
  - **鼠标右键 `【同步到... >】` 定向同步**：按需动态拉取目标机器的实时目录树（支持懒加载与远程新建文件夹），将本地文件或文件夹直接静默同步至目标设备指定目录。
  - **AI Agent 原生技能（Skill）**：内置 [`multinteractor` Skill](./.agents/skills/multinteractor/SKILL.md) 与结构化 CLI 工具 `mi`（全命令支持 `--json` 输出），赋能 AI 智能体自主完成跨机文件分发、目录探查与剪贴板协同。
- **内网即用即连（1-to-N 并发 + 180 秒空闲自动释放）**
  - 内网设备间平时不维持网状常驻连接（零空闲端口占用与心跳开销）；触发文件传输或远程目录浏览时，基于云端同步的内网地址**即用即连**建立高速双向通道，支持一对多（`1-to-N`）多机并发传输，并在**空闲满 180 秒（可配置）无数据交互后自动断开释放**。
- **云端“边传边下（Stream Teeing）”流式缓存**
  - 跨网段或内网不可达时自动无缝切换至云端中转通道；源设备分块上传的同时，云端支持一写多读实时流式分流（Stream Teeing），接收端无需等待源端全部上传完毕即可并发流式拉取。
- **全链路 BLAKE3 摘要校验与跨通道断点续传**
  - 所有传输数据强制执行 **全文 + `4 MB` 分块 `BLAKE3` 哈希校验**；支持基于 `Upload-Offset` 与 HTTP `Range` 的双向断点续传，即使内网直连中断切换至云端缓存池，也能从已校验分块偏移量处无缝续传，杜绝残损文件落盘。
- **全链路 SQLite WAL 审计日志**
  - 云端单节点服务内置轻量级 SQLite（WAL 模式）审计引擎，完整记录设备连接鉴权、事件广播投递、文件分块传输及摘要校验回执。

---

## 系统架构总览

```mermaid
flowchart TB
    subgraph TriggerLayer["智能体与用户触发层"]
        Skill[".agents/skills/multinteractor/SKILL.md"]
        Hotkey["全局快捷键 Alt+C / Alt+V"]
        CtxMenu["系统右键菜单【同步到... >】"]
    end

    subgraph ClientWorkspace["client/ (Rust 客户端工作区)"]
        CLI["crates/mi-cli<br/>(mi 命令行工具)"]
        OS["crates/mi-os<br/>(热键 / 焦点窗口探测 / 右键扩展)"]
        UI["crates/mi-ui<br/>(远程目录树选择弹窗)"]
        Daemon["crates/mi-daemon<br/>(常驻服务 / 1-to-N 内网通道 / 断点续传引擎)"]
        Proto["crates/mi-protocol<br/>(CloudEventEnvelope v6 / BLAKE3 摘要清单)"]
    end

    subgraph CloudServer["server/ (Python 云端中心服务)"]
        SrvMain["app/main.py<br/>(API 与 WebSocket 信令网关)"]
        SrvSub["app/client_sub_manager.py<br/>(客户端准入与 Topic 订阅管理)"]
        SrvCache["app/stream_tee_cache.py<br/>(边传边下与 Upload-Offset/Range 缓存池)"]
        SrvAudit["app/audit_logger.py<br/>(SQLite WAL 全链路审计日志)"]
    end

    Skill -->|Shell 调用| CLI
    Hotkey --> OS
    CtxMenu --> OS
    CtxMenu --> UI

    CLI -->|HTTP IPC 127.0.0.1:53318| Daemon
    OS -->|HTTP IPC 127.0.0.1:53318| Daemon
    UI -->|HTTP IPC 127.0.0.1:53318| Daemon
    Daemon -->|核心依赖| Proto

    Daemon <==>|1. WSS 常驻信令 & HTTP Range 断点续传| SrvMain
    Daemon <-.->|2. 内网即用即连 1-to-N 通道 (180s 空闲自动断开)| Daemon
    SrvMain --> SrvSub
    SrvMain --> SrvCache
    SrvMain --> SrvAudit
```

---

## 仓库目录结构

```text
multinteractor/
├── .agents/skills/multinteractor/SKILL.md   # AI 智能体跨机协同标准技能定义
├── server/                                  # Python 云端中心服务 (信令网关 + 边传边下缓存 + SQLite 审计)
│   ├── app/                                 # FastAPI 应用、订阅管理、Stream Teeing 引擎与审计模块
│   └── tests/                               # 服务端单元测试
├── client/                                  # Rust 客户端 Cargo Workspace
│   └── crates/
│       ├── mi-protocol/                     # 跨端协议契约 (CloudEventEnvelope v6) 与 BLAKE3 分块摘要库
│       ├── mi-daemon/                       # 本地常驻守护服务、1-to-N 内网通道池、断点续传与 .tar.zst 引擎
│       ├── mi-os/                           # OS 原生集成 (Alt+C/Alt+V 热键、焦点窗口识别、右键菜单、防休眠锁)
│       ├── mi-cli/                          # 命令行工具 `mi` (供终端用户与 Agent Skill 调用)
│       └── mi-ui/                           # 轻量级远程目录树选择弹窗与传输进度提示
├── tests/                                   # L1~L4 全链路测试套件 (契约夹具 / 集成 / 混沌故障注入 / E2E 验收)
├── deploy/                                  # Docker / Systemd / Windows 服务与右键菜单 / macOS LaunchAgent 部署模板
├── Makefile                                 # 统一构建、开发启动与测试入口
├── REQUIREMENTS.md                          # 需求规格说明书 (PRD & SRS v6)
├── PROJECT_STRUCTURE.md                     # 项目架构与模块边界规范
├── IMPLEMENTATION_PLAN.md                   # 分阶段工程实施计划
├── TEST_AND_ACCEPTANCE.md                   # 测试金字塔与验收标准说明书
└── LICENSE                                  # Apache-2.0 开源许可证
```

---

## 让 AI 帮你安装（推荐）

本项目针对 AI 编程助手（如 **Antigravity**、**Claude Code**、**Cursor**、**Windsurf**、**Cline** 等）进行了原生适配。你可以直接复制以下提示词发送给你的 AI 助手，由 AI 自动完成环境检查、编译安装、开机自启配置及 Skill 技能注册：

### 场景一：在本机安装客户端、CLI 工具与 Agent Skill（日常使用设备）

将以下指令复制给 AI 助手（请按需替换 `<云端服务器地址>` 与 `<集群密钥>`）：

```text
请帮我在本机编译并安装 multinteractor 客户端与 AI 技能，按以下步骤执行并验证：
1. 检查本机 Rust 工具链（若未安装请提示或协助安装 stable 工具链），在仓库根目录下执行 `cd client && cargo build --release --workspace` 编译 Release 产物。
2. 将编译生成的 `mi-daemon`、`mi`、`mi-ui` 二进制文件安装到系统 PATH 目录（macOS/Linux 复制到 `/usr/local/bin/` 或 `~/.local/bin/`；Windows 复制到 `%LOCALAPPDATA%\Programs\multinteractor\` 并加入用户 PATH）。
3. 配置云端连接参数（服务端地址：`http://<云端服务器IP>:8000`，预共享密钥：`MI_CLUSTER_TOKEN=<集群密钥>`），并配置后台常驻自启动：
   - 若为 macOS：使用 `deploy/client/macos/com.multinteractor.daemon.plist` 配置到 `~/Library/LaunchAgents/` 并通过 `launchctl` 启动。
   - 若为 Windows：执行 `deploy/client/windows/install_service_and_menu.ps1` 注册开机自启与资源管理器右键菜单【同步到... >】。
4. 将 `.agents/skills/multinteractor` 软链接或复制到当前 AI 助手的全局/工作区 Skills 目录中。
5. 运行 `mi devices list --json` 验证本地 `mi-daemon` (127.0.0.1:53318) 是否正常响应并已连接至云端服务。
```

### 场景二：在服务器一键部署云端中心服务（Cloud Hub）

登录或让 AI 连接到你的个人云服务器后，发送以下指令：

```text
请帮我部署 multinteractor 云端中心服务（server/）：
1. 若本机已安装 Docker 与 Docker Compose：
   - 使用 `deploy/server/docker-compose.yml` 构建并启动容器服务（`docker compose -f deploy/server/docker-compose.yml up -d --build`）。
2. 若使用宿主机 Python 环境（需 Python >= 3.11）：
   - 在 `server/` 下创建虚拟环境并安装依赖 `pip install -e .`。
   - 参考 `deploy/server/multinteractor-hub.service` 配置 Systemd 守护进程并启动（或使用 `make server-dev` 启动）。
3. 检查 `8000` 端口监听状态，确认服务健康运行并告知我客户端连接所需的服务端地址与 Token 配置。
```

### 场景三：全栈本地联调与自动化验收（开发者 / 贡献者）

```text
请读取仓库根目录的 Makefile 与 PROJECT_STRUCTURE.md，帮我完成 multinteractor 全栈开发环境初始化与自检：
1. 检查 Python >= 3.11 与 Rust Cargo 环境。
2. 运行 `make client-check` 与 `make server-test` 验证客户端与服务端基础模块。
3. 运行 `make test-all` 执行 L1~L4 全链路测试套件（含协议黄金样本对齐、Stream Teeing 边传边下、断网断点续传、180s 空闲断连与 CLI/Skill 端到端测试），并向我汇报测试结果。
```

---

## 快速开始（手动安装）

### 1. 环境依赖

- **云端服务端 (`server/`)**：Python `>= 3.11`（推荐使用 `uvicorn` + `fastapi`）
- **客户端工作区 (`client/`)**：Rust `stable` 工具链（Cargo Workspace）

### 2. 启动云端中心服务

使用 `Makefile` 一键启动本地开发服务端（默认监听 `0.0.0.0:8000`）：

```bash
make server-dev
```

或使用 Docker Compose 部署（详见 [`deploy/server/docker-compose.yml`](./deploy/server/docker-compose.yml)）：

```bash
docker compose -f deploy/server/docker-compose.yml up -d
```

可通过环境变量自定义服务端配置：

| 环境变量 | 默认值 | 说明 |
| :--- | :--- | :--- |
| `MI_SERVER_HOST` | `0.0.0.0` | 服务端监听地址 |
| `MI_SERVER_PORT` | `8000` | 服务端监听端口 |
| `MI_CLUSTER_TOKEN` | `dev-secret-token` | 设备注册准入预共享密钥（PSK / Token） |
| `MI_CACHE_DIR` | `./.mi_cache` | 云端流式缓存池落盘目录 |
| `MI_CACHE_TTL_SECONDS` | `86400` | 缓存文件过期清理时间（默认 24 小时） |
| `MI_CACHE_MAX_BYTES` | `10737418240` | 云端缓存池最大容量配额（默认 10 GB） |

### 3. 编译与运行客户端

编译客户端常驻守护进程 `mi-daemon` 与命令行工具 `mi`：

```bash
make client-build
```

启动本地客户端守护服务（默认监听本地回环 IPC `127.0.0.1:53318`）：

```bash
./client/target/debug/mi-daemon
```

> 生产环境自启动配置与系统右键菜单安装脚本请参考 [`deploy/client/`](./deploy/client/) 下的 Windows PowerShell 脚本与 macOS LaunchAgent 配置。

---

## CLI 与 AI 智能体技能（Agent Skill）使用指南

客户端提供 `mi` 命令行工具，所有子命令均支持 `--json` 结构化输出，既可直接用于终端脚本，也可由支持 Skill 的 AI 智能体（通过 [`.agents/skills/multinteractor/SKILL.md`](./.agents/skills/multinteractor/SKILL.md)）自动调用：

### 常用命令速查

| 场景分类 | 命令示例 | 说明 |
| :--- | :--- | :--- |
| **查询在线设备** | `mi devices list --json` | 查看当前已注册设备的在线状态与内网可达性 |
| **查看待粘贴事件** | `mi events list --limit 10 --json` | 查看本机接收事件缓存列表（`Event Cache List`） |
| **广播文本 (`Alt+C`)** | `mi copy --text "Hello multinteractor" --json` | 计算 `BLAKE3` 摘要后向所有订阅设备广播文本 |
| **广播文件/目录 (`Alt+C`)** | `mi copy --path "./report.pdf" --json` | 计算全文与分块摘要并触发广播及流式缓存上传 |
| **智能粘贴 (`Alt+V`)** | `mi paste --dest "./downloads" --json` | 将最新（或指定 `--event-id`）事件粘贴或落盘至目标目录 |
| **远程目录浏览** | `mi remote-ls --device "pc-b" --path "ROOT" --json` | 按需动态拉取目标机器盘符与目录树结构 |
| **远程新建文件夹** | `mi remote-mkdir --device "pc-b" --path "D:\\Work\\Docs" --json` | 在目标机器指定路径下创建新文件夹 |
| **定向同步 (`【同步到...】`)** | `mi sync --source "./build" --device "pc-b" --remote-dir "D:\\Work\\Docs" --json` | 将本地文件/目录直接同步至目标机器指定目录（含摘要校验与断点续传） |
| **查询/恢复传输任务** | `mi transfer resume --task-id "<transfer_id>" --json` | 查询大文件传输进度或从已校验分块偏移量处恢复中断的传输 |

---

## 测试与质量验证

项目内置完整的 **L1（契约与单元测试）~ L4（端到端与混沌故障注入测试）** 测试金字塔，可通过 `Makefile` 直接运行：

```bash
# 运行服务端单元测试
make server-test

# 检查并运行 Rust 客户端单元测试
make client-test

# 运行 L1~L4 全量单元、集成、边传边下、混沌断网续传、180s 空闲断连与端到端验收测试
make test-all
```

更多测试设计与验收指标请参考 [`TEST_AND_ACCEPTANCE.md`](./TEST_AND_ACCEPTANCE.md)。

---

## 架构与工程文档索引

- **[需求规格说明书 (`REQUIREMENTS.md`)](./REQUIREMENTS.md)**：完整的业务背景、系统架构、协议规范与功能/非功能性需求定义。
- **[项目目录结构规范 (`PROJECT_STRUCTURE.md`)](./PROJECT_STRUCTURE.md)**：Monorepo 模块职责边界、单向依赖规则与子系统详解。
- **[工程落地计划 (`IMPLEMENTATION_PLAN.md`)](./IMPLEMENTATION_PLAN.md)**：任务拆解、依赖拓扑与分阶段交付路线图。
- **[测试与验收规格说明书 (`TEST_AND_ACCEPTANCE.md`)](./TEST_AND_ACCEPTANCE.md)**：测试金字塔、混沌测试场景与验收准则。
- **[AI 智能体技能定义 (`SKILL.md`)](./.agents/skills/multinteractor/SKILL.md)**：智能体跨设备协同标准工作流与异常处理手册。

---

## 开源协议 (License)

本项目基于 **[Apache License 2.0](./LICENSE)** 开源协议发布。详情请参阅 [`LICENSE`](./LICENSE) 文件。

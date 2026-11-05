---
name: multinteractor
description: >-
  跨设备数据协同技能（multinteractor）。当用户需要向关联的其他电脑广播复制内容（文本、图片、文件、文件夹）、
  从接收事件列表中粘贴/保存跨机数据、动态浏览目标机器的文件目录结构、在目标机器新建文件夹、或直接将本地文本/文件/文件夹定向同步到目标机器指定目录（含摘要校验与断点续传）时使用此技能。
---

# multinteractor 跨设备数据协同技能

本技能通过调用本地常驻 Rust 客户端提供的 CLI 工具 `mi`（或本地 IPC 接口 `http://127.0.0.1:53318/api/v1`），实现多台设备间的文本、图片、文件及文件夹广播、拉取、远程目录浏览与定向目录同步。所有命令均通过 `--json` 参数返回结构化 JSON，且底层自动执行 `BLAKE3` 摘要校验与断点续传。

---

## 1. 核心命令速查表

| 场景分类 | 命令模板 | 说明 |
| :--- | :--- | :--- |
| **查询在线设备** | `mi devices list --json` | 查看当前注册到 Python 云端服务的设备列表、在线状态及内网可达性 |
| **查看待粘贴事件** | `mi events list --limit 10 --json` | 查看本机接收事件缓存列表（`Event Cache List`）中的跨端复制记录 |
| **广播文本 (`Alt+C`)** | `mi copy --text "<文本内容>" --json` | 将文本计算 `BLAKE3` 摘要后广播给所有订阅设备 |
| **广播文件/目录 (`Alt+C`)** | `mi copy --path "<本地文件或目录路径>" --json` | 将文件/文件夹计算摘要并触发跨端广播与云端流式边传边下缓存 |
| **触发粘贴 (`Alt+V`)** | `mi paste [--event-id <id>] [--dest "<保存目录>"] --json` | 将最新（或指定 `event_id`）的事件粘贴至当前活动窗口，或直接落盘保存到 `--dest` 目录 |
| **拉取远程目录结构** | `mi remote-ls --device "<设备名或ID>" --path "<远程路径>" --json` | 按需动态拉取目标机器的文件目录结构（首屏探盘符传 `--path "ROOT"`） |
| **远程新建文件夹** | `mi remote-mkdir --device "<设备名或ID>" --path "<远程新目录路径>" --json` | 在目标机器指定路径下创建新文件夹 |
| **定向同步 (`【同步到...】`)**| `mi sync --source "<本地路径>" --device "<设备名或ID>" --remote-dir "<目标机器目录>" --json` | 将本地文件/文件夹直接同步至目标机器指定目录（支持断点续传与摘要校验） |
| **断点续传/状态查询** | `mi transfer status --task-id "<transfer_id>" --json` | 查询大文件传输进度、断点偏移量（`verified_offset`）及摘要校验状态 |

---

## 2. 标准执行工作流 (Standard Runbooks)

### 工作流 A：将本地文件/文件夹（或文本）同步到另一台电脑的指定目录
当用户说“把 `AI教程.docx` 同步到电脑 C 的工作目录下”或“把当前构建产物发到测试机 B”时，严格按以下步骤执行：

1. **确认目标设备在线状态**：
   ```bash
   mi devices list --json
   ```
   检查返回 JSON 中目标设备的 `"online": true` 并获取其标准 `device_id`。

2. **动态拉取目标机器目录结构（若目标路径不明确或需确认目录是否存在）**：
   * 查询目标机器盘符与常用目录（桌面、下载、文档）：
     ```bash
     mi remote-ls --device "<device_id>" --path "ROOT" --json
     ```
   * 按需逐级展开目标目录（例如查看 `D:\` 下的目录）：
     ```bash
     mi remote-ls --device "<device_id>" --path "D:\\" --json
     ```
   * 如目标子目录不存在，可远程创建：
     ```bash
     mi remote-mkdir --device "<device_id>" --path "D:\\工作目录\\AI资料" --json
     ```

3. **执行定向同步并验证摘要**：
   ```bash
   mi sync --source "./AI教程.docx" --device "<device_id>" --remote-dir "D:\\工作目录\\AI资料" --json
   ```
   * **验证步骤**：检查返回 JSON 中的 `"status": "COMPLETED"` 且 `"digest_verified": true`，向用户反馈目标机器落盘路径及 `BLAKE3` 校验结果。

---

### 工作流 B：接收并保存其他设备广播过来的内容（对应 `Alt+V`）
当用户说“把刚才在电脑 A 复制的文件保存到当前目录”或“看看别的电脑发了什么过来”时：

1. **列出本机接收事件缓存列表**：
   ```bash
   mi events list --limit 5 --json
   ```
   从返回列表中识别最新（或用户指定的）事件 `event_id`、`content_category`（`TEXT` / `FILE` / `DIRECTORY`）及文件名。

2. **执行粘贴或指定目录落盘**：
   * 若为文件/文件夹，指定 `--dest` 直接拉取（自动优先复用内网即用即连通道，否则走云端边传边下缓存，支持 `Range` 断点续传）：
     ```bash
     mi paste --event-id "<event_id>" --dest "./downloads" --json
     ```
   * **验证步骤**：确认返回结果中 `"digest_verified": true`，确保落盘文件完整无损。

---

## 3. 异常处理与断点续传恢复指南

* **传输中断恢复**：若大文件传输因网络波动返回 `"status": "INTERRUPTED"`，直接执行：
  ```bash
  mi transfer resume --task-id "<transfer_id>" --json
  ```
  客户端将自动读取本地 `.resume.json` 或查询云端 `Upload-Offset`，从已通过 `4MB` 分块摘要校验的断点偏移量处继续传输，无需从头重传。
* **摘要校验失败 (`HASH_MISMATCH`)**：底层会自动丢弃损坏分块并重传；若由于源文件在传输期间被修改导致全文摘要变更，应提示用户重新在源设备执行 `Alt+C` 或 `mi copy`。

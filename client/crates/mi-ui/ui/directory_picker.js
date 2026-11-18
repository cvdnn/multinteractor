// 【同步到... >】远程目录树懒加载选择器与远程新建文件夹交互逻辑 (FR-CTX-02, FR-CTX-03)

import { formatTransferToastText } from "./transfer_toast.js";

export class RemoteDirectoryPickerModel {
  constructor(deviceId, daemonIpcBaseUrl = "/api/v1") {
    this.deviceId = deviceId;
    this.daemonIpcBaseUrl = daemonIpcBaseUrl;
    this.selectedPath = "ROOT";
    this.expandedNodes = new Map();
  }

  async expandDirectory(remotePath) {
    const url = `${this.daemonIpcBaseUrl}/remote-ls?device=${encodeURIComponent(
      this.deviceId
    )}&path=${encodeURIComponent(remotePath)}`;
    const resp = await fetch(url);
    if (!resp.ok) {
      throw new Error(`Failed to fetch remote directory ${remotePath}: ${resp.status}`);
    }
    const data = await resp.json();
    if (data.error) {
      throw new Error(data.error);
    }
    this.expandedNodes.set(remotePath, data.entries || []);
    this.selectedPath = data.path || remotePath;
    return data;
  }

  async createRemoteFolder(newFolderPath) {
    const resp = await fetch(`${this.daemonIpcBaseUrl}/remote-mkdir`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ device: this.deviceId, path: newFolderPath }),
    });
    if (!resp.ok) {
      throw new Error(`Failed to create remote directory ${newFolderPath}: ${resp.status}`);
    }
    const res = await resp.json();
    if (res.error) {
      throw new Error(res.error);
    }
    this.selectedPath = res.created_path || newFolderPath;
    return this.selectedPath;
  }

  async confirmDirectSync(localSourcePath) {
    const resp = await fetch(`${this.daemonIpcBaseUrl}/sync`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        source: localSourcePath,
        target_device: this.deviceId,
        remote_dir: this.selectedPath,
      }),
    });
    if (!resp.ok) {
      throw new Error(`Direct sync failed: ${resp.status}`);
    }
    const res = await resp.json();
    if (res.error) {
      throw new Error(res.error);
    }
    return res;
  }
}

function initPickerUi() {
  if (typeof document === "undefined") return;
  const params = new URLSearchParams(window.location.search);
  const device = params.get("device") || "local";
  const source = params.get("source") || "";

  const titleEl = document.getElementById("target-device-title");
  const freeBadgeEl = document.getElementById("free-space-badge");
  const breadcrumbEl = document.getElementById("breadcrumb-path");
  const treeEl = document.getElementById("directory-tree");
  const toastEl = document.getElementById("transfer-toast-slot");
  const mkdirBtn = document.getElementById("btn-remote-mkdir");
  const syncBtn = document.getElementById("btn-confirm-sync");

  if (!treeEl || !breadcrumbEl) return;
  if (titleEl) {
    titleEl.textContent = `同步到: ${device}`;
  }

  const model = new RemoteDirectoryPickerModel(device, "/api/v1");

  async function loadDir(path) {
    try {
      const data = await model.expandDirectory(path);
      breadcrumbEl.textContent = data.path || path;
      if (freeBadgeEl && typeof data.free_space_bytes === "number") {
        const gb = (data.free_space_bytes / (1024 * 1024 * 1024)).toFixed(1);
        freeBadgeEl.textContent = `剩余空间: ${gb} GB`;
      }
      treeEl.innerHTML = "";
      for (const entry of data.entries || []) {
        if (!entry.is_dir) continue;
        const row = document.createElement("div");
        row.className = "tree-row";
        row.textContent = `📁 ${entry.name}`;
        row.onclick = () => {
          for (const c of treeEl.children) c.classList.remove("selected");
          row.classList.add("selected");
          model.selectedPath = entry.path;
          breadcrumbEl.textContent = entry.path;
        };
        row.ondblclick = () => loadDir(entry.path);
        treeEl.appendChild(row);
      }
    } catch (err) {
      if (toastEl) toastEl.textContent = `加载目录失败: ${err.message}`;
    }
  }

  if (mkdirBtn) {
    mkdirBtn.onclick = async () => {
      const name = window.prompt("请输入新文件夹名称:");
      if (!name) return;
      const base = model.selectedPath === "ROOT" ? "." : model.selectedPath;
      const target = `${base.replace(/\/$/, "")}/${name}`;
      try {
        const created = await model.createRemoteFolder(target);
        await loadDir(created);
      } catch (err) {
        if (toastEl) toastEl.textContent = `新建文件夹失败: ${err.message}`;
      }
    };
  }

  if (syncBtn) {
    syncBtn.onclick = async () => {
      try {
        const res = await model.confirmDirectSync(source);
        if (toastEl) {
          toastEl.textContent = formatTransferToastText({
            task_id: res.transfer_id,
            file_name: source || "selection",
            verified_offset: res.bytes_transferred || 0,
            total_size: res.bytes_transferred || 0,
            channel_used: res.channel_used || "LAN_DIRECT",
            digest_verified: Boolean(res.digest_verified),
            status: "COMPLETED",
          });
        }
      } catch (err) {
        if (toastEl) toastEl.textContent = `同步失败: ${err.message}`;
      }
    };
  }

  loadDir("ROOT");
}

initPickerUi();

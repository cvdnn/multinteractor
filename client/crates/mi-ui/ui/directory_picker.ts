// 【同步到... >】远程目录树懒加载选择器与远程新建文件夹交互逻辑 (FR-CTX-02, FR-CTX-03)

export interface RemoteFsEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size_bytes: number;
  mtime: number;
}

export interface RemoteLsPayload {
  device_id: string;
  path: string;
  entries: RemoteFsEntry[];
  free_space_bytes: number;
}

export class RemoteDirectoryPickerModel {
  private selectedPath: string = "ROOT";
  private expandedNodes: Map<string, RemoteFsEntry[]> = new Map();

  constructor(
    public readonly deviceId: string,
    public readonly daemonIpcBaseUrl: string = "http://127.0.0.1:53318/api/v1"
  ) {}

  public async expandDirectory(remotePath: string): Promise<RemoteLsPayload> {
    const url = `${this.daemonIpcBaseUrl}/remote-ls?device=${encodeURIComponent(
      this.deviceId
    )}&path=${encodeURIComponent(remotePath)}`;
    const resp = await fetch(url);
    if (!resp.ok) {
      throw new Error(`Failed to fetch remote directory ${remotePath}: ${resp.status}`);
    }
    const data = (await resp.json()) as RemoteLsPayload;
    this.expandedNodes.set(remotePath, data.entries);
    this.selectedPath = remotePath;
    return data;
  }

  public async createRemoteFolder(newFolderPath: string): Promise<string> {
    const resp = await fetch(`${this.daemonIpcBaseUrl}/remote-mkdir`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ device: this.deviceId, path: newFolderPath }),
    });
    if (!resp.ok) {
      throw new Error(`Failed to create remote directory ${newFolderPath}: ${resp.status}`);
    }
    const res = await resp.json();
    this.selectedPath = res.created_path;
    return res.created_path;
  }

  public async confirmDirectSync(localSourcePath: string): Promise<{
    transfer_id: string;
    digest_verified: boolean;
    saved_path: string;
  }> {
    const resp = await fetch(`${this.daemonIpcBaseUrl}/sync`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        source: localSourcePath,
        device: this.deviceId,
        remote_dir: this.selectedPath,
      }),
    });
    if (!resp.ok) {
      throw new Error(`Direct sync failed: ${resp.status}`);
    }
    return resp.json();
  }
}

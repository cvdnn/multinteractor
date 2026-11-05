# multinteractor — 云端中心服务端 (`server/`)

本目录包含 `multinteractor` 个人单节点云端中心服务，负责：
1. **客户端准入与订阅路由管理 (`app/client_sub_manager.py`)**：设备注册鉴权、在线状态与内网 `IP:Port` 同步、Pub/Sub Topic 订阅关系管理。
2. **内嵌式“边传边下 (Stream Teeing)”与断点续传缓存池 (`app/stream_tee_cache.py`)**：支持 `HEAD`/`PATCH` (`Upload-Offset`) 分块断点续传上传、`Range` 断点下载与一写多读实时分流。
3. **全链路 SQLite WAL 日志审计 (`app/audit_logger.py`)**：持久化记录连接审计、事件投递审计与文件传输/摘要校验审计。

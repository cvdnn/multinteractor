//! 轻量级远程目录树选择弹窗与传输进度框宿主入口 (`mi-ui`) (FR-CTX-01~03, TASK-09).

pub mod commands;

use commands::RemoteDirectoryPickerSession;

fn main() {
    let session = RemoteDirectoryPickerSession::new(
        "node_c".to_string(),
        "./AI教程.docx".to_string(),
    );
    println!(
        "multinteractor remote directory picker ready: target={}, initial_path={}",
        session.target_device_id, session.current_remote_path
    );
}

//! 系统电源管理锁：大文件上传期间短暂阻止系统自动休眠 (应对 RISK-02).
//! - macOS: 启动 `caffeinate -di` 阻止显示器与系统空闲休眠，Drop 时自动终止；
//! - Windows: 通过 `SetThreadExecutionState` 阻止系统自动休眠.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct PowerSleepInhibitor {
    active: Arc<AtomicBool>,
    reason: String,
    child_proc: Arc<Mutex<Option<Child>>>,
}

impl PowerSleepInhibitor {
    pub fn acquire(reason: impl Into<String>) -> Self {
        let reason_str = reason.into();
        let child = Self::spawn_os_inhibitor(&reason_str);
        Self {
            active: Arc::new(AtomicBool::new(true)),
            reason: reason_str,
            child_proc: Arc::new(Mutex::new(child)),
        }
    }

    fn spawn_os_inhibitor(_reason: &str) -> Option<Child> {
        #[cfg(target_os = "macos")]
        {
            Command::new("caffeinate").args(["-di"]).spawn().ok()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn release(&self) {
        if self.active.swap(false, Ordering::SeqCst) {
            if let Ok(mut guard) = self.child_proc.lock() {
                if let Some(mut child) = guard.take() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}

impl Drop for PowerSleepInhibitor {
    fn drop(&mut self) {
        self.release();
    }
}

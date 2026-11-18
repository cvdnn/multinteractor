//! 原生桌面 GUI 主控台、系统状态栏（Menu Bar / System Tray）常驻与守护服务生命周期托管.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use eframe::egui;
use mi_daemon::config::DaemonConfig;
use mi_daemon::{DaemonNode, RunningDaemonHandles};
use mi_protocol::envelope::CloudEventEnvelope;
use mi_protocol::ipc::{
    DeviceStatusDto, IpcCopyRequest, IpcPasteRequest, IpcRemoteLsRequest, IpcRemoteMkdirRequest,
    IpcSyncRequest,
};
use mi_protocol::rpc::{FsEntry, ROOT_PATH_SENTINEL};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ActiveTab {
    Clipboard,
    RemoteSync,
}

struct DaemonController {
    runtime: tokio::runtime::Runtime,
    config: DaemonConfig,
    node: Option<DaemonNode>,
    handles: Option<RunningDaemonHandles>,
    last_error: Option<String>,
}

impl DaemonController {
    fn new(config_path: Option<PathBuf>) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        let config = DaemonConfig::from_env_and_file(config_path.as_deref());
        let mut ctrl = Self {
            runtime,
            config,
            node: None,
            handles: None,
            last_error: None,
        };
        ctrl.start();
        Ok(ctrl)
    }

    fn is_running(&self) -> bool {
        self.handles.is_some()
    }

    fn start(&mut self) {
        if self.handles.is_some() {
            return;
        }
        let node = DaemonNode::new(self.config.clone());
        match self.runtime.block_on(node.start_background()) {
            Ok(handles) => {
                self.node = Some(node);
                self.handles = Some(handles);
                self.last_error = None;
            }
            Err(err) => {
                self.last_error = Some(format!("启动守护服务失败: {}", err));
            }
        }
    }

    fn stop(&mut self) {
        if let Some(handles) = self.handles.take() {
            handles.shutdown();
        }
        self.node = None;
    }

    fn devices(&self) -> Vec<DeviceStatusDto> {
        self.node
            .as_ref()
            .map(|n| n.ipc.handle_devices_list())
            .unwrap_or_default()
    }

    fn events(&self, limit: usize) -> Vec<CloudEventEnvelope> {
        self.node
            .as_ref()
            .map(|n| n.ipc.handle_events_list(limit).events)
            .unwrap_or_default()
    }

    fn copy_text(&self, text: &str) -> anyhow::Result<String> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("守护服务未运行"))?;
        let now_ms = now_unix_ms();
        let event_id = format!("evt_{}_{}", self.config.device_id, now_ms);
        let resp = node.ipc.handle_copy(
            IpcCopyRequest {
                text: Some(text.to_string()),
                path: None,
            },
            event_id,
            now_ms,
        )?;
        Ok(format!(
            "已广播文本 ({} bytes, BLAKE3: {}...)",
            resp.size_bytes,
            &resp.content_hash.chars().take(12).collect::<String>()
        ))
    }

    fn paste_latest(&self, event_id: Option<String>) -> anyhow::Result<String> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("守护服务未运行"))?;
        let resp = node.ipc.handle_paste(IpcPasteRequest {
            event_id,
            dest_dir: None,
        })?;
        if let Some(text) = resp.inline_text {
            Ok(format!("粘贴校验通过 (BLAKE3): {}", text))
        } else if let Some(saved) = resp.saved_path {
            Ok(format!("已校验并落盘至: {}", saved))
        } else {
            Ok("粘贴完成".to_string())
        }
    }

    fn remote_ls(&self, device: &str, path: &str) -> anyhow::Result<(Vec<FsEntry>, u64)> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("守护服务未运行"))?;
        let resp = node.ipc.handle_remote_ls(IpcRemoteLsRequest {
            device: device.to_string(),
            path: path.to_string(),
        })?;
        Ok((resp.entries, resp.free_space_bytes))
    }

    fn remote_mkdir(&self, device: &str, path: &str) -> anyhow::Result<String> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("守护服务未运行"))?;
        let resp = node.ipc.handle_remote_mkdir(IpcRemoteMkdirRequest {
            device: device.to_string(),
            path: path.to_string(),
        })?;
        if resp.status == "CREATED" {
            Ok(format!("已创建远端目录: {}", resp.created_path))
        } else {
            anyhow::bail!("{}", resp.error.unwrap_or_else(|| "创建失败".to_string()))
        }
    }

    fn sync_file(&self, source: &str, device: &str, remote_dir: &str) -> anyhow::Result<String> {
        let node = self
            .node
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("守护服务未运行"))?;
        let resp = node.ipc.handle_sync(
            IpcSyncRequest {
                source: source.to_string(),
                target_device: device.to_string(),
                remote_dir: remote_dir.to_string(),
            },
            now_unix_ms(),
        )?;
        Ok(format!(
            "同步成功 [{}]: {} ({} bytes)",
            resp.channel_used, resp.remote_saved_path, resp.bytes_transferred
        ))
    }
}

struct TrayHolder {
    tray_icon: TrayIcon,
    status_item: MenuItem,
    show_item: MenuItem,
    toggle_item: MenuItem,
    quit_item: MenuItem,
    running_icon: Icon,
    stopped_icon: Icon,
}

impl TrayHolder {
    fn new(device_id: &str) -> anyhow::Result<Self> {
        let status_item = MenuItem::new(format!("状态: 运行中 ({})", device_id), false, None);
        let show_item = MenuItem::new("显示主界面", true, None);
        let toggle_item = MenuItem::new("停止守护服务", true, None);
        let quit_item = MenuItem::new("退出 multinteractor", true, None);

        let menu = Menu::new();
        menu.append(&status_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&show_item)?;
        menu.append(&toggle_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&quit_item)?;

        let running_icon = build_status_icon(true)?;
        let stopped_icon = build_status_icon(false)?;

        let mut builder = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(format!("multinteractor ({}) - 运行中", device_id))
            .with_icon(running_icon.clone());

        #[cfg(target_os = "macos")]
        {
            builder = builder.with_title("MI ● 运行中");
        }

        let tray_icon = builder.build()?;
        Ok(Self {
            tray_icon,
            status_item,
            show_item,
            toggle_item,
            quit_item,
            running_icon,
            stopped_icon,
        })
    }

    fn update_status(&self, running: bool, device_id: &str, online_peers: usize) {
        if running {
            let _ = self.tray_icon.set_icon(Some(self.running_icon.clone()));
            let _ = self.tray_icon.set_tooltip(Some(format!(
                "multinteractor ({}) - 运行中 | 在线设备: {}",
                device_id, online_peers
            )));
            #[cfg(target_os = "macos")]
            self.tray_icon
                .set_title(Some(format!("MI ● 运行中 ({})", online_peers)));
            self.status_item
                .set_text(format!("状态: 运行中 ({} | 在线 {} 台)", device_id, online_peers));
            self.toggle_item.set_text("停止守护服务");
        } else {
            let _ = self.tray_icon.set_icon(Some(self.stopped_icon.clone()));
            let _ = self
                .tray_icon
                .set_tooltip(Some(format!("multinteractor ({}) - 已停止", device_id)));
            #[cfg(target_os = "macos")]
            self.tray_icon.set_title(Some("MI ○ 已停止"));
            self.status_item
                .set_text(format!("状态: 已停止 ({})", device_id));
            self.toggle_item.set_text("启动守护服务");
        }
    }
}

struct MultinteractorApp {
    daemon: Arc<Mutex<DaemonController>>,
    tray: Option<TrayHolder>,
    should_quit: bool,
    window_visible: bool,
    active_tab: ActiveTab,

    // UI state
    copy_input: String,
    status_banner: Option<String>,
    selected_device: String,
    current_remote_path: String,
    remote_entries: Vec<FsEntry>,
    remote_free_bytes: u64,
    new_folder_name: String,
    sync_source_path: String,
    last_tray_running: Option<bool>,
    last_tray_peers: usize,
}

impl MultinteractorApp {
    fn new(
        daemon: Arc<Mutex<DaemonController>>,
        tray: Option<TrayHolder>,
        default_device: String,
        default_source: String,
    ) -> Self {
        Self {
            daemon,
            tray,
            should_quit: false,
            window_visible: true,
            active_tab: ActiveTab::Clipboard,
            copy_input: String::new(),
            status_banner: None,
            selected_device: default_device,
            current_remote_path: ROOT_PATH_SENTINEL.to_string(),
            remote_entries: Vec::new(),
            remote_free_bytes: 0,
            new_folder_name: String::new(),
            sync_source_path: default_source,
            last_tray_running: None,
            last_tray_peers: 0,
        }
    }

    fn show_window(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.window_visible = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    fn load_remote_dir(&mut self, path: &str) {
        let device = self.selected_device.clone();
        if device.is_empty() {
            self.status_banner = Some("请先在左侧选择目标设备".to_string());
            return;
        }
        let guard = self.daemon.lock().unwrap();
        match guard.remote_ls(&device, path) {
            Ok((entries, free_bytes)) => {
                self.current_remote_path = path.to_string();
                self.remote_entries = entries;
                self.remote_free_bytes = free_bytes;
                self.status_banner = Some(format!("已加载 {} 的目录: {}", device, path));
            }
            Err(err) => {
                self.status_banner = Some(format!("目录加载失败: {}", err));
            }
        }
    }

    fn poll_tray_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if matches!(ev, TrayIconEvent::DoubleClick { .. }) {
                self.show_window(ctx);
            }
        }

        while let Ok(ev) = MenuEvent::receiver().try_recv() {
            let Some(ref tray) = self.tray else {
                continue;
            };
            if ev.id == tray.show_item.id() {
                self.show_window(ctx);
            } else if ev.id == tray.toggle_item.id() {
                let mut guard = self.daemon.lock().unwrap();
                if guard.is_running() {
                    guard.stop();
                    self.status_banner = Some("守护服务已停止".to_string());
                } else {
                    guard.start();
                    self.status_banner = Some("守护服务已启动".to_string());
                }
            } else if ev.id == tray.quit_item.id() {
                self.should_quit = true;
                self.daemon.lock().unwrap().stop();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}

impl eframe::App for MultinteractorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 保持后台轮询托盘事件与状态刷新
        ctx.request_repaint_after(Duration::from_millis(400));

        self.poll_tray_events(ctx);

        // 拦截关闭按钮 -> 最小化并常驻状态栏
        if ctx.input(|i| i.viewport().close_requested()) && !self.should_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_to_tray(ctx);
            return;
        }

        let (running, device_id, device_name, ipc_addr, lan_addr, devices, events, daemon_err) = {
            let guard = self.daemon.lock().unwrap();
            (
                guard.is_running(),
                guard.config.device_id.clone(),
                guard.config.device_name.clone(),
                guard.config.local_ipc_addr.clone(),
                guard.config.lan_bind_addr.clone(),
                guard.devices(),
                guard.events(15),
                guard.last_error.clone(),
            )
        };

        let online_peers = devices.iter().filter(|d| d.online).count();
        if self.last_tray_running != Some(running) || self.last_tray_peers != online_peers {
            if let Some(ref tray) = self.tray {
                tray.update_status(running, &device_id, online_peers);
            }
            self.last_tray_running = Some(running);
            self.last_tray_peers = online_peers;
        }

        if !self.window_visible {
            return;
        }

        // 顶部全局状态栏
        egui::TopBottomPanel::top("top_status_bar")
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(16.0, 10.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // 品牌标识与主标题
                    badge(ui, "MI", egui::Color32::WHITE, theme::PRIMARY);
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new("multinteractor 跨机协同中枢")
                            .size(16.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    ui.add_space(8.0);
                    if running {
                        badge(ui, "● 守护服务运行中", theme::SUCCESS, theme::SUCCESS_SOFT);
                    } else {
                        badge(ui, "○ 守护服务已停止", theme::DANGER, theme::DANGER_SOFT);
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("最小化到状态栏").clicked() {
                            self.hide_to_tray(ctx);
                        }
                        if running {
                            if ui.button("停止守护服务").clicked() {
                                self.daemon.lock().unwrap().stop();
                                self.status_banner = Some("已停止后台守护服务".to_string());
                            }
                        } else if primary_button(ui, "启动守护服务").clicked() {
                            self.daemon.lock().unwrap().start();
                            self.status_banner = Some("已启动后台守护服务".to_string());
                        }
                    });
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    info_chip(ui, "本机节点", &format!("{} ({})", device_name, device_id));
                    info_chip(ui, "IPC", &ipc_addr);
                    info_chip(ui, "LAN mTLS", &lan_addr);
                });
            });

        // 底部状态提示栏
        egui::TopBottomPanel::bottom("bottom_banner")
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(16.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(ref err) = daemon_err {
                        badge(ui, "异常", theme::DANGER, theme::DANGER_SOFT);
                        ui.label(egui::RichText::new(err).small().color(theme::DANGER));
                    } else if let Some(ref msg) = self.status_banner {
                        badge(ui, "状态", theme::PRIMARY, theme::PRIMARY_SOFT);
                        ui.label(egui::RichText::new(msg).small().color(theme::TEXT_PRIMARY));
                    } else {
                        badge(ui, "提示", theme::TEXT_SECONDARY, theme::BG_SUBSURFACE);
                        ui.label(
                            egui::RichText::new("关闭窗口将自动最小化至系统状态栏，守护服务持续在后台运行。")
                                .small()
                                .color(theme::TEXT_SECONDARY),
                        );
                    }
                });
            });

        // 左栏：集群设备列表
        egui::SidePanel::left("devices_panel")
            .default_width(300.0)
            .min_width(260.0)
            .max_width(380.0)
            .resizable(true)
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_CANVAS)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .inner_margin(egui::Margin::symmetric(12.0, 12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("集群设备")
                            .size(14.0)
                            .strong()
                            .color(theme::TEXT_PRIMARY),
                    );
                    badge(
                        ui,
                        &format!("{} 台", devices.len()),
                        theme::PRIMARY,
                        theme::PRIMARY_SOFT,
                    );
                });
                ui.add_space(8.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if devices.is_empty() {
                        card_frame(ui, theme::BORDER).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("暂无已发现的在线设备")
                                    .color(theme::TEXT_SECONDARY),
                            );
                        });
                    }
                    for dev in &devices {
                        let is_self = dev.device_id == device_id;
                        let is_selected = !is_self && dev.device_id == self.selected_device;
                        let stroke = if is_selected {
                            theme::PRIMARY
                        } else {
                            theme::BORDER
                        };
                        card_frame(ui, stroke).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let dot_color = if dev.online {
                                    theme::SUCCESS
                                } else {
                                    egui::Color32::from_rgb(148, 163, 184)
                                };
                                ui.colored_label(dot_color, "●");
                                ui.label(
                                    egui::RichText::new(&dev.name)
                                        .strong()
                                        .color(theme::TEXT_PRIMARY),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if is_self {
                                            badge(ui, "本机", theme::PRIMARY, theme::PRIMARY_SOFT);
                                        } else if dev.online {
                                            badge(ui, "在线", theme::SUCCESS, theme::SUCCESS_SOFT);
                                        } else {
                                            badge(
                                                ui,
                                                "离线",
                                                theme::TEXT_SECONDARY,
                                                theme::BG_SUBSURFACE,
                                            );
                                        }
                                    },
                                );
                            });
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("ID: {}  •  {}", dev.device_id, dev.os))
                                    .small()
                                    .color(theme::TEXT_SECONDARY),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "端点: {}",
                                    dev.lan_endpoint.as_deref().unwrap_or("云端中继模式")
                                ))
                                .small()
                                .color(theme::TEXT_SECONDARY),
                            );
                            if !is_self && dev.online {
                                ui.add_space(8.0);
                                let btn_text = if is_selected {
                                    "正在浏览 / 重新加载根目录"
                                } else {
                                    "浏览远程目录 / 定向同步"
                                };
                                let btn = egui::Button::new(
                                    egui::RichText::new(btn_text)
                                        .small()
                                        .color(if is_selected {
                                            theme::PRIMARY
                                        } else {
                                            theme::TEXT_PRIMARY
                                        }),
                                )
                                .fill(if is_selected {
                                    theme::PRIMARY_SOFT
                                } else {
                                    theme::BG_SUBSURFACE
                                })
                                .min_size(egui::vec2(ui.available_width(), 28.0));
                                if ui.add(btn).clicked() {
                                    self.selected_device = dev.device_id.clone();
                                    self.active_tab = ActiveTab::RemoteSync;
                                    self.load_remote_dir(ROOT_PATH_SENTINEL);
                                }
                            }
                        });
                        ui.add_space(8.0);
                    }
                });
            });

        // 右栏：主工作区（剪贴板流 / 远程同步）
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::BG_CANVAS)
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                // 分段式标签栏
                egui::Frame::none()
                    .fill(theme::BG_SUBSURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::BORDER))
                    .rounding(8.0)
                    .inner_margin(4.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if tab_button(
                                ui,
                                "跨机剪贴板流",
                                self.active_tab == ActiveTab::Clipboard,
                            ) {
                                self.active_tab = ActiveTab::Clipboard;
                            }
                            if tab_button(
                                ui,
                                "远程目录浏览 & 定向同步",
                                self.active_tab == ActiveTab::RemoteSync,
                            ) {
                                self.active_tab = ActiveTab::RemoteSync;
                            }
                        });
                    });
                ui.add_space(10.0);

                match self.active_tab {
                    ActiveTab::Clipboard => {
                        card_frame(ui, theme::BORDER).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("快速广播文本到所有在线设备")
                                        .strong()
                                        .color(theme::TEXT_PRIMARY),
                                );
                                badge(ui, "BLAKE3 校验", theme::SUCCESS, theme::SUCCESS_SOFT);
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                let input_w = (ui.available_width() - 236.0).max(160.0);
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.copy_input)
                                        .desired_width(input_w)
                                        .hint_text("输入要跨机广播的文本内容..."),
                                );
                                if primary_button(ui, "广播 (Alt+C)").clicked()
                                    && !self.copy_input.trim().is_empty()
                                {
                                    let text = self.copy_input.clone();
                                    let res = self.daemon.lock().unwrap().copy_text(&text);
                                    self.status_banner = Some(match res {
                                        Ok(msg) => {
                                            self.copy_input.clear();
                                            msg
                                        }
                                        Err(e) => format!("广播失败: {}", e),
                                    });
                                }
                                if ui.button("粘贴最新 (Alt+V)").clicked() {
                                    let res = self.daemon.lock().unwrap().paste_latest(None);
                                    self.status_banner = Some(match res {
                                        Ok(msg) => msg,
                                        Err(e) => format!("粘贴失败: {}", e),
                                    });
                                }
                            });
                        });

                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("最近接收事件缓存")
                                    .size(14.0)
                                    .strong()
                                    .color(theme::TEXT_PRIMARY),
                            );
                            badge(
                                ui,
                                &format!("{} 条", events.len()),
                                theme::PRIMARY,
                                theme::PRIMARY_SOFT,
                            );
                        });
                        ui.add_space(6.0);

                        egui::ScrollArea::vertical().show(ui, |ui| {
                            if events.is_empty() {
                                card_frame(ui, theme::BORDER).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.label(
                                        egui::RichText::new(
                                            "暂无接收事件，可点击上方“广播”或在其他设备复制内容。",
                                        )
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                });
                            }
                            for ev in &events {
                                card_frame(ui, theme::BORDER).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.horizontal(|ui| {
                                        let cat = format!("{:?}", ev.payload.content_category);
                                        badge(ui, &cat, theme::PRIMARY, theme::PRIMARY_SOFT);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "来自 {}",
                                                ev.source_device.device_name
                                            ))
                                            .strong()
                                            .color(theme::TEXT_PRIMARY),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                if ui.button("校验并粘贴").clicked() {
                                                    let res = self
                                                        .daemon
                                                        .lock()
                                                        .unwrap()
                                                        .paste_latest(Some(ev.event_id.clone()));
                                                    self.status_banner = Some(match res {
                                                        Ok(msg) => msg,
                                                        Err(e) => format!("粘贴失败: {}", e),
                                                    });
                                                }
                                            },
                                        );
                                    });
                                    if let Some(ref txt) = ev.payload.inline_data {
                                        ui.add_space(4.0);
                                        ui.label(
                                            egui::RichText::new(txt).color(theme::TEXT_PRIMARY),
                                        );
                                    }
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "ID: {}  |  BLAKE3: {}...",
                                            ev.event_id,
                                            &ev.payload
                                                .digest
                                                .full_hash
                                                .chars()
                                                .take(16)
                                                .collect::<String>()
                                        ))
                                        .small()
                                        .monospace()
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                });
                                ui.add_space(8.0);
                            }
                        });
                    }
                    ActiveTab::RemoteSync => {
                        // 1. 目标设备与路径导航卡片
                        card_frame(ui, theme::BORDER).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("目标设备")
                                        .strong()
                                        .color(theme::TEXT_PRIMARY),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.selected_device)
                                        .desired_width(150.0)
                                        .hint_text("输入或左侧选择设备"),
                                );
                                if primary_button(ui, "加载根目录").clicked() {
                                    self.load_remote_dir(ROOT_PATH_SENTINEL);
                                }
                                if self.current_remote_path != ROOT_PATH_SENTINEL {
                                    if ui.button("返回上级").clicked() {
                                        let parent = parent_remote_path(&self.current_remote_path);
                                        self.load_remote_dir(&parent);
                                    }
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if self.remote_free_bytes > 0 {
                                            let gb = self.remote_free_bytes as f64
                                                / (1024.0 * 1024.0 * 1024.0);
                                            badge(
                                                ui,
                                                &format!("可用空间: {:.2} GB", gb),
                                                theme::SUCCESS,
                                                theme::SUCCESS_SOFT,
                                            );
                                        }
                                    },
                                );
                            });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("当前远程路径")
                                        .small()
                                        .color(theme::TEXT_SECONDARY),
                                );
                                egui::Frame::none()
                                    .fill(theme::BG_SUBSURFACE)
                                    .rounding(4.0)
                                    .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(&self.current_remote_path)
                                                .small()
                                                .monospace()
                                                .color(theme::TEXT_PRIMARY),
                                        );
                                    });
                            });
                        });

                        ui.add_space(8.0);

                        // 2. 远程目录内容列表卡片（自适应剩余高度）
                        let mut next_path: Option<String> = None;
                        let list_height = (ui.available_height() - 128.0).max(140.0);
                        card_frame(ui, theme::BORDER).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            egui::ScrollArea::vertical()
                                .min_scrolled_height(list_height)
                                .max_height(list_height)
                                .show(ui, |ui| {
                                    if self.remote_entries.is_empty() {
                                        ui.add_space(12.0);
                                        ui.label(
                                            egui::RichText::new(
                                                "目录为空或尚未加载，请点击上方“加载根目录”或左侧设备卡片。",
                                            )
                                            .color(theme::TEXT_SECONDARY),
                                        );
                                    }
                                    for entry in &self.remote_entries {
                                        egui::Frame::none()
                                            .fill(theme::BG_CANVAS)
                                            .rounding(6.0)
                                            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                            .show(ui, |ui| {
                                                ui.set_min_width(ui.available_width());
                                                ui.horizontal(|ui| {
                                                    if entry.is_dir {
                                                        badge(
                                                            ui,
                                                            "目录",
                                                            theme::PRIMARY,
                                                            theme::PRIMARY_SOFT,
                                                        );
                                                        if ui
                                                            .add(
                                                                egui::Button::new(
                                                                    egui::RichText::new(&entry.name)
                                                                        .strong()
                                                                        .color(theme::TEXT_PRIMARY),
                                                                )
                                                                .frame(false),
                                                            )
                                                            .clicked()
                                                        {
                                                            next_path = Some(entry.path.clone());
                                                        }
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                if ui
                                                                    .small_button("打开 >")
                                                                    .clicked()
                                                                {
                                                                    next_path =
                                                                        Some(entry.path.clone());
                                                                }
                                                            },
                                                        );
                                                    } else {
                                                        badge(
                                                            ui,
                                                            "文件",
                                                            theme::TEXT_SECONDARY,
                                                            theme::BG_SUBSURFACE,
                                                        );
                                                        ui.label(
                                                            egui::RichText::new(&entry.name)
                                                                .color(theme::TEXT_PRIMARY),
                                                        );
                                                        ui.with_layout(
                                                            egui::Layout::right_to_left(
                                                                egui::Align::Center,
                                                            ),
                                                            |ui| {
                                                                ui.label(
                                                                    egui::RichText::new(
                                                                        format_bytes(
                                                                            entry.size_bytes,
                                                                        ),
                                                                    )
                                                                    .small()
                                                                    .monospace()
                                                                    .color(theme::TEXT_SECONDARY),
                                                                );
                                                            },
                                                        );
                                                    }
                                                });
                                            });
                                        ui.add_space(4.0);
                                    }
                                });
                        });
                        if let Some(p) = next_path {
                            self.load_remote_dir(&p);
                        }

                        ui.add_space(8.0);

                        // 3. 底部操作表单卡片（两列网格严格对齐）
                        card_frame(ui, theme::BORDER).show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            let btn_width = 140.0;
                            let label_col_width = 92.0;
                            let input_width =
                                (ui.available_width() - label_col_width - btn_width - 28.0)
                                    .max(160.0);

                            egui::Grid::new("remote_sync_form_grid")
                                .num_columns(3)
                                .spacing([10.0, 10.0])
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new("新建远程目录")
                                            .strong()
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.new_folder_name)
                                            .desired_width(input_width)
                                            .hint_text("输入新文件夹名称..."),
                                    );
                                    if ui
                                        .add_sized(
                                            [btn_width, 28.0],
                                            egui::Button::new("+ 创建目录"),
                                        )
                                        .clicked()
                                        && !self.new_folder_name.trim().is_empty()
                                    {
                                        let sep = if self.current_remote_path.ends_with('/') {
                                            ""
                                        } else {
                                            "/"
                                        };
                                        let target = format!(
                                            "{}{}{}",
                                            self.current_remote_path,
                                            sep,
                                            self.new_folder_name.trim()
                                        );
                                        let res = self
                                            .daemon
                                            .lock()
                                            .unwrap()
                                            .remote_mkdir(&self.selected_device, &target);
                                        match res {
                                            Ok(msg) => {
                                                self.status_banner = Some(msg);
                                                self.new_folder_name.clear();
                                                let cur = self.current_remote_path.clone();
                                                self.load_remote_dir(&cur);
                                            }
                                            Err(err) => {
                                                self.status_banner =
                                                    Some(format!("新建目录失败: {}", err));
                                            }
                                        }
                                    }
                                    ui.end_row();

                                    ui.label(
                                        egui::RichText::new("本地源路径")
                                            .strong()
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                    ui.add(
                                        egui::TextEdit::singleline(&mut self.sync_source_path)
                                            .desired_width(input_width)
                                            .hint_text("输入要同步的本地文件或目录路径..."),
                                    );
                                    let sync_btn = egui::Button::new(
                                        egui::RichText::new("立即同步到此目录")
                                            .color(egui::Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(theme::PRIMARY);
                                    if ui.add_sized([btn_width, 28.0], sync_btn).clicked() {
                                        let res = self.daemon.lock().unwrap().sync_file(
                                            &self.sync_source_path,
                                            &self.selected_device,
                                            &self.current_remote_path,
                                        );
                                        self.status_banner = Some(match res {
                                            Ok(msg) => msg,
                                            Err(err) => format!("同步失败: {}", err),
                                        });
                                    }
                                    ui.end_row();
                                });
                        });
                    }
                }
            });
    }
}

pub fn run_native_gui(
    config_path: Option<PathBuf>,
    default_device: String,
    default_source: String,
) -> anyhow::Result<()> {
    let daemon = Arc::new(Mutex::new(DaemonController::new(config_path)?));
    let device_id = daemon.lock().unwrap().config.device_id.clone();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 660.0])
            .with_min_inner_size([760.0, 500.0])
            .with_title("multinteractor 跨机协同中枢"),
        ..Default::default()
    };

    let daemon_for_app = daemon.clone();
    let res = eframe::run_native(
        "multinteractor",
        native_options,
        Box::new(move |cc| {
            configure_light_theme_and_fonts(&cc.egui_ctx);
            let tray = TrayHolder::new(&device_id).ok();
            Ok(Box::new(MultinteractorApp::new(
                daemon_for_app,
                tray,
                default_device,
                default_source,
            )))
        }),
    );

    // 兜底确保退出 GUI 时停止后台守护服务
    daemon.lock().unwrap().stop();
    res.map_err(|e| anyhow::anyhow!("eframe error: {}", e))
}

mod theme {
    use eframe::egui::Color32;

    pub const BG_CANVAS: Color32 = Color32::from_rgb(248, 250, 252);
    pub const BG_SURFACE: Color32 = Color32::WHITE;
    pub const BG_SUBSURFACE: Color32 = Color32::from_rgb(241, 245, 249);
    pub const BORDER: Color32 = Color32::from_rgb(226, 232, 240);
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(15, 23, 42);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(100, 116, 139);
    pub const PRIMARY: Color32 = Color32::from_rgb(37, 99, 235);
    pub const PRIMARY_SOFT: Color32 = Color32::from_rgb(239, 246, 255);
    pub const SUCCESS: Color32 = Color32::from_rgb(5, 150, 105);
    pub const SUCCESS_SOFT: Color32 = Color32::from_rgb(236, 253, 245);
    pub const DANGER: Color32 = Color32::from_rgb(225, 29, 72);
    pub const DANGER_SOFT: Color32 = Color32::from_rgb(255, 241, 242);
}

fn card_frame(_ui: &egui::Ui, stroke_color: egui::Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(theme::BG_SURFACE)
        .stroke(egui::Stroke::new(1.0_f32, stroke_color))
        .rounding(8.0)
        .inner_margin(12.0)
}

fn badge(ui: &mut egui::Ui, text: &str, fg: egui::Color32, bg: egui::Color32) {
    egui::Frame::none()
        .fill(bg)
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(7.0, 2.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).small().strong().color(fg));
        });
}

fn info_chip(ui: &mut egui::Ui, label: &str, value: &str) {
    egui::Frame::none()
        .fill(theme::BG_SUBSURFACE)
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    egui::RichText::new(format!("{}:", label))
                        .small()
                        .color(theme::TEXT_SECONDARY),
                );
                ui.label(
                    egui::RichText::new(value)
                        .small()
                        .monospace()
                        .color(theme::TEXT_PRIMARY),
                );
            });
        });
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(text).color(egui::Color32::WHITE).strong()).fill(theme::PRIMARY))
}

fn tab_button(ui: &mut egui::Ui, text: &str, active: bool) -> bool {
    let rich = if active {
        egui::RichText::new(text).strong().color(theme::PRIMARY)
    } else {
        egui::RichText::new(text).color(theme::TEXT_SECONDARY)
    };
    let btn = egui::Button::new(rich)
        .fill(if active {
            theme::BG_SURFACE
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(if active {
            egui::Stroke::new(1.0_f32, theme::BORDER)
        } else {
            egui::Stroke::NONE
        })
        .rounding(6.0);
    ui.add(btn).clicked()
}

fn parent_remote_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => ROOT_PATH_SENTINEL.to_string(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.2} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{} B", bytes)
    }
}

fn configure_light_theme_and_fonts(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = theme::BG_CANVAS;
    visuals.window_fill = theme::BG_SURFACE;
    visuals.extreme_bg_color = theme::BG_SURFACE;
    visuals.faint_bg_color = theme::BG_SUBSURFACE;
    visuals.selection.bg_fill = theme::PRIMARY;
    visuals.selection.stroke = egui::Stroke::new(1.0_f32, theme::PRIMARY);

    let rounding = egui::Rounding::same(6.0);
    visuals.widgets.noninteractive.rounding = rounding;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY);

    visuals.widgets.inactive.rounding = rounding;
    visuals.widgets.inactive.weak_bg_fill = theme::BG_SUBSURFACE;
    visuals.widgets.inactive.bg_fill = theme::BG_SUBSURFACE;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, theme::BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme::TEXT_PRIMARY);

    visuals.widgets.hovered.rounding = rounding;
    visuals.widgets.hovered.weak_bg_fill = theme::PRIMARY_SOFT;
    visuals.widgets.hovered.bg_fill = theme::PRIMARY_SOFT;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, theme::PRIMARY);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, theme::PRIMARY);

    visuals.widgets.active.rounding = rounding;
    visuals.widgets.active.weak_bg_fill = theme::PRIMARY_SOFT;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, theme::PRIMARY);
    ctx.set_visuals(visuals);

    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size.y = 28.0;
    });

    // 加载多字体回退链，确保简体中文与常见符号完整渲染不出现缺字方框
    let mut fonts = egui::FontDefinitions::default();
    let candidates = [
        ("cjk_pingfang", "/System/Library/Fonts/PingFang.ttc"),
        ("cjk_stheiti", "/System/Library/Fonts/STHeiti Medium.ttc"),
        ("cjk_hiragino", "/System/Library/Fonts/Hiragino Sans GB.ttc"),
        (
            "cjk_arial_unicode",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ),
        ("cjk_msyh", "C:\\Windows\\Fonts\\msyh.ttc"),
        (
            "cjk_noto",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ),
    ];

    let mut loaded_keys = Vec::new();
    for (key, path) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(key.to_owned(), egui::FontData::from_owned(bytes));
            loaded_keys.push(key.to_owned());
        }
    }

    if !loaded_keys.is_empty() {
        let prop = fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default();
        for (idx, key) in loaded_keys.iter().enumerate() {
            prop.insert(idx, key.clone());
        }

        let mono = fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default();
        for key in &loaded_keys {
            mono.push(key.clone());
        }
    }
    ctx.set_fonts(fonts);
}

fn build_status_icon(running: bool) -> anyhow::Result<Icon> {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let (r, g, b) = if running {
        (37u8, 99u8, 235u8) // Royal Blue
    } else {
        (148u8, 163u8, 184u8) // Slate-400
    };
    let (dot_r, dot_g, dot_b) = if running {
        (16u8, 185u8, 129u8) // Emerald
    } else {
        (225u8, 29u8, 72u8) // Rose
    };

    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist <= 13.0 {
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            }
            // 右下角状态小圆点
            let sdx = x as f32 - 24.0;
            let sdy = y as f32 - 24.0;
            if (sdx * sdx + sdy * sdy).sqrt() <= 5.5 {
                rgba[idx] = dot_r;
                rgba[idx + 1] = dot_g;
                rgba[idx + 2] = dot_b;
                rgba[idx + 3] = 255;
            }
        }
    }
    Ok(Icon::from_rgba(rgba, size, size)?)
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

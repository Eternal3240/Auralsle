//! OPPO 互联（O+ Connect）「手机通知镜像」集成模块。
//!
//! 数据流：增量跟踪 `%APPDATA%\O+Connect\devicespace\oshare\ovmsocketlog.log`
//! 中的 `ShowToast Data received:{json}` → 解析 [`model::Notification`] →
//! 解码手机侧来源 App 原始图标 → 按 `key` 去重/原地替换 → 驱动灵屿上岛展示；
//! 可选通过系统通知开关抑制 `com.oplus.devicespace` 自身重复弹出。

pub mod mirror;
pub mod model;
pub mod registry;

pub use mirror::{
    cached_icon_png, default_icon_dir, is_suppress_active, log_path, refresh_oppo_setting_status,
    restore_crash_leftover_if_needed, restore_on_normal_exit, restore_oppo_native_now,
    set_suppress_active, spawn, status_snapshot, MirrorConfig, MirrorEvent, MirrorEventKind,
    MirrorHandle, MirrorStatus, Watcher,
};
pub use model::Notification;
pub use registry::{open_system_notification_settings, OPPO_AUMID};

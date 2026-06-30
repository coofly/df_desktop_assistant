//! 提供 Tauri 桌面应用的可执行入口。

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// 启动桌面助手应用。
fn main() {
    df_desktop_assistant_lib::run()
}

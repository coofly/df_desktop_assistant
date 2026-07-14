//! 提供与 Tauri identifier 一致的统一应用数据目录。

use std::{fs, path::PathBuf};

const APP_IDENTIFIER: &str = "com.coofly.df-desktop-assistant";

/// 返回标准应用数据目录。
pub(crate) fn canonical_app_data_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join(APP_IDENTIFIER)
}

/// 创建并返回标准应用数据目录。
pub(crate) fn prepare_app_data_dir() -> PathBuf {
    let app_data_dir = canonical_app_data_dir();
    if let Err(error) = fs::create_dir_all(&app_data_dir) {
        eprintln!("创建统一应用数据目录失败：{error}");
    }
    app_data_dir
}

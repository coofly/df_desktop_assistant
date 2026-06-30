//! 处理 Windows 开机自启动注册表读写。

use std::{io, path::Path, process::Command};

use crate::app_log;

const STARTUP_ENTRY_NAME: &str = "Delta Force Craft Assistant";
const STARTUP_REGISTRY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_REGISTRY_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// 为注册表启动项生成带引号的程序路径。
fn quoted_startup_exe_path(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.to_string_lossy())
}

/// 构造 reg.exe 写入或删除启动项所需参数。
fn startup_registry_command(enable: bool, exe_path: &Path) -> Vec<String> {
    if enable {
        vec![
            "add".to_string(),
            STARTUP_REGISTRY_PATH.to_string(),
            "/v".to_string(),
            STARTUP_ENTRY_NAME.to_string(),
            "/t".to_string(),
            "REG_SZ".to_string(),
            "/d".to_string(),
            quoted_startup_exe_path(exe_path),
            "/f".to_string(),
        ]
    } else {
        vec![
            "delete".to_string(),
            STARTUP_REGISTRY_PATH.to_string(),
            "/v".to_string(),
            STARTUP_ENTRY_NAME.to_string(),
            "/f".to_string(),
        ]
    }
}

/// 将 reg.exe 的执行状态转换为面向前端的结果。
fn startup_registry_status_result(enable: bool, success: bool) -> Result<(), String> {
    if success {
        Ok(())
    } else if enable {
        Err("开机自启动开启失败，请确认当前账号有权限写入用户启动项".to_string())
    } else {
        Err("开机自启动关闭失败，请确认当前账号有权限删除用户启动项".to_string())
    }
}

/// 判断注册表启动项值是否代表已开启自启动。
fn startup_registry_value_to_enabled(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

/// 从 Windows 注册表读取启动项原始值。
fn read_startup_registry_value() -> Result<Option<String>, String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
            REG_SZ,
        },
    };

    unsafe {
        let mut key: HKEY = null_mut();
        let open_result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_null(STARTUP_REGISTRY_SUBKEY).as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut key,
        );

        if open_result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }

        if open_result != ERROR_SUCCESS {
            return Err(registry_error_message(
                "读取开机自启动状态失败",
                open_result,
            ));
        }

        let value_name = wide_null(STARTUP_ENTRY_NAME);
        let mut value_type = 0;
        let mut value_size = 0;
        let size_result = RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut value_type,
            null_mut(),
            &mut value_size,
        );

        if size_result == ERROR_FILE_NOT_FOUND {
            RegCloseKey(key);
            return Ok(None);
        }

        if size_result != ERROR_SUCCESS && size_result != ERROR_MORE_DATA {
            RegCloseKey(key);
            return Err(registry_error_message(
                "读取开机自启动状态失败",
                size_result,
            ));
        }

        if value_type != REG_SZ || value_size == 0 {
            RegCloseKey(key);
            return Ok(None);
        }

        let mut buffer = vec![0u8; value_size as usize];
        let query_result = RegQueryValueExW(
            key,
            value_name.as_ptr(),
            std::ptr::null(),
            &mut value_type,
            buffer.as_mut_ptr(),
            &mut value_size,
        );
        RegCloseKey(key);

        if query_result == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }

        if query_result != ERROR_SUCCESS {
            return Err(registry_error_message(
                "读取开机自启动状态失败",
                query_result,
            ));
        }

        if value_type != REG_SZ {
            return Ok(None);
        }

        let len = (value_size as usize / 2).min(buffer.len() / 2);
        let wide: Vec<u16> = buffer
            .chunks_exact(2)
            .take(len)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .take_while(|character| *character != 0)
            .collect();

        Ok(Some(String::from_utf16_lossy(&wide)))
    }
}

/// 读取当前账号是否启用了开机自启动。
pub(crate) fn read_launch_on_startup() -> Result<bool, String> {
    let value = read_startup_registry_value()?;
    Ok(startup_registry_value_to_enabled(value.as_deref()))
}

/// 格式化 Windows 注册表 API 错误信息。
fn registry_error_message(context: &str, code: u32) -> String {
    format!(
        "{context}：{}（Windows 错误码 {code}）",
        io::Error::from_raw_os_error(code as i32)
    )
}

/// 应用开机自启动开关到用户注册表启动项。
pub(crate) fn apply_launch_on_startup(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|error| error.to_string())?;
    if !enable && !read_launch_on_startup()? {
        app_log::debug("开机自启动已处于关闭状态，跳过删除启动项");
        return Ok(());
    }

    let args = startup_registry_command(enable, &exe_path);
    app_log::debug(format!(
        "开始{}开机自启动，程序路径：{}",
        if enable { "开启" } else { "关闭" },
        exe_path.display()
    ));
    let status = Command::new("reg.exe")
        .args(args)
        .status()
        .map_err(|error| error.to_string())?;

    if status.success() {
        app_log::debug(format!(
            "开机自启动{}操作完成",
            if enable { "开启" } else { "关闭" }
        ));
    } else {
        app_log::warn(format!(
            "开机自启动{}操作返回失败状态：{}",
            if enable { "开启" } else { "关闭" },
            status
        ));
    }

    startup_registry_status_result(enable, status.success())
}

/// 将普通字符串转换为 Windows API 需要的空结尾宽字符串。
fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_registration_builds_expected_commands() {
        let exe_path = std::path::Path::new(r"C:\Program Files\Delta Force\assistant.exe");

        let enable = startup_registry_command(true, exe_path);
        let disable = startup_registry_command(false, exe_path);

        assert_eq!(
            enable,
            vec![
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Delta Force Craft Assistant",
                "/t",
                "REG_SZ",
                "/d",
                r#""C:\Program Files\Delta Force\assistant.exe""#,
                "/f"
            ]
        );
        assert_eq!(
            disable,
            vec![
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Delta Force Craft Assistant",
                "/f"
            ]
        );
    }

    #[test]
    fn startup_registry_status_treats_delete_failure_as_error() {
        assert!(startup_registry_status_result(false, true).is_ok());
        assert!(startup_registry_status_result(false, false).is_err());
        assert!(startup_registry_status_result(true, false).is_err());
    }

    #[test]
    fn startup_registry_value_controls_startup_status() {
        assert!(startup_registry_value_to_enabled(Some(
            r#""C:\Program Files\Delta Force\assistant.exe""#
        )));
        assert!(!startup_registry_value_to_enabled(Some("")));
        assert!(!startup_registry_value_to_enabled(None));
    }
}

//! 处理 Windows 开机自启动注册表读写。

use std::{io, path::Path};

use crate::app_log;

const STARTUP_ENTRY_NAME: &str = "Delta Force Craft Assistant";
const STARTUP_REGISTRY_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// 为注册表启动项生成带引号的程序路径。
fn quoted_startup_exe_path(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.to_string_lossy())
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

/// 使用 Windows API 写入当前用户的开机自启动项。
fn write_startup_registry_value(exe_path: &Path) -> Result<(), String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
            REG_OPTION_NON_VOLATILE, REG_SZ,
        },
    };

    unsafe {
        let mut key: HKEY = null_mut();
        let mut disposition = 0;
        let create_result = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide_null(STARTUP_REGISTRY_SUBKEY).as_ptr(),
            0,
            null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null(),
            &mut key,
            &mut disposition,
        );
        if create_result != ERROR_SUCCESS {
            return Err(registry_error_message(
                "打开开机自启动注册表项失败",
                create_result,
            ));
        }

        let value_name = wide_null(STARTUP_ENTRY_NAME);
        let value_data = wide_null(&quoted_startup_exe_path(exe_path));
        let set_result = RegSetValueExW(
            key,
            value_name.as_ptr(),
            0,
            REG_SZ,
            value_data.as_ptr().cast(),
            (value_data.len() * std::mem::size_of::<u16>()) as u32,
        );
        RegCloseKey(key);

        if set_result != ERROR_SUCCESS {
            return Err(registry_error_message(
                "写入开机自启动注册表值失败",
                set_result,
            ));
        }
        Ok(())
    }
}

/// 使用 Windows API 删除当前用户的开机自启动项。
fn delete_startup_registry_value() -> Result<(), String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
        },
    };

    unsafe {
        let mut key: HKEY = null_mut();
        let open_result = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            wide_null(STARTUP_REGISTRY_SUBKEY).as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut key,
        );
        if open_result == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if open_result != ERROR_SUCCESS {
            return Err(registry_error_message(
                "打开开机自启动注册表项失败",
                open_result,
            ));
        }

        let delete_result = RegDeleteValueW(key, wide_null(STARTUP_ENTRY_NAME).as_ptr());
        RegCloseKey(key);
        if delete_result != ERROR_SUCCESS && delete_result != ERROR_FILE_NOT_FOUND {
            return Err(registry_error_message(
                "删除开机自启动注册表值失败",
                delete_result,
            ));
        }
        Ok(())
    }
}

/// 应用开机自启动开关到用户注册表启动项。
pub(crate) fn apply_launch_on_startup(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|error| error.to_string())?;
    if !enable && !read_launch_on_startup()? {
        app_log::debug("开机自启动已处于关闭状态，跳过删除启动项");
        return Ok(());
    }

    app_log::debug(format!(
        "开始{}开机自启动，程序路径：{}",
        if enable { "开启" } else { "关闭" },
        exe_path.display()
    ));
    if enable {
        write_startup_registry_value(&exe_path)?;
    } else {
        delete_startup_registry_value()?;
    }
    app_log::debug(format!(
        "开机自启动{}操作完成",
        if enable { "开启" } else { "关闭" }
    ));
    Ok(())
}

/// 将普通字符串转换为 Windows API 需要的空结尾宽字符串。
fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证启动程序路径始终使用双引号包裹。
    #[test]
    fn startup_registration_quotes_executable_path() {
        let exe_path = std::path::Path::new(r"C:\Program Files\Delta Force\assistant.exe");

        assert_eq!(
            quoted_startup_exe_path(exe_path),
            r#""C:\Program Files\Delta Force\assistant.exe""#
        );
    }

    /// 验证注册表中非空字符串代表自启动已开启。
    #[test]
    fn startup_registry_value_controls_startup_status() {
        assert!(startup_registry_value_to_enabled(Some(
            r#""C:\Program Files\Delta Force\assistant.exe""#
        )));
        assert!(!startup_registry_value_to_enabled(Some("")));
        assert!(!startup_registry_value_to_enabled(None));
    }
}

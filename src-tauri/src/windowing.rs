//! 管理主窗口桌面挂件行为、位置持久化和设置窗口定位。

use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::{
    app_log, lock_value,
    models::{WindowPosition, WindowStateFile},
    read_window_state_file, write_window_state_file, AssistantState, MAIN_WINDOW_LABEL,
    SETTINGS_WINDOW_HEIGHT, SETTINGS_WINDOW_LABEL, SETTINGS_WINDOW_MAIN_GAP, SETTINGS_WINDOW_WIDTH,
};

/// 启动时恢复主窗口上次保存的位置。
pub(crate) fn restore_main_window_position(app: &tauri::AppHandle) {
    let state = app.state::<AssistantState>();
    let window_state = match read_window_state_file(&state.window_state_path) {
        Ok(window_state) => window_state,
        Err(error) => {
            app_log::warn(format!("读取窗口状态失败：{error}"));
            return;
        }
    };
    let Some(position) = window_state.position else {
        return;
    };

    if position.x <= -30_000 || position.y <= -30_000 {
        app_log::warn(format!(
            "忽略疑似隐藏窗口位置：x={}，y={}",
            position.x, position.y
        ));
        return;
    }

    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let result = window.set_position(tauri::PhysicalPosition::new(position.x, position.y));

        match result {
            Ok(_) => app_log::info(format!(
                "已恢复主窗口位置：x={}，y={}",
                position.x, position.y
            )),
            Err(error) => app_log::warn(format!("恢复主窗口位置失败：{error}")),
        }
    } else {
        app_log::warn("恢复主窗口位置失败：没有找到主窗口");
    }
}

/// 在主窗口移动后保存屏幕坐标和锁定状态。
pub(crate) fn persist_main_window_position(
    window: &tauri::Window,
    state: State<'_, AssistantState>,
    fallback_position: &tauri::PhysicalPosition<i32>,
) {
    let position = main_window_screen_position(window).unwrap_or(WindowPosition {
        x: fallback_position.x,
        y: fallback_position.y,
    });

    if position.x <= -30_000 || position.y <= -30_000 {
        app_log::debug(format!(
            "跳过保存疑似隐藏窗口位置：x={}，y={}",
            position.x, position.y
        ));
        return;
    }

    let window_state = WindowStateFile {
        position: Some(position),
        locked: lock_value(&state),
    };

    if let Err(error) = write_window_state_file(&state.window_state_path, &window_state) {
        app_log::warn(format!("保存主窗口状态失败：{error}"));
    }
}

/// 通过 HWND 读取主窗口的真实屏幕坐标。
fn main_window_screen_position(window: &tauri::Window) -> Option<WindowPosition> {
    use windows_sys::Win32::{Foundation::RECT, UI::WindowsAndMessaging::GetWindowRect};

    let Ok(hwnd) = window.hwnd() else {
        app_log::warn("保存主窗口屏幕位置失败：无法获取 HWND");
        return None;
    };

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    let success = unsafe { GetWindowRect(hwnd.0 as _, &mut rect) } != 0;
    if !success {
        app_log::warn("保存主窗口屏幕位置失败：GetWindowRect 调用失败");
        return None;
    }

    Some(WindowPosition {
        x: rect.left,
        y: rect.top,
    })
}

/// 将主窗口应用为桌面挂件层级。
fn apply_main_window_desktop_layer(window: &tauri::WebviewWindow) {
    apply_main_window_desktop_widget_behavior(window);
    apply_square_window_region(window, "主窗口桌面挂件行为");

    if let Err(error) = window.set_always_on_top(false) {
        app_log::warn(format!("取消主窗口置顶失败：{error}"));
    }
}

/// 使用 WebviewWindow 句柄应用直角窗口区域。
fn apply_square_window_region(window: &tauri::WebviewWindow, context: &str) {
    let Ok(hwnd) = window.hwnd() else {
        app_log::warn(format!("{context}设置直角窗口区域失败：无法获取 HWND"));
        return;
    };

    apply_square_hwnd_region(hwnd.0 as _, context);
}

/// 使用普通 Window 句柄应用直角窗口区域。
pub(crate) fn apply_square_tauri_window_region(window: &tauri::Window, context: &str) {
    let Ok(hwnd) = window.hwnd() else {
        app_log::warn(format!("{context}设置直角窗口区域失败：无法获取 HWND"));
        return;
    };

    apply_square_hwnd_region(hwnd.0 as _, context);
}

/// 根据 HWND 设置直角窗口区域，避免透明窗口圆角漏边。
fn apply_square_hwnd_region(hwnd: windows_sys::Win32::Foundation::HWND, context: &str) {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{CreateRectRgn, SetWindowRgn},
        UI::WindowsAndMessaging::GetClientRect,
    };

    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };

    unsafe {
        if GetClientRect(hwnd, &mut rect) == 0 {
            app_log::warn(format!(
                "{context}设置直角窗口区域失败：GetClientRect 调用失败"
            ));
            return;
        }

        let region = CreateRectRgn(0, 0, rect.right - rect.left, rect.bottom - rect.top);
        if region.is_null() {
            app_log::warn(format!(
                "{context}设置直角窗口区域失败：CreateRectRgn 调用失败"
            ));
            return;
        }

        if SetWindowRgn(hwnd, region, 1) == 0 {
            app_log::warn(format!(
                "{context}设置直角窗口区域失败：SetWindowRgn 调用失败"
            ));
        }
    }
}

/// 把主窗口挂到 Windows 桌面层并隐藏任务栏入口。
fn apply_main_window_desktop_widget_behavior(window: &tauri::WebviewWindow) {
    use std::ptr::null;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, RECT},
        UI::WindowsAndMessaging::{
            EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindow, GetWindowLongPtrW,
            GetWindowRect, SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos,
            GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT, HWND_TOP, SMTO_NORMAL, SWP_FRAMECHANGED,
            SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER,
            SWP_SHOWWINDOW, WS_CHILD, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_POPUP,
        },
    };

    let Ok(hwnd) = window.hwnd() else {
        app_log::warn("设置主窗口桌面挂件行为失败：无法获取 HWND");
        return;
    };
    let hwnd = hwnd.0 as HWND;

    unsafe {
        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let widget_ex_style = (ex_style | WS_EX_TOOLWINDOW as isize) & !(WS_EX_APPWINDOW as isize);

        if widget_ex_style != ex_style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, widget_ex_style);
        }

        attach_main_window_to_desktop(hwnd);

        let _ = SetWindowPos(
            hwnd,
            HWND_TOP,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_FRAMECHANGED,
        );
    }

    app_log::info("已应用主窗口桌面挂件行为");

    /// 将目标窗口挂载到桌面 WorkerW/Progman 父窗口。
    unsafe fn attach_main_window_to_desktop(target_hwnd: HWND) {
        let Some(desktop_hwnd) = find_desktop_parent_window() else {
            app_log::warn("未找到可挂载的桌面窗口，跳过 SetParent");
            return;
        };

        let mut target_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let mut desktop_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let has_target_rect = GetWindowRect(target_hwnd, &mut target_rect) != 0;
        let has_desktop_rect = GetWindowRect(desktop_hwnd, &mut desktop_rect) != 0;

        let previous_parent = SetParent(target_hwnd, desktop_hwnd);

        let style = GetWindowLongPtrW(target_hwnd, GWL_STYLE);
        let desktop_child_style = (style | WS_CHILD as isize) & !(WS_POPUP as isize);
        if desktop_child_style != style {
            SetWindowLongPtrW(target_hwnd, GWL_STYLE, desktop_child_style);
        }

        if has_target_rect && has_desktop_rect {
            let width = target_rect.right - target_rect.left;
            let height = target_rect.bottom - target_rect.top;
            let mut x = target_rect.left - desktop_rect.left;
            let mut y = target_rect.top - desktop_rect.top;
            let parent_width = desktop_rect.right - desktop_rect.left;
            let parent_height = desktop_rect.bottom - desktop_rect.top;
            let needs_recenter =
                x + width < 0 || y + height < 0 || x > parent_width || y > parent_height;

            if needs_recenter || width <= 0 || height <= 0 {
                x = (parent_width - width.max(1)).max(0) / 2;
                y = (parent_height - height.max(1)).max(0) / 2;
            }

            let _ = SetWindowPos(
                target_hwnd,
                HWND_TOP,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW | SWP_FRAMECHANGED,
            );
        }

        app_log::info(format!(
            "主窗口已挂载到桌面窗口：desktop={:#x} class=\"{}\" previous={:#x}",
            desktop_hwnd as usize,
            window_class_name(desktop_hwnd),
            previous_parent as usize
        ));
    }

    /// 查找可承载桌面挂件的 WorkerW 或 Progman 窗口。
    unsafe fn find_desktop_parent_window() -> Option<HWND> {
        let progman = FindWindowW(wide_null("Progman").as_ptr(), null());
        if !progman.is_null() {
            let mut result = 0usize;
            let _ = SendMessageTimeoutW(progman, 0x052c, 0, 0, SMTO_NORMAL, 1000, &mut result);
        }

        let mut search = DesktopParentSearch {
            shell_view_parent: null_mut_hwnd(),
            worker_after_shell_view: null_mut_hwnd(),
            first_worker: null_mut_hwnd(),
            progman,
        };

        EnumWindows(
            Some(enum_desktop_parent_candidate),
            (&mut search as *mut DesktopParentSearch) as LPARAM,
        );

        if !search.worker_after_shell_view.is_null() {
            Some(search.worker_after_shell_view)
        } else if is_allowed_desktop_parent(search.shell_view_parent) {
            Some(search.shell_view_parent)
        } else if is_allowed_desktop_parent(search.first_worker) {
            Some(search.first_worker)
        } else if is_allowed_desktop_parent(progman) {
            Some(progman)
        } else {
            None
        }
    }

    /// 记录桌面父窗口枚举过程中的候选窗口。
    struct DesktopParentSearch {
        shell_view_parent: HWND,
        worker_after_shell_view: HWND,
        first_worker: HWND,
        progman: HWND,
    }

    /// 枚举顶层窗口并挑选桌面父窗口候选。
    unsafe extern "system" fn enum_desktop_parent_candidate(hwnd: HWND, lparam: LPARAM) -> i32 {
        let search = &mut *(lparam as *mut DesktopParentSearch);
        let class_name = window_class_name(hwnd);

        if class_name.eq_ignore_ascii_case("WorkerW") && search.first_worker.is_null() {
            search.first_worker = hwnd;
        }

        if is_allowed_desktop_parent(hwnd) && has_direct_shell_view_child(hwnd) {
            search.shell_view_parent = hwnd;

            let next = GetWindow(hwnd, GW_HWNDNEXT);
            if !next.is_null() && window_class_name(next).eq_ignore_ascii_case("WorkerW") {
                search.worker_after_shell_view = next;
            }

            return 0;
        }

        if hwnd == search.progman && has_direct_shell_view_child(hwnd) {
            search.shell_view_parent = hwnd;
            return 0;
        }

        1
    }

    /// 判断窗口是否直接包含桌面图标视图。
    unsafe fn has_direct_shell_view_child(hwnd: HWND) -> bool {
        !FindWindowExW(
            hwnd,
            null_mut_hwnd(),
            wide_null("SHELLDLL_DefView").as_ptr(),
            null(),
        )
        .is_null()
    }

    /// 判断窗口类名是否允许作为桌面父窗口。
    unsafe fn is_allowed_desktop_parent(hwnd: HWND) -> bool {
        if hwnd.is_null() {
            return false;
        }

        let class_name = window_class_name(hwnd);
        class_name.eq_ignore_ascii_case("Progman") || class_name.eq_ignore_ascii_case("WorkerW")
    }

    /// 读取 Windows 窗口类名。
    unsafe fn window_class_name(hwnd: HWND) -> String {
        let mut buffer = [0u16; 256];
        let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        wide_buffer_to_string(&buffer, len)
    }

    /// 将 Windows 宽字符缓冲区转为可记录字符串。
    fn wide_buffer_to_string(buffer: &[u16], len: i32) -> String {
        if len <= 0 {
            return String::new();
        }

        String::from_utf16_lossy(&buffer[..len as usize])
            .chars()
            .filter(|character| !character.is_control())
            .take(80)
            .collect()
    }
}

/// 将普通字符串转换为 Windows API 需要的空结尾宽字符串。
fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 返回空的可变 HWND 指针。
fn null_mut_hwnd() -> windows_sys::Win32::Foundation::HWND {
    std::ptr::null_mut()
}

/// 对主窗口应用桌面挂件层级。
pub(crate) fn apply_main_window_desktop_layer_by_app(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        apply_main_window_desktop_layer(&window);
    } else {
        app_log::warn("设置主窗口桌面挂件层失败：没有找到主窗口");
    }
}

/// 根据锁定状态更新主窗口缩放和直角区域。
pub(crate) fn apply_main_window_lock_state(app: &tauri::AppHandle, locked: bool) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.set_resizable(!locked) {
            app_log::warn(format!("更新主窗口缩放状态失败：{error}"));
        }

        apply_square_window_region(&window, "更新主窗口锁定状态后");
    } else {
        app_log::warn("更新主窗口缩放状态失败：没有找到主窗口");
    }
}

/// 计算设置窗口相对主窗口的贴靠位置。
fn anchored_popup_position(
    main_position: tauri::PhysicalPosition<i32>,
    main_size: tauri::PhysicalSize<u32>,
    popup_size: tauri::PhysicalSize<u32>,
    work_area_position: tauri::PhysicalPosition<i32>,
) -> tauri::PhysicalPosition<i32> {
    let work_top = work_area_position.y;
    let popup_height = popup_size.height as i32;
    let main_height = main_size.height as i32;

    let y = if main_position.y - popup_height - SETTINGS_WINDOW_MAIN_GAP >= work_top {
        main_position.y - popup_height - SETTINGS_WINDOW_MAIN_GAP
    } else {
        main_position.y + main_height + SETTINGS_WINDOW_MAIN_GAP
    };

    tauri::PhysicalPosition::new(main_position.x, y)
}

/// 将设置窗口定位到主窗口上方或下方。
fn position_popup_window_next_to_main(
    main_window: &tauri::WebviewWindow,
    popup_window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let main_position = main_window
        .outer_position()
        .map_err(|error| error.to_string())?;
    let main_size = main_window
        .outer_size()
        .map_err(|error| error.to_string())?;
    let popup_size = popup_window
        .outer_size()
        .map_err(|error| error.to_string())?;
    let monitor = main_window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "无法获取主窗口所在显示器".to_string())?;
    let work_area = monitor.work_area();
    let position =
        anchored_popup_position(main_position, main_size, popup_size, work_area.position);

    popup_window
        .set_position(position)
        .map_err(|error| error.to_string())?;
    app_log::debug(format!(
        "设置窗口已贴靠主窗口：x={}，y={}",
        position.x, position.y
    ));
    Ok(())
}

/// 在创建或显示设置窗口时贴靠主窗口。
fn place_settings_window(app: &tauri::AppHandle, settings_window: &tauri::WebviewWindow) {
    let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        app_log::warn("设置窗口定位失败：没有找到主窗口");
        return;
    };

    if let Err(error) = position_popup_window_next_to_main(&main_window, settings_window) {
        app_log::warn(format!("设置窗口定位失败：{error}"));
    }
}

/// 显示已有设置窗口或创建新的设置窗口。
pub(crate) fn open_settings_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        app_log::info("显示设置窗口");
        place_settings_window(app, &window);
        let _ = window.set_always_on_top(true);
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }

    app_log::debug("开始创建设置窗口");
    let result = WebviewWindowBuilder::new(
        app,
        SETTINGS_WINDOW_LABEL,
        WebviewUrl::App("/settings".into()),
    )
    .title("设置")
    .inner_size(SETTINGS_WINDOW_WIDTH, SETTINGS_WINDOW_HEIGHT)
    .min_inner_size(320.0, 320.0)
    .decorations(false)
    .resizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .transparent(true)
    .shadow(false)
    .build()
    .and_then(|window| {
        place_settings_window(app, &window);
        let _ = window.set_focus();
        Ok(window)
    });

    match result {
        Ok(_) => app_log::info("设置窗口创建成功"),
        Err(error) => app_log::error(format!("设置窗口创建失败：{error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_position_uses_space_above_main_window_first() {
        let position = anchored_popup_position(
            tauri::PhysicalPosition::new(200, 500),
            tauri::PhysicalSize::new(1000, 260),
            tauri::PhysicalSize::new(360, 340),
            tauri::PhysicalPosition::new(0, 0),
        );

        assert_eq!(position, tauri::PhysicalPosition::new(200, 157));
    }

    #[test]
    fn popup_position_falls_below_main_window_without_top_space() {
        let position = anchored_popup_position(
            tauri::PhysicalPosition::new(200, 120),
            tauri::PhysicalSize::new(1000, 260),
            tauri::PhysicalSize::new(360, 340),
            tauri::PhysicalPosition::new(0, 0),
        );

        assert_eq!(position, tauri::PhysicalPosition::new(200, 383));
    }

    #[test]
    fn popup_position_aligns_left_edge_with_main_window() {
        let position = anchored_popup_position(
            tauri::PhysicalPosition::new(1780, 500),
            tauri::PhysicalSize::new(1000, 260),
            tauri::PhysicalSize::new(360, 340),
            tauri::PhysicalPosition::new(0, 0),
        );

        assert_eq!(position, tauri::PhysicalPosition::new(1780, 157));
    }
}

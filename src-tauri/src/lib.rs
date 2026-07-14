//! 组装 Tauri 应用、命令入口和助手共享状态。

mod app_data;
mod app_log;
mod image_cache;
mod kkrb;
mod models;
mod recommendations;
mod startup;
mod windowing;

use models::{
    AssistantSettings, AssistantSnapshot, CraftRecommendation, StoredAssistantSettings,
    WindowStateFile,
};
use recommendations::{mock_recommendations, snapshot_with_recommendations};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
    Emitter, Manager, State, WindowEvent,
};
use windowing::{
    apply_main_window_desktop_layer_by_app, apply_main_window_lock_state,
    apply_square_tauri_window_region, open_settings_window, persist_main_window_position,
    restore_main_window_position,
};

const MAIN_WINDOW_LABEL: &str = "main";
pub(crate) const SETTINGS_WINDOW_LABEL: &str = "settings";
const TRAY_SETTINGS: &str = "open_settings";
const TRAY_LOCK: &str = "lock_window";
const TRAY_UNLOCK: &str = "unlock_window";
const TRAY_QUIT: &str = "quit_app";
const SETTINGS_FILE_NAME: &str = "settings.json";
const WINDOW_STATE_FILE_NAME: &str = "window-state.json";
const SETTINGS_UPDATED_EVENT: &str = "settings-updated";
pub(crate) const SETTINGS_WINDOW_WIDTH: f64 = 360.0;
pub(crate) const SETTINGS_WINDOW_HEIGHT: f64 = 390.0;
pub(crate) const SETTINGS_WINDOW_MAIN_GAP: i32 = 3;

/// 持有 Tauri 命令共享的助手状态。
pub(crate) struct AssistantState {
    locked: Mutex<bool>,
    settings: Mutex<AssistantSettings>,
    settings_path: PathBuf,
    pub(crate) window_state_path: PathBuf,
    cached_recommendations: Mutex<Option<Vec<CraftRecommendation>>>,
    image_cache: image_cache::ImageCache,
    kkrb_client: kkrb::KkrbClient,
}

impl AssistantState {
    /// 根据应用数据目录初始化共享状态。
    fn new(app_data_dir: PathBuf) -> Self {
        let settings_path = app_data_dir.join(SETTINGS_FILE_NAME);
        let window_state_path = app_data_dir.join(WINDOW_STATE_FILE_NAME);
        let settings = read_settings(&settings_path).unwrap_or_else(|error| {
            app_log::warn(format!("设置读取失败，将使用默认设置：{error}"));
            AssistantSettings::default()
        });
        let window_state = read_window_state_file(&window_state_path).unwrap_or_default();
        app_log::debug(format!(
            "助手状态初始化完成，设置文件路径：{}，窗口状态路径：{}，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%，锁定位置：{}",
            settings_path.display(),
            window_state_path.display(),
            settings.refresh_interval_minutes,
            enabled_text(settings.launch_on_startup),
            settings.main_window_opacity_percent,
            enabled_text(window_state.locked)
        ));

        Self {
            locked: Mutex::new(window_state.locked),
            settings: Mutex::new(settings),
            settings_path,
            window_state_path,
            cached_recommendations: Mutex::new(None),
            image_cache: image_cache::ImageCache::new(&app_data_dir),
            kkrb_client: kkrb::KkrbClient::new(app_data_dir.join("kkrb_cookies.txt")),
        }
    }
}

impl Default for AssistantState {
    /// 使用默认应用数据目录创建助手状态。
    fn default() -> Self {
        Self::new(default_app_data_dir())
    }
}

/// 计算助手使用的应用数据目录。
fn default_app_data_dir() -> PathBuf {
    app_data::canonical_app_data_dir()
}

/// 将布尔开关转换为中文日志文本。
fn enabled_text(enabled: bool) -> &'static str {
    if enabled {
        "开启"
    } else {
        "关闭"
    }
}

/// 读取设置文件并兼容缺失字段。
fn read_settings_file(path: &Path) -> Result<AssistantSettings, String> {
    if !path.exists() {
        app_log::debug(format!(
            "未找到设置文件，将使用默认设置：{}",
            path.display()
        ));
        return Ok(AssistantSettings::default());
    }

    app_log::debug(format!("开始读取设置文件：{}", path.display()));
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let stored_settings: StoredAssistantSettings =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let default_settings = AssistantSettings::default();
    let settings = AssistantSettings::new(
        stored_settings
            .refresh_interval_minutes
            .unwrap_or(default_settings.refresh_interval_minutes),
        stored_settings
            .launch_on_startup
            .unwrap_or(default_settings.launch_on_startup),
        stored_settings
            .main_window_opacity_percent
            .unwrap_or(default_settings.main_window_opacity_percent),
    );
    app_log::debug(format!(
        "设置文件读取成功，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
        settings.refresh_interval_minutes,
        enabled_text(settings.launch_on_startup),
        settings.main_window_opacity_percent
    ));

    Ok(settings)
}

/// 读取设置文件并同步真实开机自启动状态。
fn read_settings(path: &Path) -> Result<AssistantSettings, String> {
    let mut settings = read_settings_file(path)?;
    settings.launch_on_startup = match startup::read_launch_on_startup() {
        Ok(launch_on_startup) => launch_on_startup,
        Err(error) => {
            app_log::warn(format!(
                "读取真实开机自启动状态失败，将按未开启处理：{error}"
            ));
            false
        }
    };
    Ok(settings)
}

/// 将设置持久化到 JSON 文件。
fn write_settings_file(path: &Path, settings: &AssistantSettings) -> Result<(), String> {
    app_log::debug(format!(
        "开始写入设置文件：{}，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
        path.display(),
        settings.refresh_interval_minutes,
        enabled_text(settings.launch_on_startup),
        settings.main_window_opacity_percent
    ));

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    app_log::debug("设置文件写入成功");
    Ok(())
}

/// 读取主窗口状态文件。
pub(crate) fn read_window_state_file(path: &Path) -> Result<WindowStateFile, String> {
    if !path.exists() {
        app_log::debug(format!("未找到窗口状态文件：{}", path.display()));
        return Ok(WindowStateFile::default());
    }

    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let window_state: WindowStateFile =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let position_text = window_state
        .position
        .as_ref()
        .map(|position| format!("x={}，y={}", position.x, position.y))
        .unwrap_or_else(|| "无位置".to_string());
    app_log::debug(format!(
        "窗口状态读取成功：{}，锁定位置：{}",
        position_text,
        enabled_text(window_state.locked)
    ));
    Ok(window_state)
}

/// 写入主窗口状态文件。
pub(crate) fn write_window_state_file(
    path: &Path,
    window_state: &WindowStateFile,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let text = serde_json::to_string_pretty(window_state).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    app_log::debug(format!(
        "窗口状态写入成功：锁定位置：{}",
        enabled_text(window_state.locked)
    ));
    Ok(())
}

/// 读取当前窗口锁定状态。
pub(crate) fn lock_value(state: &AssistantState) -> bool {
    *state.locked.lock().expect("assistant state lock poisoned")
}

/// 更新当前窗口锁定状态并返回新值。
fn set_lock_value(state: &AssistantState, locked: bool) -> bool {
    let mut lock = state.locked.lock().expect("assistant state lock poisoned");
    *lock = locked;
    *lock
}

/// 将窗口锁定状态写入窗口状态文件。
fn persist_window_lock_state(state: &AssistantState, locked: bool) {
    let mut window_state = read_window_state_file(&state.window_state_path).unwrap_or_default();
    window_state.locked = locked;

    if let Err(error) = write_window_state_file(&state.window_state_path, &window_state) {
        app_log::warn(format!("保存窗口锁定状态失败：{error}"));
    }
}

#[tauri::command]
/// 获取制作推荐快照并在失败时回退到缓存或 mock。
fn get_recommendations(state: State<'_, AssistantState>) -> AssistantSnapshot {
    app_log::debug("前端请求获取制作推荐");
    let locked = lock_value(&state);

    match state.kkrb_client.fetch_live_recommendations() {
        Ok(result) => {
            let mut recommendations = result.recommendations;
            state
                .image_cache
                .cache_recommendation_images(&mut recommendations);
            app_log::info(format!(
                "制作推荐获取成功，来源：kkrb 实时数据，session：{}，数量：{}",
                result.session_source.label(),
                recommendations.len(),
            ));
            *state
                .cached_recommendations
                .lock()
                .expect("assistant cache lock poisoned") = Some(recommendations.clone());

            snapshot_with_recommendations(locked, "kkrb 实时数据", recommendations)
        }
        Err(error) => {
            app_log::warn(format!("kkrb 实时数据获取失败：{error}"));
            let cached = state
                .cached_recommendations
                .lock()
                .expect("assistant cache lock poisoned")
                .clone();

            if let Some(recommendations) = cached {
                app_log::info(format!(
                    "使用内存缓存推荐数据，数量：{}",
                    recommendations.len()
                ));
                snapshot_with_recommendations(
                    locked,
                    format!("kkrb 获取失败，使用缓存：{error}"),
                    recommendations,
                )
            } else {
                app_log::info("没有可用缓存，使用本地 mock 推荐数据");
                snapshot_with_recommendations(
                    locked,
                    format!("kkrb 获取失败，使用 mock：{error}"),
                    mock_recommendations(),
                )
            }
        }
    }
}

#[tauri::command]
/// 返回当前主窗口是否锁定位置。
fn get_lock_state(state: State<'_, AssistantState>) -> bool {
    let locked = lock_value(&state);
    app_log::debug(format!("前端读取锁定状态：{}", enabled_text(locked)));
    locked
}

#[tauri::command]
/// 更新主窗口位置锁定状态并持久化。
fn set_lock_state(app: tauri::AppHandle, state: State<'_, AssistantState>, locked: bool) -> bool {
    app_log::info(format!(
        "窗口位置已{}",
        if locked { "锁定" } else { "解锁" }
    ));
    let locked = set_lock_value(&state, locked);
    persist_window_lock_state(&state, locked);
    apply_main_window_lock_state(&app, locked);
    locked
}

#[tauri::command]
/// 读取助手设置并同步真实自启动状态。
fn get_settings(state: State<'_, AssistantState>) -> AssistantSettings {
    let mut settings = state
        .settings
        .lock()
        .expect("assistant settings lock poisoned")
        .clone();
    match startup::read_launch_on_startup() {
        Ok(launch_on_startup) => {
            settings.launch_on_startup = launch_on_startup;
            *state
                .settings
                .lock()
                .expect("assistant settings lock poisoned") = settings.clone();
        }
        Err(error) => {
            app_log::warn(format!(
                "读取真实开机自启动状态失败，将按未开启处理：{error}"
            ));
            settings.launch_on_startup = false;
            *state
                .settings
                .lock()
                .expect("assistant settings lock poisoned") = settings.clone();
        }
    }
    app_log::debug(format!(
        "前端读取设置，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
        settings.refresh_interval_minutes,
        enabled_text(settings.launch_on_startup),
        settings.main_window_opacity_percent
    ));
    settings
}

#[tauri::command]
/// 保存助手设置并通知主窗口刷新设置。
fn save_settings(
    app: tauri::AppHandle,
    state: State<'_, AssistantState>,
    refresh_interval_minutes: u16,
    launch_on_startup: bool,
    main_window_opacity_percent: u8,
) -> Result<AssistantSettings, String> {
    app_log::debug(format!(
        "前端请求保存设置，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
        refresh_interval_minutes,
        enabled_text(launch_on_startup),
        main_window_opacity_percent
    ));
    let settings = AssistantSettings::new(
        refresh_interval_minutes,
        launch_on_startup,
        main_window_opacity_percent,
    );

    startup::apply_launch_on_startup(settings.launch_on_startup)?;
    let settings = AssistantSettings::new(
        settings.refresh_interval_minutes,
        startup::read_launch_on_startup()?,
        settings.main_window_opacity_percent,
    );
    write_settings_file(&state.settings_path, &settings)?;
    *state
        .settings
        .lock()
        .expect("assistant settings lock poisoned") = settings.clone();
    let _ = app.emit_to(MAIN_WINDOW_LABEL, SETTINGS_UPDATED_EVENT, settings.clone());
    app_log::info(format!(
        "设置保存成功，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
        settings.refresh_interval_minutes,
        enabled_text(settings.launch_on_startup),
        settings.main_window_opacity_percent
    ));

    Ok(settings)
}

#[tauri::command]
/// 从前端请求启动主窗口拖动。
fn start_window_drag(
    window: tauri::Window,
    state: State<'_, AssistantState>,
) -> Result<(), String> {
    if lock_value(&state) {
        app_log::debug("窗口已锁定，本次拖动请求被忽略");
        return Ok(());
    }

    app_log::debug("开始拖动主窗口");
    window.start_dragging().map_err(|error| {
        app_log::warn(format!("窗口拖动启动失败：{error}"));
        error.to_string()
    })
}

#[tauri::command]
/// 从前端请求启动设置窗口拖动。
fn start_settings_window_drag(window: tauri::Window) -> Result<(), String> {
    app_log::debug("开始拖动设置窗口");
    window.start_dragging().map_err(|error| {
        app_log::warn(format!("设置窗口拖动启动失败：{error}"));
        error.to_string()
    })
}

#[tauri::command]
/// 关闭当前设置窗口。
fn close_settings_window(window: tauri::Window) -> Result<(), String> {
    if window.label() != SETTINGS_WINDOW_LABEL {
        return Err("只能关闭设置窗口".to_string());
    }

    app_log::debug("销毁设置窗口");
    window.destroy().map_err(|error| {
        app_log::warn(format!("设置窗口销毁失败：{error}"));
        error.to_string()
    })
}

#[tauri::command]
/// 从右下角启动主窗口尺寸调整。
fn start_window_resize(
    window: tauri::Window,
    state: State<'_, AssistantState>,
) -> Result<(), String> {
    if lock_value(&state) {
        app_log::debug("窗口已锁定，本次调整大小请求被忽略");
        return Ok(());
    }

    app_log::debug("开始从右下角调整主窗口大小");
    window
        .start_resize_dragging(tauri_runtime::ResizeDirection::SouthEast)
        .map_err(|error| {
            app_log::warn(format!("窗口调整大小启动失败：{error}"));
            error.to_string()
        })
}

#[tauri::command]
/// 写入来自前端页面的日志。
fn write_frontend_log(source: String, message: String) {
    app_log::info(format!(
        "前端日志 [{}] {}",
        clean_log_part(&source),
        clean_log_part(&message)
    ));
}

#[tauri::command]
/// 返回当前日志文件路径。
fn get_log_file_path() -> String {
    let path = app_log::log_path()
        .unwrap_or_else(|| default_app_data_dir().join("logs").join("assistant.log"));
    let text = path.to_string_lossy().to_string();
    app_log::debug(format!("前端读取日志文件路径：{text}"));
    text
}

#[tauri::command]
/// 使用资源管理器打开日志文件夹。
fn open_log_folder() -> Result<(), String> {
    let path = app_log::log_path()
        .unwrap_or_else(|| default_app_data_dir().join("logs").join("assistant.log"));
    let Some(folder) = path.parent() else {
        return Err("无法获取日志文件夹".to_string());
    };

    app_log::debug(format!("打开日志文件夹：{}", folder.display()));
    Command::new("explorer.exe")
        .arg(folder)
        .spawn()
        .map_err(|error| {
            app_log::warn(format!("打开日志文件夹失败：{error}"));
            error.to_string()
        })?;
    Ok(())
}

/// 清理前端传入的日志片段，避免控制字符污染日志。
fn clean_log_part(text: &str) -> String {
    let mut cleaned = text
        .chars()
        .filter(|character| !character.is_control())
        .take(240)
        .collect::<String>();

    if cleaned.trim().is_empty() {
        cleaned = "空消息".to_string();
    }

    cleaned
}

/// 创建标题栏右键菜单。
fn create_assistant_menu<R: tauri::Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
    let settings = MenuItemBuilder::with_id(TRAY_SETTINGS, "设置").build(manager)?;
    let lock = MenuItemBuilder::with_id(TRAY_LOCK, "锁定位置").build(manager)?;
    let unlock = MenuItemBuilder::with_id(TRAY_UNLOCK, "解锁位置").build(manager)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT, "退出程序").build(manager)?;

    MenuBuilder::new(manager)
        .items(&[&settings, &lock, &unlock])
        .separator()
        .item(&quit)
        .build()
}

/// 处理标题栏右键菜单事件。
fn handle_assistant_menu_event(app: &tauri::AppHandle, id: &str) {
    match id {
        TRAY_SETTINGS => open_settings_window(app),
        TRAY_LOCK => {
            app_log::info("菜单触发：锁定窗口位置");
            let state = app.state::<AssistantState>();
            let locked = set_lock_value(&state, true);
            persist_window_lock_state(&state, locked);
            apply_main_window_lock_state(app, true);
        }
        TRAY_UNLOCK => {
            app_log::info("菜单触发：解锁窗口位置");
            let state = app.state::<AssistantState>();
            let locked = set_lock_value(&state, false);
            persist_window_lock_state(&state, locked);
            apply_main_window_lock_state(app, false);
        }
        TRAY_QUIT => {
            app_log::info("标题栏菜单触发：退出程序");
            app.exit(0)
        }
        unknown => app_log::warn(format!("收到未知菜单事件：{unknown}")),
    }
}

#[tauri::command]
/// 在鼠标位置弹出标题栏右键菜单。
fn show_title_bar_menu(
    app: tauri::AppHandle,
    window: tauri::Window,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let menu = create_assistant_menu(&app).map_err(|error| error.to_string())?;
    app_log::debug(format!("标题栏右键打开菜单：x={x:.0}, y={y:.0}"));
    window
        .popup_menu_at(&menu, tauri::LogicalPosition::new(x, y))
        .map_err(|error| {
            app_log::warn(format!("标题栏右键菜单打开失败：{error}"));
            error.to_string()
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 启动 Tauri 应用并注册命令、菜单和窗口事件。
pub fn run() {
    let app_data_dir = app_data::prepare_app_data_dir();
    let log_path = app_log::init(app_data_dir.clone());
    app_log::info(format!("应用启动，日志文件：{}", log_path.display()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AssistantState::new(app_data_dir.clone()))
        .on_menu_event(|app, event| handle_assistant_menu_event(app, event.id().as_ref()))
        .setup(|app| {
            app_log::debug("Tauri setup 开始执行");
            restore_main_window_position(app.handle());
            let locked = lock_value(&app.state::<AssistantState>());
            apply_main_window_lock_state(app.handle(), locked);
            apply_main_window_desktop_layer_by_app(app.handle());
            app_log::debug("Tauri setup 执行完成");
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Moved(position) if window.label() == MAIN_WINDOW_LABEL => {
                let state = window.state::<AssistantState>();
                persist_main_window_position(window, state, position);
            }
            WindowEvent::Resized(_) if window.label() == MAIN_WINDOW_LABEL => {
                apply_square_tauri_window_region(window, "主窗口尺寸变化后");
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            get_recommendations,
            get_lock_state,
            set_lock_state,
            get_settings,
            save_settings,
            start_window_drag,
            start_settings_window_drag,
            close_settings_window,
            start_window_resize,
            show_title_bar_menu,
            write_frontend_log,
            get_log_file_path,
            open_log_folder
        ])
        .run(tauri::generate_context!())
        .expect("Tauri 应用运行失败");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WindowPosition;
    use std::fs;

    #[test]
    fn mock_recommendations_include_profit_and_sell_time() {
        let recommendations = mock_recommendations();

        assert_eq!(recommendations.len(), 4);
        assert!(recommendations.iter().all(|item| item.current_profit > 0));
        assert!(recommendations
            .iter()
            .all(|item| item.sell_time.contains("晚上")));
    }

    #[test]
    fn lock_state_update_does_not_touch_cached_recommendations() {
        let state = AssistantState::default();
        let cached = vec![CraftRecommendation {
            id: "cached-with-image".to_string(),
            station: "技术中心".to_string(),
            category: "工具".to_string(),
            item_name: "带图标物品".to_string(),
            image_url: Some("https://example.test/item.png".to_string()),
            current_profit: 100,
            ideal_price: 200,
            sell_time: "晚上11点".to_string(),
            accent: "#a84d50".to_string(),
        }];

        *state
            .cached_recommendations
            .lock()
            .expect("assistant cache lock poisoned") = Some(cached.clone());
        let locked = set_lock_value(&state, true);
        let stored = state
            .cached_recommendations
            .lock()
            .expect("assistant cache lock poisoned")
            .clone()
            .expect("cached recommendations remain available");

        assert!(locked);
        assert_eq!(stored[0].id, cached[0].id);
        assert_eq!(
            stored[0].image_url.as_deref(),
            Some("https://example.test/item.png")
        );
    }

    #[test]
    fn settings_defaults_to_five_minutes() {
        let settings = AssistantSettings::default();

        assert_eq!(settings.refresh_interval_minutes, 5);
        assert!(!settings.launch_on_startup);
        assert_eq!(settings.main_window_opacity_percent, 5);
    }

    #[test]
    fn settings_clamps_refresh_interval() {
        let low = AssistantSettings::new(0, false, 85);
        let high = AssistantSettings::new(99, true, 85);

        assert_eq!(low.refresh_interval_minutes, 1);
        assert_eq!(high.refresh_interval_minutes, 60);
        assert!(high.launch_on_startup);
    }

    #[test]
    fn settings_clamps_main_window_opacity() {
        let low = AssistantSettings::new(5, false, 1);
        let high = AssistantSettings::new(5, false, 101);

        assert_eq!(low.main_window_opacity_percent, 5);
        assert_eq!(high.main_window_opacity_percent, 100);
    }

    #[test]
    fn settings_file_supports_legacy_without_opacity() {
        let path = std::env::temp_dir().join(format!(
            "df-legacy-settings-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        fs::write(
            &path,
            r#"{"refreshIntervalMinutes":12,"launchOnStartup":true}"#,
        )
        .expect("legacy settings file writes");

        let loaded = read_settings_file(&path).expect("legacy settings file reads");
        fs::remove_file(path).expect("test settings file can be removed");

        assert_eq!(loaded.refresh_interval_minutes, 12);
        assert!(loaded.launch_on_startup);
        assert_eq!(loaded.main_window_opacity_percent, 5);
    }

    #[test]
    fn settings_file_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "df-settings-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        let settings = AssistantSettings::new(12, true, 72);

        write_settings_file(&path, &settings).expect("settings file writes");
        let loaded = read_settings_file(&path).expect("settings file reads");
        fs::remove_file(path).expect("test settings file can be removed");

        assert_eq!(loaded, settings);
    }

    #[test]
    fn window_state_file_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "df-window-state-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        let window_state = WindowStateFile {
            position: Some(WindowPosition { x: 320, y: 180 }),
            locked: true,
        };

        write_window_state_file(&path, &window_state).expect("window state file writes");
        let loaded = read_window_state_file(&path).expect("window state file reads");
        fs::remove_file(path).expect("test window position file can be removed");

        assert_eq!(loaded, window_state);
    }

    #[test]
    fn lock_state_persistence_preserves_position() {
        let path = std::env::temp_dir().join(format!(
            "df-window-lock-{}.json",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time before unix epoch")
                .as_nanos()
        ));
        let state = AssistantState {
            locked: Mutex::new(false),
            settings: Mutex::new(AssistantSettings::default()),
            settings_path: path.with_file_name("unused-settings.json"),
            window_state_path: path.clone(),
            cached_recommendations: Mutex::new(None),
            image_cache: image_cache::ImageCache::new(&std::env::temp_dir()),
            kkrb_client: kkrb::KkrbClient::default(),
        };
        let position = WindowPosition { x: 320, y: 180 };
        write_window_state_file(
            &path,
            &WindowStateFile {
                position: Some(position.clone()),
                locked: false,
            },
        )
        .expect("initial window state writes");

        persist_window_lock_state(&state, true);
        let loaded = read_window_state_file(&path).expect("window state reads");
        fs::remove_file(path).expect("test window state file can be removed");

        assert_eq!(loaded.position, Some(position));
        assert!(loaded.locked);
    }
}

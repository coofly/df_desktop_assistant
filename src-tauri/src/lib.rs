mod app_log;
mod kkrb;

use serde::{Deserialize, Serialize};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
    Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const TRAY_SETTINGS: &str = "open_settings";
const TRAY_LOCK: &str = "lock_window";
const TRAY_UNLOCK: &str = "unlock_window";
const TRAY_QUIT: &str = "quit_app";
const STARTUP_ENTRY_NAME: &str = "Delta Force Craft Assistant";
const STARTUP_REGISTRY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_REGISTRY_SUBKEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const SETTINGS_FILE_NAME: &str = "settings.json";
const WINDOW_STATE_FILE_NAME: &str = "window-state.json";
const SETTINGS_UPDATED_EVENT: &str = "settings-updated";
const MIN_REFRESH_INTERVAL_MINUTES: u16 = 1;
const DEFAULT_REFRESH_INTERVAL_MINUTES: u16 = 5;
const MAX_REFRESH_INTERVAL_MINUTES: u16 = 60;
const MIN_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
const DEFAULT_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
const MAX_MAIN_WINDOW_OPACITY_PERCENT: u8 = 100;
const APP_DATA_DIR_NAME: &str = "Delta Force Craft Assistant";
const SETTINGS_WINDOW_WIDTH: f64 = 360.0;
const SETTINGS_WINDOW_HEIGHT: f64 = 390.0;
const SETTINGS_WINDOW_MAIN_GAP: i32 = 3;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CraftRecommendation {
    id: String,
    station: String,
    category: String,
    item_name: String,
    image_url: Option<String>,
    current_profit: u32,
    ideal_price: u32,
    sell_time: String,
    accent: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssistantSnapshot {
    locked: bool,
    updated_at: String,
    recommendations: Vec<CraftRecommendation>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssistantSettings {
    refresh_interval_minutes: u16,
    launch_on_startup: bool,
    main_window_opacity_percent: u8,
}

impl AssistantSettings {
    fn new(
        refresh_interval_minutes: u16,
        launch_on_startup: bool,
        main_window_opacity_percent: u8,
    ) -> Self {
        Self {
            refresh_interval_minutes: refresh_interval_minutes
                .clamp(MIN_REFRESH_INTERVAL_MINUTES, MAX_REFRESH_INTERVAL_MINUTES),
            launch_on_startup,
            main_window_opacity_percent: main_window_opacity_percent.clamp(
                MIN_MAIN_WINDOW_OPACITY_PERCENT,
                MAX_MAIN_WINDOW_OPACITY_PERCENT,
            ),
        }
    }
}

impl Default for AssistantSettings {
    fn default() -> Self {
        Self::new(
            DEFAULT_REFRESH_INTERVAL_MINUTES,
            false,
            DEFAULT_MAIN_WINDOW_OPACITY_PERCENT,
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredAssistantSettings {
    refresh_interval_minutes: Option<u16>,
    launch_on_startup: Option<bool>,
    main_window_opacity_percent: Option<u8>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowPosition {
    x: i32,
    y: i32,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowStateFile {
    position: Option<WindowPosition>,
    locked: bool,
}

struct AssistantState {
    locked: Mutex<bool>,
    settings: Mutex<AssistantSettings>,
    settings_path: PathBuf,
    window_state_path: PathBuf,
    cached_recommendations: Mutex<Option<Vec<CraftRecommendation>>>,
    kkrb_client: kkrb::KkrbClient,
}

impl AssistantState {
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
            kkrb_client: kkrb::KkrbClient::default(),
        }
    }
}

impl Default for AssistantState {
    fn default() -> Self {
        Self::new(default_app_data_dir())
    }
}

fn default_app_data_dir() -> PathBuf {
    let base_dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(std::env::temp_dir);

    base_dir.join(APP_DATA_DIR_NAME)
}

fn enabled_text(enabled: bool) -> &'static str {
    if enabled {
        "开启"
    } else {
        "关闭"
    }
}

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

fn read_settings(path: &Path) -> Result<AssistantSettings, String> {
    let mut settings = read_settings_file(path)?;
    settings.launch_on_startup = match read_launch_on_startup() {
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

fn read_window_state_file(path: &Path) -> Result<WindowStateFile, String> {
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

fn write_window_state_file(path: &Path, window_state: &WindowStateFile) -> Result<(), String> {
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

fn quoted_startup_exe_path(exe_path: &Path) -> String {
    format!("\"{}\"", exe_path.to_string_lossy())
}

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

fn startup_registry_status_result(enable: bool, success: bool) -> Result<(), String> {
    if success {
        Ok(())
    } else if enable {
        Err("开机自启动开启失败，请确认当前账号有权限写入用户启动项".to_string())
    } else {
        Err("开机自启动关闭失败，请确认当前账号有权限删除用户启动项".to_string())
    }
}

fn startup_registry_value_to_enabled(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

#[cfg(windows)]
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
            return Err(registry_error_message("读取开机自启动状态失败", open_result));
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
            return Err(registry_error_message("读取开机自启动状态失败", size_result));
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
            return Err(registry_error_message("读取开机自启动状态失败", query_result));
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

#[cfg(not(windows))]
fn read_startup_registry_value() -> Result<Option<String>, String> {
    Ok(None)
}

fn read_launch_on_startup() -> Result<bool, String> {
    let value = read_startup_registry_value()?;
    Ok(startup_registry_value_to_enabled(value.as_deref()))
}

fn registry_error_message(context: &str, code: u32) -> String {
    format!(
        "{context}：{}（Windows 错误码 {code}）",
        io::Error::from_raw_os_error(code as i32)
    )
}

fn apply_launch_on_startup(enable: bool) -> Result<(), String> {
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

fn mock_recommendations() -> Vec<CraftRecommendation> {
    vec![
        CraftRecommendation {
            id: "tech-x25u".to_string(),
            station: "技术中心".to_string(),
            category: "工具".to_string(),
            item_name: "X25U斜侧战斗握把".to_string(),
            image_url: None,
            current_profit: 6027,
            ideal_price: 35699,
            sell_time: "晚上11点".to_string(),
            accent: "#6f5a91".to_string(),
        },
        CraftRecommendation {
            id: "workbench-45-70".to_string(),
            station: "工作台".to_string(),
            category: "弹药".to_string(),
            item_name: "45-70 Govt FTX".to_string(),
            image_url: None,
            current_profit: 36888,
            ideal_price: 5795,
            sell_time: "晚上11点".to_string(),
            accent: "#a77452".to_string(),
        },
        CraftRecommendation {
            id: "pharma-advanced-kit".to_string(),
            station: "制药台".to_string(),
            category: "医疗".to_string(),
            item_name: "高级护甲维修组合".to_string(),
            image_url: None,
            current_profit: 5227,
            ideal_price: 328570,
            sell_time: "晚上11点".to_string(),
            accent: "#a84d50".to_string(),
        },
        CraftRecommendation {
            id: "armor-helmet".to_string(),
            station: "防具台".to_string(),
            category: "防具".to_string(),
            item_name: "精英防弹背心".to_string(),
            image_url: None,
            current_profit: 23378,
            ideal_price: 336490,
            sell_time: "晚上11点".to_string(),
            accent: "#a97752".to_string(),
        },
    ]
}

fn snapshot_with_recommendations(
    locked: bool,
    updated_at: impl Into<String>,
    recommendations: Vec<CraftRecommendation>,
) -> AssistantSnapshot {
    AssistantSnapshot {
        locked,
        updated_at: updated_at.into(),
        recommendations,
    }
}

fn lock_value(state: &AssistantState) -> bool {
    *state.locked.lock().expect("assistant state lock poisoned")
}

fn set_lock_value(state: &AssistantState, locked: bool) -> bool {
    let mut lock = state.locked.lock().expect("assistant state lock poisoned");
    *lock = locked;
    *lock
}

fn persist_window_lock_state(state: &AssistantState, locked: bool) {
    let mut window_state = read_window_state_file(&state.window_state_path).unwrap_or_default();
    window_state.locked = locked;

    if let Err(error) = write_window_state_file(&state.window_state_path, &window_state) {
        app_log::warn(format!("保存窗口锁定状态失败：{error}"));
    }
}

#[tauri::command]
fn get_recommendations(state: State<'_, AssistantState>) -> AssistantSnapshot {
    app_log::debug("前端请求获取制作推荐");
    let locked = lock_value(&state);

    match state.kkrb_client.fetch_live_recommendations() {
        Ok(result) => {
            let recommendations = result.recommendations;
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
fn get_lock_state(state: State<'_, AssistantState>) -> bool {
    let locked = lock_value(&state);
    app_log::debug(format!("前端读取锁定状态：{}", enabled_text(locked)));
    locked
}

#[tauri::command]
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
fn get_settings(state: State<'_, AssistantState>) -> AssistantSettings {
    let mut settings = state
        .settings
        .lock()
        .expect("assistant settings lock poisoned")
        .clone();
    match read_launch_on_startup() {
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

    apply_launch_on_startup(settings.launch_on_startup)?;
    let settings = AssistantSettings::new(
        settings.refresh_interval_minutes,
        read_launch_on_startup()?,
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
fn start_settings_window_drag(window: tauri::Window) -> Result<(), String> {
    app_log::debug("开始拖动设置窗口");
    window.start_dragging().map_err(|error| {
        app_log::warn(format!("设置窗口拖动启动失败：{error}"));
        error.to_string()
    })
}

#[tauri::command]
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
fn write_frontend_log(source: String, message: String) {
    app_log::info(format!(
        "前端日志 [{}] {}",
        clean_log_part(&source),
        clean_log_part(&message)
    ));
}

#[tauri::command]
fn get_log_file_path() -> String {
    let path = app_log::log_path()
        .unwrap_or_else(|| default_app_data_dir().join("logs").join("assistant.log"));
    let text = path.to_string_lossy().to_string();
    app_log::debug(format!("前端读取日志文件路径：{text}"));
    text
}

#[tauri::command]
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

fn restore_main_window_position(app: &tauri::AppHandle) {
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

fn persist_main_window_position(
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

fn apply_main_window_desktop_layer(window: &tauri::WebviewWindow) {
    apply_main_window_desktop_widget_behavior(window);
    apply_square_window_region(window, "主窗口桌面挂件行为");

    if let Err(error) = window.set_always_on_top(false) {
        app_log::warn(format!("取消主窗口置顶失败：{error}"));
    }
}

fn apply_square_window_region(window: &tauri::WebviewWindow, context: &str) {
    let Ok(hwnd) = window.hwnd() else {
        app_log::warn(format!("{context}设置直角窗口区域失败：无法获取 HWND"));
        return;
    };

    apply_square_hwnd_region(hwnd.0 as _, context);
}

fn apply_square_tauri_window_region(window: &tauri::Window, context: &str) {
    let Ok(hwnd) = window.hwnd() else {
        app_log::warn(format!("{context}设置直角窗口区域失败：无法获取 HWND"));
        return;
    };

    apply_square_hwnd_region(hwnd.0 as _, context);
}

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
            app_log::warn(format!("{context}设置直角窗口区域失败：GetClientRect 调用失败"));
            return;
        }

        let region = CreateRectRgn(0, 0, rect.right - rect.left, rect.bottom - rect.top);
        if region.is_null() {
            app_log::warn(format!("{context}设置直角窗口区域失败：CreateRectRgn 调用失败"));
            return;
        }

        if SetWindowRgn(hwnd, region, 1) == 0 {
            app_log::warn(format!("{context}设置直角窗口区域失败：SetWindowRgn 调用失败"));
        }
    }
}

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
                x = (parent_width - 1120).max(0) / 2;
                y = (parent_height - 244).max(0) / 2;
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

    struct DesktopParentSearch {
        shell_view_parent: HWND,
        worker_after_shell_view: HWND,
        first_worker: HWND,
        progman: HWND,
    }

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

    unsafe fn has_direct_shell_view_child(hwnd: HWND) -> bool {
        !FindWindowExW(
            hwnd,
            null_mut_hwnd(),
            wide_null("SHELLDLL_DefView").as_ptr(),
            null(),
        )
        .is_null()
    }

    unsafe fn is_allowed_desktop_parent(hwnd: HWND) -> bool {
        if hwnd.is_null() {
            return false;
        }

        let class_name = window_class_name(hwnd);
        class_name.eq_ignore_ascii_case("Progman") || class_name.eq_ignore_ascii_case("WorkerW")
    }

    unsafe fn window_class_name(hwnd: HWND) -> String {
        let mut buffer = [0u16; 256];
        let len = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        wide_buffer_to_string(&buffer, len)
    }

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

fn wide_null(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn null_mut_hwnd() -> windows_sys::Win32::Foundation::HWND {
    std::ptr::null_mut()
}

fn apply_main_window_desktop_layer_by_app(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        apply_main_window_desktop_layer(&window);
    } else {
        app_log::warn("设置主窗口桌面挂件层失败：没有找到主窗口");
    }
}

fn apply_main_window_lock_state(app: &tauri::AppHandle, locked: bool) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        if let Err(error) = window.set_resizable(!locked) {
            app_log::warn(format!("更新主窗口缩放状态失败：{error}"));
        }

        apply_square_window_region(&window, "更新主窗口锁定状态后");
    } else {
        app_log::warn("更新主窗口缩放状态失败：没有找到主窗口");
    }
}

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

fn place_settings_window(app: &tauri::AppHandle, settings_window: &tauri::WebviewWindow) {
    let Some(main_window) = app.get_webview_window(MAIN_WINDOW_LABEL) else {
        app_log::warn("设置窗口定位失败：没有找到主窗口");
        return;
    };

    if let Err(error) = position_popup_window_next_to_main(&main_window, settings_window) {
        app_log::warn(format!("设置窗口定位失败：{error}"));
    }
}

fn open_settings_window(app: &tauri::AppHandle) {
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
pub fn run() {
    let app_data_dir = default_app_data_dir();
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

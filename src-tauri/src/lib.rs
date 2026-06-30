mod app_log;
mod kkrb;

use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};
use tauri::{
    menu::{Menu, MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

const MAIN_WINDOW_LABEL: &str = "main";
const SETTINGS_WINDOW_LABEL: &str = "settings";
const TRAY_TOGGLE: &str = "toggle_window";
const TRAY_SETTINGS: &str = "open_settings";
const TRAY_LOCK: &str = "lock_window";
const TRAY_UNLOCK: &str = "unlock_window";
const TRAY_QUIT: &str = "quit_app";
const STARTUP_ENTRY_NAME: &str = "Delta Force Craft Assistant";
const STARTUP_REGISTRY_PATH: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const SETTINGS_FILE_NAME: &str = "settings.json";
const SETTINGS_UPDATED_EVENT: &str = "settings-updated";
const MIN_REFRESH_INTERVAL_MINUTES: u16 = 1;
const DEFAULT_REFRESH_INTERVAL_MINUTES: u16 = 5;
const MAX_REFRESH_INTERVAL_MINUTES: u16 = 60;
const MIN_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
const DEFAULT_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
const MAX_MAIN_WINDOW_OPACITY_PERCENT: u8 = 100;
const APP_DATA_DIR_NAME: &str = "Delta Force Craft Assistant";

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

struct AssistantState {
    locked: Mutex<bool>,
    settings: Mutex<AssistantSettings>,
    settings_path: PathBuf,
    cached_recommendations: Mutex<Option<Vec<CraftRecommendation>>>,
    kkrb_client: kkrb::KkrbClient,
}

impl AssistantState {
    fn new(settings_path: PathBuf) -> Self {
        let settings = read_settings_file(&settings_path).unwrap_or_default();
        app_log::debug(format!(
            "助手状态初始化完成，设置文件路径：{}，刷新间隔：{} 分钟，开机自启动：{}，主面板透明度：{}%",
            settings_path.display(),
            settings.refresh_interval_minutes,
            enabled_text(settings.launch_on_startup),
            settings.main_window_opacity_percent
        ));

        Self {
            locked: Mutex::new(false),
            settings: Mutex::new(settings),
            settings_path,
            cached_recommendations: Mutex::new(None),
            kkrb_client: kkrb::KkrbClient::default(),
        }
    }
}

impl Default for AssistantState {
    fn default() -> Self {
        Self::new(default_settings_path())
    }
}

fn default_settings_path() -> PathBuf {
    default_app_data_dir().join(SETTINGS_FILE_NAME)
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
            exe_path.to_string_lossy().to_string(),
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
    if success || !enable {
        Ok(())
    } else {
        Err("开机自启动设置失败".to_string())
    }
}

#[cfg(windows)]
fn apply_launch_on_startup(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|error| error.to_string())?;
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

#[cfg(not(windows))]
fn apply_launch_on_startup(_enable: bool) -> Result<(), String> {
    app_log::debug("当前平台不是 Windows，跳过开机自启动设置");
    Ok(())
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
fn set_lock_state(state: State<'_, AssistantState>, locked: bool) -> bool {
    app_log::info(format!(
        "窗口位置已{}",
        if locked { "锁定" } else { "解锁" }
    ));
    set_lock_value(&state, locked)
}

#[tauri::command]
fn get_settings(state: State<'_, AssistantState>) -> AssistantSettings {
    let settings = state
        .settings
        .lock()
        .expect("assistant settings lock poisoned")
        .clone();
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

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        app_log::debug("显示主窗口并请求焦点");
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        app_log::warn("显示主窗口失败：没有找到主窗口");
    }
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        match window.is_visible() {
            Ok(true) => {
                app_log::info("托盘菜单触发：隐藏主窗口");
                let _ = window.hide();
            }
            _ => {
                app_log::info("托盘菜单触发：显示主窗口");
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    } else {
        app_log::warn("托盘菜单切换主窗口失败：没有找到主窗口");
    }
}

fn open_settings_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW_LABEL) {
        app_log::info("显示设置窗口");
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
    .inner_size(360.0, 340.0)
    .min_inner_size(320.0, 320.0)
    .resizable(false)
    .always_on_top(true)
    .build()
    .and_then(|window| {
        let _ = window.set_focus();
        Ok(window)
    });

    match result {
        Ok(_) => app_log::info("设置窗口创建成功"),
        Err(error) => app_log::error(format!("设置窗口创建失败：{error}")),
    }
}

fn create_assistant_menu<R: tauri::Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<Menu<R>> {
    let show_hide = MenuItemBuilder::with_id(TRAY_TOGGLE, "显示/隐藏窗口").build(manager)?;
    let settings = MenuItemBuilder::with_id(TRAY_SETTINGS, "设置").build(manager)?;
    let lock = MenuItemBuilder::with_id(TRAY_LOCK, "锁定位置").build(manager)?;
    let unlock = MenuItemBuilder::with_id(TRAY_UNLOCK, "解锁位置").build(manager)?;
    let quit = MenuItemBuilder::with_id(TRAY_QUIT, "退出程序").build(manager)?;

    MenuBuilder::new(manager)
        .items(&[&show_hide, &settings, &lock, &unlock])
        .separator()
        .item(&quit)
        .build()
}

fn handle_assistant_menu_event(app: &tauri::AppHandle, id: &str) {
    match id {
        TRAY_TOGGLE => toggle_main_window(app),
        TRAY_SETTINGS => open_settings_window(app),
        TRAY_LOCK => {
            app_log::info("菜单触发：锁定窗口位置");
            let state = app.state::<AssistantState>();
            set_lock_value(&state, true);
        }
        TRAY_UNLOCK => {
            app_log::info("菜单触发：解锁窗口位置");
            let state = app.state::<AssistantState>();
            set_lock_value(&state, false);
            show_main_window(app);
        }
        TRAY_QUIT => {
            app_log::info("菜单触发：退出程序");
            app.exit(0)
        }
        unknown => app_log::warn(format!("收到未知菜单事件：{unknown}")),
    }
}

#[tauri::command]
fn show_tray_menu_at_title_bar(
    app: tauri::AppHandle,
    window: tauri::Window,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let menu = create_assistant_menu(&app).map_err(|error| error.to_string())?;
    app_log::debug(format!("标题栏右键打开托盘菜单：x={x:.0}, y={y:.0}"));
    window
        .popup_menu_at(&menu, tauri::LogicalPosition::new(x, y))
        .map_err(|error| {
            app_log::warn(format!("标题栏右键菜单打开失败：{error}"));
            error.to_string()
        })
}

fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    app_log::debug("开始创建系统托盘图标和菜单");
    let menu = create_assistant_menu(app)?;
    let icon = app.default_window_icon().cloned();
    let mut tray_builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(true)
        .tooltip("三角洲行动特勤处制作助手")
        .on_menu_event(|app, event| handle_assistant_menu_event(app, event.id().as_ref()));

    if let Some(icon) = icon {
        tray_builder = tray_builder.icon(icon);
    }

    tray_builder.build(app)?;
    app_log::info("系统托盘图标创建成功");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_data_dir = default_app_data_dir();
    let log_path = app_log::init(app_data_dir.clone());
    app_log::info(format!("应用启动，日志文件：{}", log_path.display()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AssistantState::new(app_data_dir.join(SETTINGS_FILE_NAME)))
        .setup(|app| {
            app_log::debug("Tauri setup 开始执行");
            create_tray(app)?;
            app_log::debug("Tauri setup 执行完成");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                app_log::info(format!(
                    "窗口 {} 收到关闭请求，改为隐藏到托盘",
                    window.label()
                ));
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_recommendations,
            get_lock_state,
            set_lock_state,
            get_settings,
            save_settings,
            start_window_drag,
            show_tray_menu_at_title_bar,
            write_frontend_log,
            get_log_file_path
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
                r"C:\Program Files\Delta Force\assistant.exe",
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
    fn startup_disable_treats_missing_entry_as_ok() {
        assert!(startup_registry_status_result(false, false).is_ok());
        assert!(startup_registry_status_result(true, false).is_err());
    }
}

//! 定义前后端共享的数据结构和设置取值范围。

use serde::{Deserialize, Serialize};

pub(crate) const MIN_REFRESH_INTERVAL_MINUTES: u16 = 1;
pub(crate) const DEFAULT_REFRESH_INTERVAL_MINUTES: u16 = 5;
pub(crate) const MAX_REFRESH_INTERVAL_MINUTES: u16 = 60;
pub(crate) const MIN_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
pub(crate) const DEFAULT_MAIN_WINDOW_OPACITY_PERCENT: u8 = 5;
pub(crate) const MAX_MAIN_WINDOW_OPACITY_PERCENT: u8 = 100;

/// 描述单个制作台推荐产物。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CraftRecommendation {
    pub(crate) id: String,
    pub(crate) station: String,
    pub(crate) category: String,
    pub(crate) item_name: String,
    pub(crate) image_url: Option<String>,
    pub(crate) current_profit: u32,
    pub(crate) ideal_price: u32,
    pub(crate) sell_time: String,
    pub(crate) accent: String,
}

/// 返回给主面板的一次推荐快照。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantSnapshot {
    pub(crate) locked: bool,
    pub(crate) updated_at: String,
    pub(crate) recommendations: Vec<CraftRecommendation>,
}

/// 保存助手设置项并限制取值范围。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AssistantSettings {
    pub(crate) refresh_interval_minutes: u16,
    pub(crate) launch_on_startup: bool,
    pub(crate) main_window_opacity_percent: u8,
}

impl AssistantSettings {
    /// 创建经过边界裁剪的设置值。
    pub(crate) fn new(
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
    /// 提供助手设置的默认值。
    fn default() -> Self {
        Self::new(
            DEFAULT_REFRESH_INTERVAL_MINUTES,
            false,
            DEFAULT_MAIN_WINDOW_OPACITY_PERCENT,
        )
    }
}

/// 兼容读取历史设置文件中的可选字段。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredAssistantSettings {
    pub(crate) refresh_interval_minutes: Option<u16>,
    pub(crate) launch_on_startup: Option<bool>,
    pub(crate) main_window_opacity_percent: Option<u8>,
}

/// 保存窗口左上角屏幕坐标。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowPosition {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

/// 保存主窗口位置和锁定状态。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WindowStateFile {
    pub(crate) position: Option<WindowPosition>,
    pub(crate) locked: bool,
}

//! 封装 kkrb.net 实时推荐数据的会话获取、缓存和解析。

use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use crate::{app_log, models::CraftRecommendation};

const KKRBR_BASE_URL: &str = "https://www.kkrb.net";
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36"
);
const SESSION_CACHE_FILE: &str = "kkrb_cookies.txt";
const REQUEST_TIMEOUT_SECONDS: u64 = 20;

/// 持有 kkrb 会话缓存。
#[derive(Debug)]
pub struct KkrbClient {
    cookie_cache_path: PathBuf,
    session_initialized: Mutex<bool>,
}

/// 表示一次 kkrb 推荐数据获取结果。
#[derive(Debug)]
pub(crate) struct KkrbFetchResult {
    pub(crate) recommendations: Vec<CraftRecommendation>,
    pub(crate) session_source: KkrbSessionSource,
}

/// 标识本次 kkrb 会话来源。
#[derive(Clone, Copy, Debug)]
pub(crate) enum KkrbSessionSource {
    Memory,
    LocalFile,
    PhpSession,
}

impl KkrbSessionSource {
    /// 返回会话来源的中文日志标签。
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Memory => "内存 session",
            Self::LocalFile => "本地 kkrb_cookies.txt",
            Self::PhpSession => "从完整 Cookie 新建",
        }
    }
}

/// 对应 kkrb getMenu 接口响应。
#[derive(Debug, Deserialize)]
struct MenuResponse {
    code: i32,
    #[serde(default)]
    msg: String,
}

/// 对应 kkrb getOVData 概览接口响应。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewResponse {
    code: i32,
    msg: String,
    data: Option<OverviewData>,
}

/// 承载特勤处制作产物数据块。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewData {
    sph_data: SwatProductData,
}

/// 按制作台键名索引 kkrb 产物数据。
type SwatProductData = std::collections::HashMap<String, SwatProductItem>;

/// 表示 kkrb 单个制作台产物条目。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwatProductItem {
    item_name: String,
    item_grade: u8,
    pic: Option<String>,
    place: String,
    place_name: String,
    profit: f64,
    yesterday_highest_price: f64,
    yesterday_highest_time: String,
}

impl Default for KkrbClient {
    /// 使用默认会话缓存路径创建客户端。
    fn default() -> Self {
        Self::new(default_session_cache_path())
    }
}

impl KkrbClient {
    /// 使用指定会话缓存路径创建客户端。
    pub(crate) fn new(cookie_cache_path: PathBuf) -> Self {
        app_log::debug(format!(
            "初始化 kkrb 客户端，Cookie 缓存路径：{}",
            cookie_cache_path.display()
        ));

        Self {
            cookie_cache_path,
            session_initialized: Mutex::new(false),
        }
    }

    /// 获取实时推荐并自动回收可用会话。
    pub fn fetch_live_recommendations(&self) -> Result<KkrbFetchResult, String> {
        app_log::debug("开始获取 kkrb 实时制作推荐");

        if let Some(session_source) = self.cached_session_source() {
            app_log::debug(format!("准备使用 kkrb {} 请求数据", session_source.label()));
            if let Ok(recommendations) = self.fetch_with_session() {
                app_log::debug(format!(
                    "使用 kkrb {} 获取成功，推荐数量：{}",
                    session_source.label(),
                    recommendations.len()
                ));
                return Ok(KkrbFetchResult {
                    recommendations,
                    session_source,
                });
            }

            app_log::warn(format!(
                "kkrb {} 请求失败，将重新初始化会话",
                session_source.label()
            ));
            let _ = fs::remove_file(&self.cookie_cache_path);
            *self
                .session_initialized
                .lock()
                .expect("kkrb session lock poisoned") = false;
        }

        self.initialize_session()?;
        let recommendations = self.fetch_with_session()?;
        app_log::debug(format!(
            "使用 kkrb {} 获取成功，推荐数量：{}",
            KkrbSessionSource::PhpSession.label(),
            recommendations.len()
        ));
        Ok(KkrbFetchResult {
            recommendations,
            session_source: KkrbSessionSource::PhpSession,
        })
    }

    /// 尝试从内存或磁盘读取可复用会话。
    fn cached_session_source(&self) -> Option<KkrbSessionSource> {
        if *self
            .session_initialized
            .lock()
            .expect("kkrb session lock poisoned")
        {
            app_log::debug("命中内存中的 kkrb Cookie 会话");
            return Some(KkrbSessionSource::Memory);
        }

        let available = fs::metadata(&self.cookie_cache_path)
            .map(|metadata| metadata.is_file() && metadata.len() > 0)
            .unwrap_or(false);
        if !available {
            app_log::debug("没有可用的 kkrb Cookie 磁盘缓存");
            return None;
        }
        *self
            .session_initialized
            .lock()
            .expect("kkrb session lock poisoned") = true;
        app_log::debug("读取 kkrb Cookie 磁盘缓存成功");
        Some(KkrbSessionSource::LocalFile)
    }

    /// 请求首页和菜单接口初始化新会话。
    fn initialize_session(&self) -> Result<(), String> {
        app_log::debug("开始初始化 kkrb 会话：请求首页");
        if let Some(parent) = self.cookie_cache_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let _ = fs::remove_file(&self.cookie_cache_path);
        curl_initialize_home(
            &format!("{KKRBR_BASE_URL}/?viewpage=view%2Foverview"),
            &self.cookie_cache_path,
        )?;
        app_log::debug("kkrb 首页 Cookie 已写入缓存");

        app_log::debug("开始请求 kkrb getMenu 以初始化会话");
        let menu_text = curl_post(
            &format!("{KKRBR_BASE_URL}/getMenu"),
            &self.cookie_cache_path,
            &[("globalData", "false")],
        )?;
        let menu: MenuResponse =
            serde_json::from_str(&menu_text).map_err(|error| error.to_string())?;

        if menu.code != 1 {
            let message = if menu.msg.is_empty() {
                "kkrb getMenu failed".to_string()
            } else {
                menu.msg
            };
            app_log::warn(format!("kkrb getMenu 返回失败：{message}"));
            return Err(message);
        }

        app_log::debug("kkrb getMenu 会话初始化成功");

        *self
            .session_initialized
            .lock()
            .expect("kkrb session lock poisoned") = true;
        Ok(())
    }

    /// 使用已有会话请求并解析推荐数据。
    fn fetch_with_session(&self) -> Result<Vec<CraftRecommendation>, String> {
        app_log::debug("开始请求 kkrb 概览数据");
        let overview_text = curl_post(
            &format!("{KKRBR_BASE_URL}/getOVData"),
            &self.cookie_cache_path,
            &[("version", ""), ("globalData", "false")],
        )?;

        app_log::debug(format!(
            "kkrb 概览数据请求成功，响应长度：{} 字节",
            overview_text.len()
        ));
        normalize_swat_recommendations(&overview_text)
    }
}

/// 使用 Windows curl 请求首页并创建完整 Cookie jar。
fn curl_initialize_home(url: &str, cookie_cache_path: &Path) -> Result<(), String> {
    let mut arguments = curl_common_arguments();
    arguments.extend([
        "-c".to_string(),
        cookie_cache_path.to_string_lossy().into_owned(),
        "-o".to_string(),
        "NUL".to_string(),
    ]);
    arguments.push(url.to_string());
    run_curl(&arguments).map(|_| ())
}

/// 使用完整 Cookie jar 提交表单并返回响应正文。
fn curl_post(
    url: &str,
    cookie_cache_path: &Path,
    fields: &[(&str, &str)],
) -> Result<String, String> {
    let mut arguments = curl_common_arguments();
    arguments.extend([
        "-b".to_string(),
        cookie_cache_path.to_string_lossy().into_owned(),
        "-c".to_string(),
        cookie_cache_path.to_string_lossy().into_owned(),
        "-X".to_string(),
        "POST".to_string(),
    ]);
    for (name, value) in fields {
        arguments.push("--data-urlencode".to_string());
        arguments.push(format!("{name}={value}"));
    }
    arguments.push(url.to_string());
    run_curl(&arguments)
}

/// 构建 kkrb 请求共用的 curl 参数。
fn curl_common_arguments() -> Vec<String> {
    vec![
        "-sS".to_string(),
        "--fail-with-body".to_string(),
        "--max-time".to_string(),
        REQUEST_TIMEOUT_SECONDS.to_string(),
        "-A".to_string(),
        USER_AGENT.to_string(),
        "-H".to_string(),
        "Origin: https://www.kkrb.net".to_string(),
        "-H".to_string(),
        "Referer: https://www.kkrb.net/?viewpage=view%2Foverview".to_string(),
        "-H".to_string(),
        "X-Requested-With: XMLHttpRequest".to_string(),
    ]
}

/// 执行固定的 curl.exe 请求并检查退出状态和 UTF-8 输出。
fn run_curl(arguments: &[String]) -> Result<String, String> {
    let output = Command::new("curl.exe")
        .args(arguments)
        .output()
        .map_err(|error| format!("启动 curl.exe 失败：{error}"))?;
    if !output.status.success() {
        return Err(format!(
            "curl.exe 请求失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| format!("curl.exe 响应不是 UTF-8：{error}"))
}

/// 计算 kkrb 会话缓存文件路径。
fn default_session_cache_path() -> PathBuf {
    let base_dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);

    base_dir
        .join("com.coofly.df-desktop-assistant")
        .join(SESSION_CACHE_FILE)
}

/// 将 kkrb 概览 JSON 标准化为前端推荐列表。
pub fn normalize_swat_recommendations(json: &str) -> Result<Vec<CraftRecommendation>, String> {
    app_log::debug("开始解析 kkrb 特勤处制作推荐 JSON");
    let response: OverviewResponse =
        serde_json::from_str(json).map_err(|error| error.to_string())?;

    if response.code != 1 {
        app_log::warn(format!("kkrb 概览接口返回失败：{}", response.msg));
        return Err(response.msg);
    }

    let data = response
        .data
        .ok_or_else(|| "kkrb response missing data".to_string())?;
    let mut recommendations: Vec<_> = data
        .sph_data
        .into_iter()
        .map(|(_key, item)| CraftRecommendation {
            category: category_for_station(&item.place_name).to_string(),
            id: item.place,
            station: item.place_name,
            item_name: item.item_name,
            image_url: item.pic.filter(|pic| !pic.trim().is_empty()),
            current_profit: item.profit.round().max(0.0) as u32,
            ideal_price: item.yesterday_highest_price.round().max(0.0) as u32,
            sell_time: item.yesterday_highest_time,
            accent: accent_for_grade(item.item_grade).to_string(),
        })
        .collect();

    recommendations.sort_by_key(|item| station_rank(&item.station));
    app_log::debug(format!(
        "kkrb 特勤处制作推荐解析完成，推荐数量：{}",
        recommendations.len()
    ));

    Ok(recommendations)
}

/// 根据制作台名称推断产物分类。
fn category_for_station(station: &str) -> &'static str {
    match station {
        "技术中心" => "工具",
        "工作台" => "弹药",
        "制药台" => "医疗",
        "防具台" => "防具",
        _ => "产物",
    }
}

/// 根据物品品级映射前端强调色。
fn accent_for_grade(grade: u8) -> &'static str {
    match grade {
        6 => "#a84d50",
        5 => "#c48a3a",
        4 => "#8d63b3",
        3 => "#4f86c6",
        2 => "#6c9b62",
        _ => "#69736f",
    }
}

/// 返回制作台在主面板中的固定排序。
fn station_rank(station: &str) -> usize {
    match station {
        "技术中心" => 0,
        "工作台" => 1,
        "制药台" => 2,
        "防具台" => 3,
        _ => 99,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hourly_swat_products_from_overview_json() {
        let json = r##"
        {
          "code": 1,
          "msg": "获取成功",
          "data": {
            "sphData": {
              "armor": {
                "itemName": "精英防弹背心",
                "itemGrade": 4,
                "place": "armor",
                "placeName": "防具台",
                "profit": 32984.0,
                "yesterdayHighestPrice": 393770.0,
                "yesterdayHighestTime": "晚上11点"
              },
              "tech": {
                "itemName": "共振人体工程握把",
                "itemGrade": 4,
                "pic": "https://playerhub.df.qq.com/playerhub/60004/object/13140000034.png",
                "place": "tech",
                "placeName": "技术中心",
                "profit": 8011.2525,
                "yesterdayHighestPrice": 89409.0,
                "yesterdayHighestTime": "晚上11点"
              },
              "pharma": {
                "itemName": "精密护甲维修包",
                "itemGrade": 3,
                "place": "pharma",
                "placeName": "制药台",
                "profit": 8289.0,
                "yesterdayHighestPrice": 162870.0,
                "yesterdayHighestTime": "晚上11点"
              },
              "workbench": {
                "itemName": "7.62x54R BT",
                "itemGrade": 5,
                "place": "workbench",
                "placeName": "工作台",
                "profit": 38584.0,
                "yesterdayHighestPrice": 6934.0,
                "yesterdayHighestTime": "晚上11点"
              }
            }
          }
        }
        "##;

        let recommendations = normalize_swat_recommendations(json).expect("valid kkrb data");

        assert_eq!(recommendations.len(), 4);
        assert_eq!(recommendations[0].station, "技术中心");
        assert_eq!(recommendations[0].item_name, "共振人体工程握把");
        assert_eq!(
            recommendations[0].image_url.as_deref(),
            Some("https://playerhub.df.qq.com/playerhub/60004/object/13140000034.png")
        );
        assert_eq!(recommendations[0].current_profit, 8011);
        assert_eq!(recommendations[0].ideal_price, 89409);
        assert_eq!(recommendations[1].category, "弹药");
        assert_eq!(recommendations[1].accent, "#c48a3a");
    }

    #[test]
    fn maps_grade_six_to_red_accent() {
        assert_eq!(accent_for_grade(6), "#a84d50");
    }

    #[test]
    fn returns_api_message_when_overview_response_fails() {
        let error = normalize_swat_recommendations(r#"{"code":-101,"msg":"系统繁忙，请稍后再试"}"#)
            .expect_err("api error should be returned");

        assert_eq!(error, "系统繁忙，请稍后再试");
    }

    /// 验证当前菜单响应结构可以正常解析。
    #[test]
    fn parses_current_menu_response() {
        let menu: MenuResponse = serde_json::from_str(r#"{"code":1,"msg":""}"#).unwrap();

        assert_eq!(menu.code, 1);
    }

    #[test]
    #[ignore = "hits live kkrb.net"]
    fn fetches_live_swat_products() {
        let cache_path = std::env::temp_dir().join(format!(
            "df_desktop_assistant_kkrb_live_cookies_{}.txt",
            std::process::id()
        ));
        let recommendations = KkrbClient::new(cache_path.clone())
            .fetch_live_recommendations()
            .expect("live kkrb data");
        let _ = fs::remove_file(cache_path);

        assert_eq!(recommendations.recommendations.len(), 4);
        assert!(recommendations
            .recommendations
            .iter()
            .all(|item| item.current_profit > 0));
    }
}

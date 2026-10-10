//! 封装 kkrb.net 概览数据的会话获取、缓存和制作推荐解析。

use reqwest::header::{HeaderMap, HeaderValue, COOKIE, ORIGIN, REFERER, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

use crate::{app_log, models::CraftRecommendation};

const KKRBR_BASE_URL: &str = "https://www.kkrb.net";
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36"
);
const SESSION_CACHE_FILE: &str = "kkrb_session.json";
const REQUEST_TIMEOUT_SECONDS: u64 = 20;
const CONNECT_TIMEOUT_SECONDS: u64 = 8;

/// 持有 kkrb HTTP 客户端和会话缓存。
#[derive(Debug)]
pub struct KkrbClient {
    client: reqwest::blocking::Client,
    session_cache_path: PathBuf,
    session: Mutex<Option<KkrbSession>>,
    request_lock: Mutex<()>,
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
            Self::LocalFile => "本地 kkrb_session.json",
            Self::PhpSession => "从 PHPSESSID 和 csrf_token 新建",
        }
    }
}

/// 缓存当前 kkrb 接口需要的完整会话 Cookie。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct KkrbSession {
    php_session_id: String,
    csrf_token: String,
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
    pub(crate) fn new(session_cache_path: PathBuf) -> Self {
        app_log::debug(format!(
            "初始化 kkrb 客户端，会话缓存路径：{}",
            session_cache_path.display()
        ));

        Self {
            client: build_client().expect("failed to build kkrb client"),
            session_cache_path,
            session: Mutex::new(None),
            request_lock: Mutex::new(()),
        }
    }

    /// 获取实时推荐并自动回收可用会话。
    pub fn fetch_live_recommendations(&self) -> Result<KkrbFetchResult, String> {
        app_log::debug("开始获取 kkrb 实时制作推荐");
        let (recommendations, session_source) =
            self.fetch_overview(normalize_swat_recommendations)?;
        Ok(KkrbFetchResult {
            recommendations,
            session_source,
        })
    }

    /// 串行复用概览会话，并使用指定解析器读取独立数据块。
    pub(crate) fn fetch_overview<T>(
        &self,
        normalize: fn(&str) -> Result<T, String>,
    ) -> Result<(T, KkrbSessionSource), String> {
        let _request = self
            .request_lock
            .lock()
            .expect("kkrb request lock poisoned");
        if let Some((session, session_source)) = self.cached_session() {
            app_log::debug(format!("准备使用 kkrb {} 请求数据", session_source.label()));
            if let Ok(data) = self
                .fetch_with_session(&session)
                .and_then(|text| normalize(&text))
            {
                return Ok((data, session_source));
            }

            app_log::warn(format!(
                "kkrb {} 请求失败，将重新初始化会话",
                session_source.label()
            ));
            let _ = fs::remove_file(&self.session_cache_path);
            *self.session.lock().expect("kkrb session lock poisoned") = None;
        }

        let session = self.initialize_session()?;
        let data = normalize(&self.fetch_with_session(&session)?)?;
        Ok((data, KkrbSessionSource::PhpSession))
    }

    /// 尝试从内存或磁盘读取可复用会话。
    fn cached_session(&self) -> Option<(KkrbSession, KkrbSessionSource)> {
        if let Some(session) = self
            .session
            .lock()
            .expect("kkrb session lock poisoned")
            .clone()
        {
            app_log::debug("命中内存中的 kkrb 会话缓存");
            return Some((session, KkrbSessionSource::Memory));
        }

        let session = match read_session_cache(&self.session_cache_path) {
            Ok(session) => session,
            Err(error) => {
                app_log::debug(format!("没有可用的 kkrb 磁盘会话缓存：{error}"));
                return None;
            }
        };
        *self.session.lock().expect("kkrb session lock poisoned") = Some(session.clone());
        app_log::debug("读取 kkrb 磁盘会话缓存成功");
        Some((session, KkrbSessionSource::LocalFile))
    }

    /// 请求首页和菜单接口初始化新会话。
    fn initialize_session(&self) -> Result<KkrbSession, String> {
        app_log::debug("开始初始化 kkrb 会话：请求首页");
        let home_response = self
            .client
            .get(format!("{KKRBR_BASE_URL}/?viewpage=view%2Foverview"))
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let php_session_id = cookie_value_from_headers(home_response.headers(), "PHPSESSID")
            .ok_or_else(|| "kkrb homepage response missing PHPSESSID".to_string())?;
        app_log::debug("kkrb 首页返回 PHPSESSID");

        app_log::debug("开始请求 kkrb getMenu 以初始化会话");
        let menu_response = self
            .client
            .post(format!("{KKRBR_BASE_URL}/getMenu"))
            .header(COOKIE, php_session_cookie_header(&php_session_id))
            .form(&[("globalData", "false")])
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?;
        let csrf_token = cookie_value_from_headers(menu_response.headers(), "csrf_token")
            .ok_or_else(|| "kkrb getMenu response missing csrf_token".to_string())?;
        let menu: MenuResponse = menu_response.json().map_err(|error| error.to_string())?;

        if menu.code != 1 {
            let message = if menu.msg.is_empty() {
                "kkrb getMenu failed".to_string()
            } else {
                menu.msg
            };
            app_log::warn(format!("kkrb getMenu 返回失败：{message}"));
            return Err(message);
        }

        let session = KkrbSession {
            php_session_id,
            csrf_token,
        };
        write_session_cache(&self.session_cache_path, &session)?;
        *self.session.lock().expect("kkrb session lock poisoned") = Some(session.clone());
        app_log::debug("kkrb 完整会话 Cookie 已写入缓存");
        Ok(session)
    }

    /// 使用已有会话请求概览 JSON，供各数据块独立解析。
    fn fetch_with_session(&self, session: &KkrbSession) -> Result<String, String> {
        app_log::debug("开始请求 kkrb 概览数据");
        let overview_text = self
            .client
            .post(format!("{KKRBR_BASE_URL}/getOVData"))
            .header(COOKIE, session_cookie_header(session))
            .form(&[("version", ""), ("globalData", "false")])
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .text()
            .map_err(|error| error.to_string())?;

        app_log::debug(format!(
            "kkrb 概览数据请求成功，响应长度：{} 字节",
            overview_text.len()
        ));
        Ok(overview_text)
    }
}

/// 构建带浏览器请求头和超时设置的 HTTP 客户端。
fn build_client() -> Result<reqwest::blocking::Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-requested-with",
        HeaderValue::from_static("XMLHttpRequest"),
    );
    headers.insert(ORIGIN, HeaderValue::from_static(KKRBR_BASE_URL));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://www.kkrb.net/?viewpage=view%2Foverview"),
    );
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .user_agent(USER_AGENT)
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECONDS))
        .timeout(Duration::from_secs(REQUEST_TIMEOUT_SECONDS))
        .build()
        .map_err(|error| error.to_string())
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

/// 从响应头中提取指定 Set-Cookie 的值。
fn cookie_value_from_headers(headers: &HeaderMap, cookie_name: &str) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| cookie_value_from_set_cookie(value, cookie_name))
}

/// 从单条 Set-Cookie 中提取指定 Cookie 的值。
fn cookie_value_from_set_cookie(set_cookie: &str, cookie_name: &str) -> Option<String> {
    let (name, value) = set_cookie.split(';').next()?.trim().split_once('=')?;
    (name == cookie_name && !value.is_empty()).then(|| value.to_string())
}

/// 生成初始化菜单接口所需的 PHP session Cookie。
fn php_session_cookie_header(php_session_id: &str) -> String {
    format!("PHPSESSID={php_session_id}")
}

/// 生成概览接口所需的完整 Cookie 请求头。
fn session_cookie_header(session: &KkrbSession) -> String {
    format!(
        "PHPSESSID={}; csrf_token={}",
        session.php_session_id, session.csrf_token
    )
}

/// 从磁盘读取并校验完整会话 Cookie。
fn read_session_cache(path: &Path) -> Result<KkrbSession, String> {
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let session: KkrbSession =
        serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    if session.php_session_id.trim().is_empty() || session.csrf_token.trim().is_empty() {
        return Err("kkrb session cache is incomplete".to_string());
    }
    Ok(session)
}

/// 将完整会话 Cookie 写入统一应用数据目录。
fn write_session_cache(path: &Path, session: &KkrbSession) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let contents = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
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

    /// 验证响应头能分别提取 PHPSESSID 和 csrf_token。
    #[test]
    fn extracts_complete_session_cookies() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("PHPSESSID=php-123; path=/; HttpOnly"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("csrf_token=csrf-456; path=/"),
        );

        assert_eq!(
            cookie_value_from_headers(&headers, "PHPSESSID").as_deref(),
            Some("php-123")
        );
        assert_eq!(
            cookie_value_from_headers(&headers, "csrf_token").as_deref(),
            Some("csrf-456")
        );
    }

    /// 验证完整会话 Cookie 可以写入并恢复。
    #[test]
    fn persists_complete_session_cookies() {
        let path = std::env::temp_dir().join(format!(
            "df_desktop_assistant_kkrb_session_{}.json",
            std::process::id()
        ));
        let session = KkrbSession {
            php_session_id: "php-123".to_string(),
            csrf_token: "csrf-456".to_string(),
        };

        write_session_cache(&path, &session).unwrap();
        let restored = read_session_cache(&path).unwrap();
        let _ = fs::remove_file(path);

        assert_eq!(restored, session);
        assert_eq!(
            session_cookie_header(&restored),
            "PHPSESSID=php-123; csrf_token=csrf-456"
        );
    }

    #[test]
    #[ignore = "hits live kkrb.net"]
    fn fetches_live_swat_products() {
        let cache_path = std::env::temp_dir().join(format!(
            "df_desktop_assistant_kkrb_live_session_{}.json",
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

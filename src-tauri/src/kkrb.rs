use reqwest::header::{HeaderMap, HeaderValue, COOKIE, ORIGIN, REFERER, SET_COOKIE};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{app_log, CraftRecommendation};

const KKRBR_BASE_URL: &str = "https://www.kkrb.net";
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) ",
    "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126 Safari/537.36"
);
const SESSION_CACHE_FILE: &str = "kkrb_session.json";

#[derive(Debug)]
pub struct KkrbClient {
    client: reqwest::blocking::Client,
    session_cache_path: PathBuf,
    session: Mutex<Option<KkrbSession>>,
}

#[derive(Debug)]
pub(crate) struct KkrbFetchResult {
    pub(crate) recommendations: Vec<CraftRecommendation>,
    pub(crate) session_source: KkrbSessionSource,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum KkrbSessionSource {
    Memory,
    LocalFile,
    PhpSession,
}

impl KkrbSessionSource {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Memory => "内存 session",
            Self::LocalFile => "本地 kkrb_session.json",
            Self::PhpSession => "从 PHPSESSID 新建",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct KkrbSession {
    php_session_id: String,
    built_ver: String,
    saved_at_unix_seconds: u64,
}

#[derive(Debug, Deserialize)]
struct MenuResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    built_ver: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewResponse {
    code: i32,
    msg: String,
    data: Option<OverviewData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OverviewData {
    sph_data: SwatProductData,
}

type SwatProductData = std::collections::HashMap<String, SwatProductItem>;

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
    fn default() -> Self {
        Self::new(default_session_cache_path())
    }
}

impl KkrbClient {
    fn new(session_cache_path: PathBuf) -> Self {
        app_log::debug(format!(
            "初始化 kkrb 客户端，会话缓存路径：{}",
            session_cache_path.display()
        ));

        Self {
            client: build_client().expect("failed to build kkrb client"),
            session_cache_path,
            session: Mutex::new(None),
        }
    }

    pub fn fetch_live_recommendations(&self) -> Result<KkrbFetchResult, String> {
        app_log::debug("开始获取 kkrb 实时制作推荐");

        if let Some((session, session_source)) = self.cached_session() {
            app_log::debug(format!(
                "准备使用 kkrb {} 请求数据，版本号：{}",
                session_source.label(),
                session.built_ver,
            ));
            if let Ok(recommendations) = self.fetch_with_session(&session) {
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
        }

        let session = self.initialize_session()?;
        let recommendations = self.fetch_with_session(&session)?;
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

    fn cached_session(&self) -> Option<(KkrbSession, KkrbSessionSource)> {
        if let Some(session) = self
            .session
            .lock()
            .expect("kkrb session lock poisoned")
            .clone()
        {
            app_log::debug(format!(
                "命中内存中的 kkrb 会话缓存，保存时间戳：{}",
                session.saved_at_unix_seconds
            ));
            return Some((session, KkrbSessionSource::Memory));
        }

        let session = match read_session_cache(&self.session_cache_path) {
            Ok(session) => session,
            Err(error) => {
                app_log::debug(format!("没有可用的 kkrb 磁盘会话缓存：{error}"));
                return None;
            }
        };
        app_log::debug(format!(
            "读取 kkrb 磁盘会话缓存成功，版本号：{}",
            session.built_ver
        ));
        *self.session.lock().expect("kkrb session lock poisoned") = Some(session.clone());
        Some((session, KkrbSessionSource::LocalFile))
    }

    fn initialize_session(&self) -> Result<KkrbSession, String> {
        app_log::debug("开始初始化 kkrb 会话：请求首页");
        let home_response = self
            .client
            .get(format!("{KKRBR_BASE_URL}/?viewpage=view%2Foverview"))
            .send()
            .map_err(|error| error.to_string())?;

        let php_session_id = php_session_id_from_headers(home_response.headers())
            .ok_or_else(|| "kkrb homepage response missing PHPSESSID".to_string())?;
        app_log::debug("kkrb 首页返回 PHPSESSID，会话 Cookie 已获取");

        home_response
            .error_for_status()
            .map_err(|error| error.to_string())?;

        app_log::debug("开始请求 kkrb getMenu 以获取数据版本号");
        let menu: MenuResponse = self
            .client
            .post(format!("{KKRBR_BASE_URL}/getMenu"))
            .header(COOKIE, session_cookie_header(&php_session_id))
            .form(&[("globalData", "false")])
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;

        if menu.code != 1 {
            let message = if menu.msg.is_empty() {
                "kkrb getMenu failed".to_string()
            } else {
                menu.msg
            };
            app_log::warn(format!("kkrb getMenu 返回失败：{message}"));
            return Err(message);
        }

        let built_ver = menu
            .built_ver
            .filter(|version| !version.is_empty())
            .ok_or_else(|| "kkrb getMenu response missing built_ver".to_string())?;
        app_log::debug(format!("kkrb getMenu 成功，数据版本号：{built_ver}"));

        let session = KkrbSession {
            php_session_id,
            built_ver,
            saved_at_unix_seconds: unix_timestamp_now(),
        };

        *self.session.lock().expect("kkrb session lock poisoned") = Some(session.clone());
        match write_session_cache(&self.session_cache_path, &session) {
            Ok(()) => app_log::debug("kkrb 会话缓存写入成功"),
            Err(error) => app_log::warn(format!("kkrb 会话缓存写入失败：{error}")),
        }

        Ok(session)
    }

    fn fetch_with_session(
        &self,
        session: &KkrbSession,
    ) -> Result<Vec<CraftRecommendation>, String> {
        app_log::debug(format!(
            "开始校验 kkrb UA 状态，版本号：{}",
            session.built_ver
        ));
        let ua_status: serde_json::Value = self
            .client
            .post(format!("{KKRBR_BASE_URL}/checkUAStatus"))
            .header(COOKIE, session_cookie_header(&session.php_session_id))
            .form(&[("version", session.built_ver.as_str())])
            .send()
            .map_err(|error| error.to_string())?
            .error_for_status()
            .map_err(|error| error.to_string())?
            .json()
            .map_err(|error| error.to_string())?;

        if ua_status.get("code").and_then(|code| code.as_i64()) != Some(1) {
            let message = ua_status
                .get("msg")
                .and_then(|msg| msg.as_str())
                .unwrap_or("kkrb checkUAStatus failed")
                .to_string();
            app_log::warn(format!("kkrb UA 状态校验失败：{message}"));
            return Err(message);
        }

        app_log::debug("kkrb UA 状态校验通过，开始请求概览数据");
        let overview_text = self
            .client
            .post(format!("{KKRBR_BASE_URL}/getOVData"))
            .header(COOKIE, session_cookie_header(&session.php_session_id))
            .form(&[
                ("version", session.built_ver.as_str()),
                ("globalData", "false"),
            ])
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
        normalize_swat_recommendations(&overview_text)
    }
}

fn build_client() -> Result<reqwest::blocking::Client, String> {
    app_log::debug("开始构建 kkrb HTTP 客户端");
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
        .cookie_store(true)
        .default_headers(headers)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|error| error.to_string())
}

fn default_session_cache_path() -> PathBuf {
    let base_dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .unwrap_or_else(std::env::temp_dir);

    base_dir
        .join("com.coofly.df_desktop_assistant")
        .join(SESSION_CACHE_FILE)
}

fn php_session_id_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(php_session_id_from_set_cookie)
}

fn php_session_id_from_set_cookie(set_cookie: &str) -> Option<String> {
    let cookie = set_cookie.split(';').next()?.trim();
    cookie
        .strip_prefix("PHPSESSID=")
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn session_cookie_header(php_session_id: &str) -> String {
    format!("PHPSESSID={php_session_id}")
}

fn read_session_cache(path: &Path) -> Result<KkrbSession, String> {
    app_log::debug(format!("准备读取 kkrb 会话缓存：{}", path.display()));
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let session: KkrbSession =
        serde_json::from_str(&contents).map_err(|error| error.to_string())?;

    if session.php_session_id.trim().is_empty() || session.built_ver.trim().is_empty() {
        return Err("kkrb session cache is incomplete".to_string());
    }

    Ok(session)
}

fn write_session_cache(path: &Path, session: &KkrbSession) -> Result<(), String> {
    app_log::debug(format!("准备写入 kkrb 会话缓存：{}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let contents = serde_json::to_string_pretty(session).map_err(|error| error.to_string())?;
    fs::write(path, contents).map_err(|error| error.to_string())
}

fn unix_timestamp_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

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

fn category_for_station(station: &str) -> &'static str {
    match station {
        "技术中心" => "工具",
        "工作台" => "弹药",
        "制药台" => "医疗",
        "防具台" => "防具",
        _ => "产物",
    }
}

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

    #[test]
    fn extracts_php_session_id_from_set_cookie() {
        let session_id = php_session_id_from_set_cookie(
            "PHPSESSID=abc123; expires=Sun, 28-Jun-2026 18:29:13 GMT; path=/; HttpOnly",
        );

        assert_eq!(session_id.as_deref(), Some("abc123"));
        assert_eq!(session_cookie_header("abc123"), "PHPSESSID=abc123");
    }

    #[test]
    fn writes_and_reads_session_cache() {
        let path = std::env::temp_dir().join(format!(
            "df_desktop_assistant_kkrb_session_{}.json",
            unix_timestamp_now()
        ));
        let session = KkrbSession {
            php_session_id: "cached-session".to_string(),
            built_ver: "cached-built-ver".to_string(),
            saved_at_unix_seconds: 123,
        };

        write_session_cache(&path, &session).expect("session cache write");
        let restored = read_session_cache(&path).expect("session cache read");
        let _ = fs::remove_file(&path);

        assert_eq!(restored, session);
    }

    #[test]
    #[ignore = "hits live kkrb.net"]
    fn fetches_live_swat_products() {
        let cache_path = std::env::temp_dir().join(format!(
            "df_desktop_assistant_kkrb_live_session_{}.json",
            unix_timestamp_now()
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

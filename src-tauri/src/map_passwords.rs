//! 解析每日地图密码，缓存最后成功的快照并提供独立 Tauri 命令。

use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::{app_log, AssistantState};

const MAPS: [(&str, &str); 6] = [
    ("db", "零号大坝"),
    ("cgxg", "长弓溪谷"),
    ("bks", "巴克什"),
    ("htjd", "航天基地"),
    ("cxjy", "潮汐监狱"),
    ("az3", "AZ3"),
];

/// 描述地图名及保留前导零的密码。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MapPassword {
    id: String,
    map_name: String,
    password: String,
}

/// 返回密码列表及最后成功获取时间的毫秒时间戳。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MapPasswordSnapshot {
    passwords: Vec<MapPassword>,
    refreshed_at: Option<u64>,
}

/// 保存当前进程中最后成功的地图密码快照。
#[derive(Default)]
pub(crate) struct MapPasswordCache {
    snapshot: Mutex<Option<MapPasswordSnapshot>>,
}

/// 只读取概览接口中的地图密码响应。
#[derive(Deserialize)]
struct PasswordResponse {
    code: i32,
    #[serde(default)]
    msg: String,
    data: Option<PasswordData>,
}

/// 按源站键名索引每日密码，不依赖制作推荐字段。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordData {
    bd_data: HashMap<String, PasswordEntry>,
}

/// 读取源站的字符串密码并忽略额外字段。
#[derive(Deserialize)]
struct PasswordEntry {
    password: String,
}

impl MapPasswordCache {
    /// 获取新快照，失败时静默保留缓存或六张空密码卡片。
    fn refresh(
        &self,
        fetch: impl FnOnce() -> Result<Vec<MapPassword>, String>,
    ) -> MapPasswordSnapshot {
        let mut cached = self
            .snapshot
            .lock()
            .expect("map password cache lock poisoned");
        match fetch() {
            Ok(passwords) => {
                let snapshot = MapPasswordSnapshot {
                    passwords,
                    refreshed_at: Some(
                        SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64,
                    ),
                };
                *cached = Some(snapshot.clone());
                snapshot
            }
            Err(error) => {
                app_log::warn(format!("地图密码获取失败，保留已有数据：{error}"));
                cached.clone().unwrap_or_else(empty_snapshot)
            }
        }
    }
}

/// 按固定顺序创建六张空密码卡片，避免使用模拟密码。
fn empty_snapshot() -> MapPasswordSnapshot {
    MapPasswordSnapshot {
        passwords: MAPS
            .iter()
            .map(|(id, name)| MapPassword {
                id: (*id).to_string(),
                map_name: (*name).to_string(),
                password: "----".to_string(),
            })
            .collect(),
        refreshed_at: None,
    }
}

/// 校验六项完整密码并排除联动房，保持源站字符串不变。
fn normalize_map_passwords(json: &str) -> Result<Vec<MapPassword>, String> {
    let response: PasswordResponse =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    if response.code != 1 {
        return Err(response.msg);
    }
    let data = response.data.ok_or("kkrb response missing password data")?;
    MAPS.iter()
        .map(|(id, name)| {
            let entry = data
                .bd_data
                .get(*id)
                .ok_or_else(|| format!("missing map password: {id}"))?;
            if entry.password.is_empty()
                || !entry.password.bytes().all(|digit| digit.is_ascii_digit())
            {
                return Err(format!("invalid map password: {id}"));
            }
            Ok(MapPassword {
                id: (*id).to_string(),
                map_name: (*name).to_string(),
                password: entry.password.clone(),
            })
        })
        .collect()
}

/// 在后台线程获取每日地图密码，避免网络请求阻塞窗口交互。
#[tauri::command]
pub(crate) async fn get_map_passwords(
    app: tauri::AppHandle,
) -> Result<MapPasswordSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AssistantState>();
        state.map_password_cache.refresh(|| {
            let (passwords, source) = state.kkrb_client.fetch_overview(normalize_map_passwords)?;
            app_log::info(format!(
                "地图密码获取成功，会话：{}，数量：{}",
                source.label(),
                passwords.len()
            ));
            Ok(passwords)
        })
    })
    .await
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成带联动房和额外源站字段的真实结构测试数据。
    fn sample_json() -> String {
        serde_json::json!({
            "code": 1, "msg": "获取成功",
            "data": {"bdData": {
                "db": {"password": "0629", "updated": "20261010000001"},
                "cgxg": {"password": "0685"},
                "bks": {"password": "0914"},
                "htjd": {"password": "0177"},
                "cxjy": {"password": "0975"},
                "az3": {"password": "0530"},
                "az3r6": {"password": "266"}
            }}
        })
        .to_string()
    }

    /// 验证前导零、固定地图顺序和联动房过滤。
    #[test]
    fn keeps_password_strings_and_six_map_order() {
        let passwords = normalize_map_passwords(&sample_json()).unwrap();
        assert_eq!(passwords.len(), 6);
        for (password, (id, name)) in passwords.iter().zip(MAPS) {
            assert_eq!(password.id, id);
            assert_eq!(password.map_name, name);
        }
        assert_eq!(passwords[0].password, "0629");
        assert_eq!(passwords[5].password, "0530");
    }

    /// 验证错误响应、缺失地图和非字符串密码不会覆盖缓存。
    #[test]
    fn rejects_incomplete_or_invalid_responses() {
        for json in [
            r#"{"code":-1,"msg":"系统繁忙"}"#.to_string(),
            r#"{"code":1,"data":{}}"#.to_string(),
            sample_json().replace(r#""password":"0629""#, r#""password":629"#),
            sample_json().replace(r#""password":"0629""#, r#""password":""#),
            sample_json().replace(r#""password":"0629""#, r#""password":"----""#),
            sample_json().replace(r#""db":"#, r#""unknown":"#),
        ] {
            assert!(normalize_map_passwords(&json).is_err(), "{json}");
        }
    }

    /// 验证首次失败仅返回占位，随后失败保持数据和成功时间。
    #[test]
    fn retains_last_success_on_failure() {
        let cache = MapPasswordCache::default();
        let empty = cache.refresh(|| Err("offline".to_string()));
        assert!(empty.refreshed_at.is_none());
        assert_eq!(empty.passwords.len(), 6);
        assert!(empty.passwords.iter().all(|item| item.password == "----"));

        let success = cache.refresh(|| normalize_map_passwords(&sample_json()));
        assert!(success.refreshed_at.is_some());
        assert_eq!(cache.refresh(|| Err("offline".to_string())), success);
    }

    /// 验证缓存不按日期或密码变化隐藏源站数据。
    #[test]
    fn accepts_unchanged_source_passwords() {
        let cache = MapPasswordCache::default();
        let old_date = sample_json().replace("20261010000001", "20260822042438");
        let first = cache.refresh(|| normalize_map_passwords(&old_date));
        let second = cache.refresh(|| normalize_map_passwords(&old_date));
        assert_eq!(first.passwords, second.passwords);
        assert!(second.refreshed_at >= first.refreshed_at);
    }

    /// 按需验证 Rust 会话流程能获取源站的六项实际密码。
    #[test]
    #[ignore = "hits live kkrb.net"]
    fn fetches_live_map_passwords() {
        let path = std::env::temp_dir().join(format!(
            "df_map_password_live_session_{}.json",
            std::process::id()
        ));
        let result =
            crate::kkrb::KkrbClient::new(path.clone()).fetch_overview(normalize_map_passwords);
        let _ = std::fs::remove_file(path);
        let (passwords, _) = result.expect("live map passwords");
        assert_eq!(passwords.len(), 6);
        assert!(passwords.iter().all(|item| item.password != "----"));
    }
}

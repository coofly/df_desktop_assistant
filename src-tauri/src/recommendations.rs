//! 提供制作推荐命令、快照组装和无网络时的兜底数据。

use crate::models::{AssistantSnapshot, CraftRecommendation};
use crate::{app_log, lock_value, AssistantState};
use tauri::Manager;

/// 在后台线程获取制作推荐，保持快照结构和失败兜底逻辑。
#[tauri::command]
pub(crate) async fn get_recommendations(
    app: tauri::AppHandle,
) -> Result<AssistantSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AssistantState>();
        fetch_recommendations(&state)
    })
    .await
    .map_err(|error| error.to_string())
}

/// 获取制作推荐快照并在失败时回退到缓存或 mock。
fn fetch_recommendations(state: &AssistantState) -> AssistantSnapshot {
    app_log::debug("前端请求获取制作推荐");
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

            snapshot_with_recommendations(lock_value(state), "kkrb 实时数据", recommendations)
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
                    lock_value(state),
                    format!("kkrb 获取失败，使用缓存：{error}"),
                    recommendations,
                )
            } else {
                app_log::info("没有可用缓存，使用本地 mock 推荐数据");
                snapshot_with_recommendations(
                    lock_value(state),
                    format!("kkrb 获取失败，使用 mock：{error}"),
                    mock_recommendations(),
                )
            }
        }
    }
}

/// 提供无网络或失败兜底的本地推荐数据。
pub(crate) fn mock_recommendations() -> Vec<CraftRecommendation> {
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

/// 将锁定状态、更新时间和推荐列表组合成快照。
pub(crate) fn snapshot_with_recommendations(
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

//! 提供推荐快照组装和无网络时的兜底数据。

use crate::models::{AssistantSnapshot, CraftRecommendation};

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

//! 赞助商唯一性约束：Apinoria 是唯一 sponsor，其余一律 normal。
//!
//! 这条规则必须在客户端强制，而不能只靠远端 ads.json 摆对：
//! 远端文件任何人改一笔就会多出赞助位，而错误发生在用户机器上、无 CI 报警。
//! 因此这里以三条真实输入路径为载体逐项钉死——仓库内 ads.json、
//! 重复 Apinoria 输入、本地追加的内置条目。

use std::path::{Path, PathBuf};

use codex_plus_core::ads::{SOLE_SPONSOR_URL, normalize_ad_payload};
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("无法从 CARGO_MANIFEST_DIR 回溯到仓库根")
        .to_path_buf()
}

fn repository_ads_json() -> Value {
    let path = repo_root().join("ads.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()));
    serde_json::from_str(&text).expect("ads.json 不是合法 JSON")
}

fn sponsors(payload: &Value) -> Vec<&Value> {
    payload["ads"]
        .as_array()
        .expect("规范化结果缺少 ads")
        .iter()
        .filter(|ad| ad.get("type").and_then(Value::as_str) == Some("sponsor"))
        .collect()
}

fn assert_sole_apinoria_sponsor(payload: &Value, context: &str) {
    let sponsors = sponsors(payload);
    let ids: Vec<&str> = sponsors
        .iter()
        .map(|ad| ad.get("id").and_then(Value::as_str).unwrap_or("<无 id>"))
        .collect();
    assert_eq!(
        sponsors.len(),
        1,
        "{context}：sponsor 应恰好 1 条，实际 {} 条：{ids:?}",
        sponsors.len()
    );
    let sponsor = sponsors[0];
    assert_eq!(
        sponsor.get("id").and_then(Value::as_str),
        Some("apinoria"),
        "{context}：唯一 sponsor 必须是 Apinoria"
    );
    assert_eq!(
        sponsor.get("url").and_then(Value::as_str),
        Some("https://www.apinoria.com/"),
        "{context}：Apinoria 链接必须严格为 https://www.apinoria.com/"
    );
    assert_eq!(
        sponsor.get("title").and_then(Value::as_str),
        Some("派诺云"),
        "{context}：Apinoria 展示名应与概览页赞助位一致"
    );
}

#[test]
fn repository_ads_json_yields_apinoria_as_the_only_sponsor() {
    // 载体是仓库内真实数据，不是构造样例：真实数据里有 8 条远端 sponsor，
    // 只有强制降级生效才可能只剩 1 条。
    assert_sole_apinoria_sponsor(
        &normalize_ad_payload(repository_ads_json()),
        "仓库内 ads.json",
    );
}

#[test]
fn former_remote_sponsors_are_demoted_to_normal_and_kept() {
    // 降级不等于删除：这些条目仍要作为普通推荐展示，
    // 直接丢弃会让推荐列表凭空少掉大半内容。
    let payload = normalize_ad_payload(repository_ads_json());
    let ads = payload["ads"].as_array().unwrap();

    for id in [
        "jojocode-codex-relay",
        "0029-token-bridge",
        "volcengine-ark-agent-plan",
        "rawchat-codex-relay",
        "baikewei-ai",
    ] {
        let ad = ads
            .iter()
            .find(|ad| ad.get("id").and_then(Value::as_str) == Some(id))
            .unwrap_or_else(|| panic!("{id} 被整条丢弃，降级不应删除条目"));
        assert_eq!(
            ad.get("type").and_then(Value::as_str),
            Some("normal"),
            "{id} 应降级为普通推荐"
        );
    }
}

#[test]
fn locally_appended_builtins_are_normal_not_sponsor() {
    // 本地追加是另一条独立的分类来源：远端数据摆对了，
    // append_builtin_sponsors 仍可能凭空造出 sponsor。
    let payload = normalize_ad_payload(json!({
        "version": 1,
        "ads": [{
            "id": "remote-normal",
            "type": "normal",
            "title": "普通推荐",
            "description": "普通推荐内容",
            "url": "https://example.org"
        }]
    }));
    let ads = payload["ads"].as_array().unwrap();

    for id in [
        "cubence",
        "quya-cloud-bridge",
        "deepkey-api-key",
        "ergou-api",
        "apimart",
        "fenno-ai",
        "qiniu-ai",
    ] {
        let ad = ads
            .iter()
            .find(|ad| ad.get("id").and_then(Value::as_str) == Some(id))
            .unwrap_or_else(|| panic!("本地内置条目 {id} 未被追加"));
        assert_eq!(
            ad.get("type").and_then(Value::as_str),
            Some("normal"),
            "本地内置条目 {id} 应为普通推荐，不得作为赞助商"
        );
    }
    assert_sole_apinoria_sponsor(&payload, "仅含普通推荐的远端数据");
}

#[test]
fn apinoria_is_not_duplicated_when_remote_data_already_carries_it() {
    // 远端数据迟早会自己带上 Apinoria；那时若无条件再插一条，
    // 用户会看到两个一模一样的赞助位。
    let payload = normalize_ad_payload(json!({
        "version": 1,
        "ads": [
            {
                "id": "apinoria",
                "type": "sponsor",
                "title": "派诺云",
                "description": "远端自带的派诺云条目",
                "url": "https://www.apinoria.com/"
            },
            {
                "id": "apinoria-duplicate",
                "type": "sponsor",
                "title": "派诺云（重复投放）",
                "description": "同一家的第二条投放",
                "url": "https://www.apinoria.com/"
            },
            {
                "id": "other-sponsor",
                "type": "sponsor",
                "title": "别家",
                "description": "别家推荐内容",
                "url": "https://example.test"
            }
        ]
    }));

    assert_sole_apinoria_sponsor(&payload, "远端自带 Apinoria 且重复投放");
    let ads = payload["ads"].as_array().unwrap();
    let apinoria_count = ads
        .iter()
        .filter(|ad| ad.get("url").and_then(Value::as_str) == Some("https://www.apinoria.com/"))
        .count();
    assert_eq!(
        apinoria_count, 1,
        "指向 apinoria.com 的条目应去重到 1 条，实际 {apinoria_count} 条"
    );
}

#[test]
fn apinoria_is_injected_even_when_remote_payload_is_unusable() {
    // 远端拉不到或整份数据被过滤光时，赞助位不能空着：
    // 赞助商展示是对外承诺，不能因为数据源抖动就消失。
    let payload = normalize_ad_payload(json!({ "version": 1, "ads": [] }));
    assert_sole_apinoria_sponsor(&payload, "空的远端数据");
}

#[test]
fn apinoria_sponsor_leads_the_recommendation_list() {
    // 唯一赞助位排在末尾等于没投放。
    let payload = normalize_ad_payload(repository_ads_json());
    let ads = payload["ads"].as_array().unwrap();
    assert_eq!(
        ads[0].get("id").and_then(Value::as_str),
        Some("apinoria"),
        "Apinoria 应排在推荐列表首位"
    );
}

#[test]
fn injection_script_enforces_the_same_sole_sponsor_rule() {
    // 注入脚本是与 Rust 后端并列的第二条取数路径，它自己 normalize，
    // 不经 Rust。只在 Rust 侧强制会让插件菜单仍显示一排赞助商。
    let script = codex_plus_core::assets::injection_script(57321);
    assert!(
        script.contains("enforceCodexPlusSoleSponsor"),
        "注入脚本缺少赞助商唯一性约束"
    );
    assert!(
        script.contains(SOLE_SPONSOR_URL),
        "注入脚本未内置 Apinoria 链接"
    );
    // 约束必须挂在最终赋值点，而不是 normalize 内部：调用方用 normalize
    // 的返回长度判断本地后端是否给出内容，在那里注入会让回退分支永不成立。
    assert!(
        script.contains("codexPlusAds = enforceCodexPlusSoleSponsor(codexPlusAds);"),
        "注入脚本未在取数完成后强制赞助商唯一性"
    );
    assert!(
        script.contains("codexPlusAds = enforceCodexPlusSoleSponsor([]);"),
        "取数失败时赞助位会消失，赞助商展示是对外承诺"
    );
}

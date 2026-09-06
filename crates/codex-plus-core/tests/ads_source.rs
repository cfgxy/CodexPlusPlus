//! 广告数据源迁入本仓库后的约束。
//!
//! 数据源是运行时远端入口，改错会让全体用户拉到错误内容或直接拉不到，
//! 且失败发生在用户机器上、没有 CI 会报警，因此这里逐项钉死：
//! 两个默认 URL 必须都指向 cfgxy/CodexPlusPlus，仓库内 ads.json 必须
//! 是程序能直接读的合法 JSON，且不夹带凭据。

use std::path::{Path, PathBuf};

use codex_plus_core::ads::{DEFAULT_AD_LIST_URLS, normalize_ad_payload};
use serde_json::Value;

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <repo>/crates/codex-plus-core
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("无法从 CARGO_MANIFEST_DIR 回溯到仓库根")
        .to_path_buf()
}

fn ads_json_text() -> String {
    let path = repo_root().join("ads.json");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("读取 {} 失败：{error}", path.display()))
}

fn ads_json() -> Value {
    serde_json::from_str(&ads_json_text()).expect("ads.json 不是合法 JSON")
}

#[test]
fn default_ad_urls_point_at_cfgxy_repository_only() {
    assert_eq!(
        DEFAULT_AD_LIST_URLS,
        [
            "https://raw.githubusercontent.com/cfgxy/CodexPlusPlus/main/ads.json",
            "https://cdn.jsdelivr.net/gh/cfgxy/CodexPlusPlus@main/ads.json",
        ],
        "raw 为主源、jsDelivr 为备用源，顺序不能颠倒：raw 拿到的是即时内容，\
         jsDelivr 有 CDN 缓存延迟，主备写反会让更新迟迟不生效"
    );
}

#[test]
fn no_default_url_references_the_former_upstream() {
    for url in DEFAULT_AD_LIST_URLS {
        assert!(
            !url.contains("BigPizzaV3"),
            "默认广告源仍指向原上游仓库：{url}"
        );
        assert!(
            !url.contains("Ad-List"),
            "默认广告源仍指向已废弃的 Ad-List 仓库：{url}"
        );
    }
}

#[test]
fn repository_ads_json_is_machine_readable() {
    let payload = ads_json();
    assert_eq!(
        payload.get("version").and_then(Value::as_u64),
        Some(1),
        "ads.json 缺少 version 或不是数字"
    );
    let ads = payload
        .get("ads")
        .and_then(Value::as_array)
        .expect("ads.json 缺少 ads 数组");
    assert!(!ads.is_empty(), "ads.json 的 ads 数组为空");

    for ad in ads {
        let id = ad.get("id").and_then(Value::as_str).unwrap_or("<无 id>");
        for field in ["id", "type", "title"] {
            let value = ad.get(field).and_then(Value::as_str);
            assert!(
                value.is_some_and(|value| !value.trim().is_empty()),
                "{id} 的 {field} 缺失或为空"
            );
        }
        let ad_type = ad.get("type").and_then(Value::as_str).unwrap();
        assert!(
            matches!(ad_type, "sponsor" | "normal"),
            "{id} 的 type 只能是 sponsor 或 normal，实际：{ad_type}"
        );
    }
}

#[test]
fn repository_ads_json_survives_normalization() {
    // 真正的验收点不是「JSON 能解析」，而是「跑完 normalize 还剩得下内容」。
    // 字段缺失的条目会被 normalize 静默过滤掉，只校验可解析性发现不了。
    let normalized = normalize_ad_payload(ads_json());
    let normalized_ads = normalized["ads"].as_array().expect("规范化结果缺少 ads");

    let raw_ads = ads_json();
    let raw_ads = raw_ads["ads"].as_array().unwrap().clone();
    let kept = raw_ads
        .iter()
        .filter(|ad| {
            let id = ad.get("id").and_then(Value::as_str);
            normalized_ads
                .iter()
                .any(|kept| kept.get("id").and_then(Value::as_str) == id)
        })
        .count();

    // 上游数据里 0021-token-bridge 的 description/url 为空，会被 normalize
    // 依既有规则过滤，属预期行为，因此断言「大多数保留」而非「全部保留」。
    assert!(
        kept >= raw_ads.len() - 1,
        "ads.json 有 {} 条被规范化丢弃，超出预期（仅允许 1 条空字段占位）",
        raw_ads.len() - kept
    );
    assert!(
        normalized_ads
            .iter()
            .any(|ad| ad["id"] == serde_json::json!("qiniu-ai")),
        "本地内置推荐条目未被追加，本地追加逻辑可能已回归"
    );
}

#[test]
fn repository_ads_json_carries_no_credentials() {
    let text = ads_json_text();
    // 只查真正的凭据形态，不查 apikey/token 这类出现在赞助商名称里的普通词。
    for pattern in ["sk-", "\"secret\"", "\"password\"", "Bearer ", "ghp_"] {
        assert!(
            !text.contains(pattern),
            "ads.json 疑似包含凭据（命中 {pattern:?}），发布数据不得夹带秘密"
        );
    }
}

#[test]
fn repository_ads_json_images_do_not_depend_on_former_upstream() {
    // 数据迁到本仓却让图片继续从原上游仓拉，等于换了源仍受对方摆布：
    // 对方删文件或改分支，用户这边图片就全挂。
    for ad in ads_json()["ads"].as_array().unwrap() {
        let Some(image) = ad.get("image").and_then(Value::as_str) else {
            continue;
        };
        if image.trim().is_empty() {
            continue;
        }
        assert!(
            !image.contains("BigPizzaV3"),
            "{} 的图片仍指向原上游仓库：{image}",
            ad["id"]
        );
    }
}

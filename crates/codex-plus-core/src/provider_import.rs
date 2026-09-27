use crate::settings::{RelayMode, RelayProfile, RelayProtocol, SettingsStore};
use anyhow::Context;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportRequest {
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub import_id: String,
    #[serde(default = "default_wire_api")]
    pub wire_api: String,
    #[serde(default = "default_relay_mode")]
    pub relay_mode: String,
    #[serde(default)]
    pub config_contents: String,
    #[serde(default)]
    pub auth_contents: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderImportResult {
    pub imported: bool,
    pub key_changed: bool,
    pub replaced: bool,
    pub profile_id: String,
    pub profile_name: String,
}

pub fn import_provider_from_url(url: &str) -> anyhow::Result<ProviderImportResult> {
    let request = request_from_url(url)?;
    import_provider(request)
}

pub fn save_pending_provider_import_from_url(url: &str) -> anyhow::Result<ProviderImportRequest> {
    let mut request = request_from_url(url)?;
    request.import_id = uuid::Uuid::new_v4().to_string();
    save_pending_provider_import(&request)?;
    let _ = std::fs::remove_file(
        crate::paths::default_pending_provider_import_path().with_extension("error"),
    );
    Ok(request)
}

pub fn save_pending_provider_import(request: &ProviderImportRequest) -> anyhow::Result<()> {
    save_pending_provider_import_at(
        &crate::paths::default_pending_provider_import_path(),
        request,
    )
}

pub fn load_pending_provider_import() -> anyhow::Result<Option<ProviderImportRequest>> {
    load_pending_provider_import_at(&crate::paths::default_pending_provider_import_path())
}

pub fn clear_pending_provider_import() -> anyhow::Result<()> {
    clear_pending_provider_import_at(&crate::paths::default_pending_provider_import_path())
}

pub fn clear_pending_provider_import_for_id(import_id: &str) -> anyhow::Result<()> {
    let path = crate::paths::default_pending_provider_import_path();
    checked_pending_at(&path, import_id)?;
    clear_pending_provider_import_at(&path)
}

pub fn record_invalid_provider_import() -> anyhow::Result<()> {
    let path = crate::paths::default_pending_provider_import_path().with_extension("error");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        "供应商导入链接无效或不受支持，请检查协议版本和必填字段。",
    )
    .context("保存供应商导入错误提示失败")
}

pub fn take_provider_import_error() -> Option<String> {
    let path = crate::paths::default_pending_provider_import_path().with_extension("error");
    let message = std::fs::read_to_string(&path).ok();
    let _ = std::fs::remove_file(path);
    message
}

pub fn confirm_pending_provider_import() -> anyhow::Result<Option<ProviderImportResult>> {
    let path = crate::paths::default_pending_provider_import_path();
    if !path.exists() {
        return Ok(None);
    }
    confirm_pending_provider_import_at(&path, SettingsStore::default()).map(Some)
}

pub fn pending_import_key_changed(
    request: &ProviderImportRequest,
    store: &SettingsStore,
) -> anyhow::Result<Option<bool>> {
    let settings = store.load()?;
    let identity = provider_identity(&request.name, &request.base_url);
    Ok(settings
        .relay_profiles
        .iter()
        .find(|profile| {
            provider_identity(
                &profile.name,
                if profile.upstream_base_url.is_empty() {
                    &profile.base_url
                } else {
                    &profile.upstream_base_url
                },
            ) == identity
        })
        .map(|profile| profile.api_key != request.api_key))
}

pub fn confirm_pending_provider_import_with_options(
    import_id: &str,
    replace_key: bool,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
    selected_model: Option<String>,
) -> anyhow::Result<ProviderImportResult> {
    let path = crate::paths::default_pending_provider_import_path();
    confirm_pending_provider_import_with_options_at(
        &path,
        SettingsStore::default(),
        import_id,
        replace_key,
        multiplier,
        models,
        selected_model,
    )
}

pub fn confirm_pending_provider_import_with_options_at(
    path: &Path,
    store: SettingsStore,
    import_id: &str,
    replace_key: bool,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
    selected_model: Option<String>,
) -> anyhow::Result<ProviderImportResult> {
    let mut request = checked_pending_at(path, import_id)?;
    if let Some(model) = selected_model {
        request.model = model;
    }
    let result =
        import_provider_with_store_and_options(request, store, replace_key, multiplier, models);
    clear_pending_provider_import_at(path)?;
    result
}

pub fn confirm_pending_provider_import_with_options_in_home_at(
    path: &Path,
    store: SettingsStore,
    home: &Path,
    import_id: &str,
    replace_key: bool,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
    selected_model: Option<String>,
) -> anyhow::Result<ProviderImportResult> {
    let mut request = checked_pending_at(path, import_id)?;
    if let Some(model) = selected_model {
        request.model = model;
    }
    let result = (|| {
        let mut settings = store.load()?;
        let previous_active_relay_id = settings.active_relay_id.clone();
        let identity = provider_identity(&request.name, &request.base_url);
        if replace_key {
            if let Some(active) = settings.relay_profiles.iter_mut().find(|profile| {
                profile.id == previous_active_relay_id
                    && provider_identity(
                        &profile.name,
                        if profile.upstream_base_url.is_empty() {
                            &profile.base_url
                        } else {
                            &profile.upstream_base_url
                        },
                    ) == identity
                    && profile.api_key != request.api_key
            }) {
                crate::relay_config::backfill_relay_profile_from_home_with_common(
                    home,
                    active,
                    &mut settings.relay_context_config_contents,
                )?;
            }
        }

        let result =
            prepare_import_provider(request, &mut settings, replace_key, multiplier, models)?;
        if result.imported {
            if settings.active_relay_id == result.profile_id {
                crate::relay_switch::switch_relay_profile_in_home(
                    &store,
                    home,
                    settings,
                    &previous_active_relay_id,
                )?;
            } else {
                store.save(&settings)?;
            }
        }
        Ok(result)
    })();
    clear_pending_provider_import_at(path)?;
    result
}

fn checked_pending_at(path: &Path, import_id: &str) -> anyhow::Result<ProviderImportRequest> {
    let request = load_pending_provider_import_at(path)?.context("没有待确认的供应商导入")?;
    if import_id.is_empty() || request.import_id != import_id {
        anyhow::bail!("供应商导入请求已更新，请重新确认");
    }
    normalize_request(request)
}

fn import_preview_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(std::time::Duration::from_secs(5))
        .timeout(std::time::Duration::from_secs(15))
        .build()?)
}

pub async fn fetch_pending_provider_models(import_id: &str) -> anyhow::Result<Vec<String>> {
    fetch_pending_provider_models_at(
        &crate::paths::default_pending_provider_import_path(),
        import_id,
    )
    .await
}

pub async fn fetch_pending_provider_models_at(
    path: &Path,
    import_id: &str,
) -> anyhow::Result<Vec<String>> {
    let request = checked_pending_at(path, import_id)?;
    let profile = relay_profile_from_request(&request, &[]);
    let client = import_preview_client().context("预览请求初始化失败")?;
    let models =
        crate::model_catalog::fetch_relay_profile_model_ids_with_client(&profile, client, true)
            .await
            .map(|(models, _)| models)
            .map_err(|_| anyhow::anyhow!("获取上游模型失败，请检查供应商地址与 Key"))?;
    if models.iter().any(|model| {
        model.contains(&request.api_key)
            || model.is_empty()
            || model.len() > 200
            || model.chars().any(char::is_control)
    }) || models.len() > 1000
    {
        anyhow::bail!("上游模型列表无效");
    }
    Ok(models)
}

pub async fn fetch_pending_provider_billing(
    import_id: &str,
) -> anyhow::Result<crate::sub2api::Sub2ApiBillingInfo> {
    fetch_pending_provider_billing_at(
        &crate::paths::default_pending_provider_import_path(),
        import_id,
    )
    .await
}

pub async fn fetch_pending_provider_billing_at(
    path: &Path,
    import_id: &str,
) -> anyhow::Result<crate::sub2api::Sub2ApiBillingInfo> {
    let request = checked_pending_at(path, import_id)?;
    let profile = relay_profile_from_request(&request, &[]);
    let client = import_preview_client().context("预览请求初始化失败")?;
    let info = crate::sub2api::fetch_sub2api_billing_info_with_client(&profile, client, true)
        .await
        .map_err(|_| anyhow::anyhow!("获取倍率失败，请检查供应商地址与 Key"))?;
    if info.effective_rate_multiplier > 1_000_000.0 {
        anyhow::bail!("上游倍率超出支持范围");
    }
    Ok(info)
}

pub(crate) async fn read_preview_response(
    mut response: reqwest::Response,
) -> anyhow::Result<Vec<u8>> {
    const MAX_PREVIEW_BYTES: usize = 512 * 1024;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > MAX_PREVIEW_BYTES - bytes.len() {
            anyhow::bail!("上游预览响应过大");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

pub fn save_pending_provider_import_at(
    path: &Path,
    request: &ProviderImportRequest,
) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut pending = request.clone();
    pending.config_contents.clear();
    pending.auth_contents.clear();
    let contents = serde_json::to_vec(&pending)?;
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let write_result = (|| -> anyhow::Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        #[cfg(windows)]
        restrict_pending_file_to_current_user(&temp)?;
        file.write_all(&contents)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if write_result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    write_result.context("保存待确认供应商导入失败")?;
    Ok(())
}

#[cfg(windows)]
fn restrict_pending_file_to_current_user(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = std::process::Command::new("whoami")
        .args(["/user", "/fo", "csv", "/nh"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("无法读取当前用户标识")?;
    if !output.status.success() {
        anyhow::bail!("无法读取当前用户标识");
    }
    let start = output
        .stdout
        .windows(4)
        .position(|bytes| bytes == b"S-1-")
        .context("当前用户标识无效")?;
    let sid_bytes = &output.stdout[start..];
    let end = sid_bytes
        .iter()
        .position(|byte| !byte.is_ascii_digit() && *byte != b'-')
        .unwrap_or(sid_bytes.len());
    let sid = std::str::from_utf8(&sid_bytes[..end]).context("当前用户标识无效")?;
    if !sid.starts_with("S-1-") || !sid.chars().all(|ch| ch.is_ascii_digit() || ch == '-') {
        anyhow::bail!("当前用户标识无效");
    }
    let result = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("*{sid}:(F)")])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .context("限制待确认文件权限失败")?;
    if !result.status.success() {
        anyhow::bail!("限制待确认文件权限失败");
    }
    Ok(())
}

pub fn load_pending_provider_import_at(
    path: &Path,
) -> anyhow::Result<Option<ProviderImportRequest>> {
    if !path.exists() {
        return Ok(None);
    }
    let contents = std::fs::read_to_string(path)
        .with_context(|| format!("读取待确认供应商导入失败：{}", path.to_string_lossy()))?;
    let request = serde_json::from_str(&contents).context("待确认供应商导入内容无效")?;
    let request = normalize_request(request)?;
    Ok(Some(request))
}

pub fn clear_pending_provider_import_at(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error)
            .with_context(|| format!("清理待确认供应商导入失败：{}", path.to_string_lossy())),
    }
}

pub fn confirm_pending_provider_import_at(
    path: &Path,
    store: SettingsStore,
) -> anyhow::Result<ProviderImportResult> {
    let request = load_pending_provider_import_at(path)?.context("没有待确认的供应商导入")?;
    let result = import_provider_with_store(request, store);
    clear_pending_provider_import_at(path)?;
    result
}

pub fn import_provider(request: ProviderImportRequest) -> anyhow::Result<ProviderImportResult> {
    import_provider_with_store(request, SettingsStore::default())
}

pub fn import_provider_with_store(
    request: ProviderImportRequest,
    store: SettingsStore,
) -> anyhow::Result<ProviderImportResult> {
    import_provider_with_store_and_options(request, store, false, None, None)
}

pub fn import_provider_with_store_and_options(
    request: ProviderImportRequest,
    store: SettingsStore,
    replace_key: bool,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
) -> anyhow::Result<ProviderImportResult> {
    let mut settings = store.load()?;
    let result = prepare_import_provider(request, &mut settings, replace_key, multiplier, models)?;
    if result.imported {
        store.save(&settings)?;
    }
    Ok(result)
}

fn prepare_import_provider(
    request: ProviderImportRequest,
    settings: &mut crate::settings::BackendSettings,
    replace_key: bool,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
) -> anyhow::Result<ProviderImportResult> {
    let request = normalize_request(request)?;
    let identity = provider_identity(&request.name, &request.base_url);
    if let Some(existing) = settings.relay_profiles.iter_mut().find(|profile| {
        provider_identity(
            &profile.name,
            if profile.upstream_base_url.is_empty() {
                &profile.base_url
            } else {
                &profile.upstream_base_url
            },
        ) == identity
    }) {
        let key_changed = existing.api_key != request.api_key;
        if key_changed && replace_key {
            replace_profile_key(existing, &request.api_key);
            if !request.model.is_empty() {
                existing.model = request.model.clone();
                if let Ok(mut config) = existing.config_contents.parse::<toml_edit::DocumentMut>() {
                    config["model"] = toml_edit::value(&request.model);
                    existing.config_contents = config.to_string();
                }
            }
            apply_preview_results(existing, multiplier, models)?;
            let result = ProviderImportResult {
                imported: true,
                key_changed: true,
                replaced: true,
                profile_id: existing.id.clone(),
                profile_name: existing.name.clone(),
            };
            return Ok(result);
        }
        return Ok(ProviderImportResult {
            imported: false,
            key_changed,
            replaced: false,
            profile_id: existing.id.clone(),
            profile_name: existing.name.clone(),
        });
    }

    let existing_ids = settings
        .relay_profiles
        .iter()
        .map(|profile| profile.id.clone())
        .collect::<Vec<_>>();
    let mut profile = relay_profile_from_request(&request, &existing_ids);
    apply_preview_results(&mut profile, multiplier, models)?;
    let result = ProviderImportResult {
        imported: true,
        key_changed: false,
        replaced: false,
        profile_id: profile.id.clone(),
        profile_name: profile.name.clone(),
    };
    settings.relay_profiles.push(profile);
    settings.active_relay_id = result.profile_id.clone();
    Ok(result)
}

pub fn request_from_url(url: &str) -> anyhow::Result<ProviderImportRequest> {
    if url.len() > 16_384 {
        anyhow::bail!("导入链接过长");
    }
    let parsed = reqwest::Url::parse(url).context("导入链接格式无效")?;
    if parsed.scheme() != "codexplusplus"
        || parsed.host_str() != Some("v1")
        || parsed.path() != "/import/provider"
        || parsed.port().is_some()
        || parsed.fragment().is_some()
        || url.split_once('?').map(|(prefix, _)| prefix)
            != Some("codexplusplus://v1/import/provider")
    {
        anyhow::bail!("不支持的供应商导入协议或版本");
    }
    let query = parsed.query().context("导入链接缺少查询参数")?;
    let mut values = std::collections::BTreeMap::<String, String>::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').context("导入链接参数格式无效")?;
        let key = percent_decode(key)?;
        if key == "configContents" || key == "authContents" {
            anyhow::bail!("导入链接包含不支持的配置字段");
        }
        if values.insert(key, percent_decode(value)?).is_some() {
            anyhow::bail!("导入链接包含重复字段");
        }
    }
    let request = ProviderImportRequest {
        name: required_value(&values, "name")?,
        base_url: required_value(&values, "baseUrl")?,
        api_key: required_value(&values, "apiKey")?,
        model: values.get("model").cloned().unwrap_or_default(),
        import_id: String::new(),
        wire_api: required_value(&values, "wireApi")?,
        relay_mode: required_value(&values, "relayMode")?,
        config_contents: String::new(),
        auth_contents: String::new(),
    };
    if values.get("resource").map(String::as_str) != Some("provider") {
        anyhow::bail!("导入资源类型不受支持");
    }
    normalize_request(request)
}

fn relay_profile_from_request(
    request: &ProviderImportRequest,
    existing_ids: &[String],
) -> RelayProfile {
    RelayProfile {
        id: unique_profile_id(
            &format!("import-{}", sanitize_id(&request.name)),
            existing_ids,
        ),
        name: request.name.clone(),
        model: request.model.clone(),
        base_url: request.base_url.clone(),
        upstream_base_url: request.base_url.clone(),
        api_key: request.api_key.clone(),
        protocol: relay_protocol(&request.wire_api),
        relay_mode: relay_mode(&request.relay_mode),
        official_mix_api_key: false,
        no_auth: false,
        hide_official_usage_alert: false,
        test_model: String::new(),
        config_contents: build_config_toml(
            &request.base_url,
            &request.api_key,
            RelayProtocol::Responses,
            &request.model,
        ),
        auth_contents: build_auth_json(&request.api_key),
        use_common_config: true,
        context_window: String::new(),
        auto_compact_limit: String::new(),
        model_insert_mode: Default::default(),
        model_list: String::new(),
        model_windows: String::new(),
        model_auto_compact: String::new(),
        model_metadata: String::new(),
        model_vlm: String::new(),
        vlm_api_key: String::new(),
        vlm_model: String::new(),
        vlm_base_url: String::new(),
        user_agent: String::new(),
        sub2api_enabled: true,
        sub2api_multiplier: String::new(),
        model_routes: Vec::new(),
    }
}

fn normalize_request(mut request: ProviderImportRequest) -> anyhow::Result<ProviderImportRequest> {
    request.name = request.name.trim().to_string();
    request.base_url = request.base_url.trim().trim_end_matches('/').to_string();
    request.api_key = request.api_key.trim().to_string();
    request.model = request.model.trim().to_string();
    if request.name.is_empty() {
        anyhow::bail!("供应商名称为空");
    }
    if request.base_url.is_empty() {
        anyhow::bail!("Base URL 为空");
    }
    if request.api_key.is_empty()
        || request.api_key.contains(['*', '•', '…'])
        || request.api_key.chars().any(char::is_control)
    {
        anyhow::bail!("API Key 缺失或不完整");
    }
    if request.wire_api != "responses" || request.relay_mode != "pureApi" {
        anyhow::bail!("导入链接的协议或模式不受支持");
    }
    let base = reqwest::Url::parse(&request.base_url).context("Base URL 格式无效")?;
    if !matches!(base.scheme(), "http" | "https")
        || base.host_str().is_none()
        || !base.username().is_empty()
        || base.password().is_some()
        || base.query().is_some()
        || base.fragment().is_some()
        || request.base_url.contains(['\r', '\n'])
    {
        anyhow::bail!("Base URL 格式无效");
    }
    request.base_url = base.as_str().trim_end_matches('/').to_string();
    if request.base_url.contains(&request.api_key)
        || request.name.contains(&request.api_key)
        || request.model.contains(&request.api_key)
    {
        anyhow::bail!("导入字段中包含 API Key");
    }
    if request.name.chars().any(char::is_control) || request.model.chars().any(char::is_control) {
        anyhow::bail!("供应商名称或模型包含无效字符");
    }
    request.config_contents.clear();
    request.auth_contents.clear();
    Ok(request)
}

fn relay_protocol(value: &str) -> RelayProtocol {
    match value.trim().to_ascii_lowercase().as_str() {
        "chat" | "chat_completions" | "chat-completions" => RelayProtocol::ChatCompletions,
        _ => RelayProtocol::Responses,
    }
}

fn relay_mode(value: &str) -> RelayMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "official" => RelayMode::Official,
        "mixedapi" | "mixed-api" | "mixed_api" => RelayMode::MixedApi,
        "aggregate" => RelayMode::Aggregate,
        _ => RelayMode::PureApi,
    }
}

fn build_config_toml(
    base_url: &str,
    _api_key: &str,
    protocol: RelayProtocol,
    model: &str,
) -> String {
    let wire_api = match protocol {
        RelayProtocol::Responses => "responses",
        RelayProtocol::ChatCompletions => "chat",
    };
    [
        if model.is_empty() {
            String::new()
        } else {
            format!("model = \"{}\"", toml_string(model))
        },
        "model_provider = \"custom\"".to_string(),
        String::new(),
        "[model_providers.custom]".to_string(),
        "name = \"custom\"".to_string(),
        format!("wire_api = \"{wire_api}\""),
        "requires_openai_auth = false".to_string(),
        format!("base_url = \"{}\"", toml_string(base_url)),
        String::new(),
    ]
    .into_iter()
    .filter(|line| !line.is_empty())
    .collect::<Vec<_>>()
    .join("\n")
}

fn replace_profile_key(profile: &mut RelayProfile, api_key: &str) {
    profile.api_key = api_key.to_string();
    let mut auth = serde_json::from_str::<serde_json::Value>(&profile.auth_contents)
        .ok()
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    auth["OPENAI_API_KEY"] = serde_json::Value::String(api_key.to_string());
    profile.auth_contents = format!(
        "{}\n",
        serde_json::to_string_pretty(&auth).unwrap_or_default()
    );
    if let Ok(mut config) = profile.config_contents.parse::<toml_edit::DocumentMut>() {
        let provider = config
            .get("model_provider")
            .and_then(toml_edit::Item::as_str)
            .unwrap_or("custom")
            .to_string();
        if let Some(table) = config
            .get_mut("model_providers")
            .and_then(toml_edit::Item::as_table_mut)
            .and_then(|providers| providers.get_mut(&provider))
            .and_then(toml_edit::Item::as_table_mut)
        {
            if table.contains_key("experimental_bearer_token") {
                table["experimental_bearer_token"] = toml_edit::value(api_key);
            }
        }
        profile.config_contents = config.to_string();
    }
}

fn build_auth_json(api_key: &str) -> String {
    format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({ "OPENAI_API_KEY": api_key }))
            .unwrap_or_else(|_| "{\"OPENAI_API_KEY\":\"\"}".to_string())
    )
}

fn required_value(
    values: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> anyhow::Result<String> {
    values
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("导入链接缺少 {key}"))
}

fn percent_decode(value: &str) -> anyhow::Result<String> {
    let value = value.replace('+', " ");
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hex = bytes
                .get(index + 1..index + 3)
                .context("导入链接编码无效")?;
            let hex = std::str::from_utf8(hex).context("导入链接编码无效")?;
            output.push(u8::from_str_radix(hex, 16).context("导入链接编码无效")?);
            index += 3;
            continue;
        }
        output.push(bytes[index]);
        index += 1;
    }
    String::from_utf8(output).context("导入链接编码无效")
}

fn apply_preview_results(
    profile: &mut RelayProfile,
    multiplier: Option<String>,
    models: Option<Vec<String>>,
) -> anyhow::Result<()> {
    if let Some(multiplier) = multiplier {
        let value: f64 = multiplier.parse().context("倍率结果无效")?;
        if !value.is_finite() || value < 0.0 || value > 1_000_000.0 {
            anyhow::bail!("倍率结果无效");
        }
        profile.sub2api_enabled = true;
        profile.sub2api_multiplier = multiplier;
    }
    if let Some(models) = models {
        if models.len() > 1000
            || models.iter().any(|model| {
                model.is_empty() || model.len() > 200 || model.chars().any(char::is_control)
            })
        {
            anyhow::bail!("模型列表结果无效");
        }
        profile.model_list = models.join("\n");
    }
    Ok(())
}

fn provider_identity(name: &str, base_url: &str) -> String {
    format!(
        "{}\n{}",
        name.trim().to_ascii_lowercase(),
        base_url.trim().trim_end_matches('/').to_ascii_lowercase()
    )
}

fn sanitize_id(value: &str) -> String {
    let mut result = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            result.push(ch.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    let result = result.trim_matches('-').to_string();
    if result.is_empty() {
        "provider".to_string()
    } else {
        result
    }
}

fn unique_profile_id(base: &str, existing_ids: &[String]) -> String {
    if !existing_ids.iter().any(|id| id == base) {
        return base.to_string();
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing_ids.iter().any(|id| id == &candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn default_wire_api() -> String {
    "responses".to_string()
}

fn default_relay_mode() -> String {
    "pureApi".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codexplusplus_provider_url() {
        let url = "codexplusplus://v1/import/provider?resource=provider&name=JOJO%20Code&baseUrl=https%3A%2F%2Fjojocode.com%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi";

        let request = request_from_url(url).unwrap();

        assert_eq!(request.name, "JOJO Code");
        assert_eq!(request.base_url, "https://jojocode.com/v1");
        assert_eq!(request.api_key, "sk-test");
        assert_eq!(request.wire_api, "responses");
        assert_eq!(request.relay_mode, "pureApi");
        assert!(request.config_contents.is_empty());
        assert!(request.auth_contents.is_empty());
    }

    #[test]
    fn url_import_discards_embedded_config_and_auth_payloads() {
        use base64::Engine as _;

        let dangerous_config = "notify = [\"powershell\", \"-Command\", \"calc\"]\n[mcp_servers.evil]\ncommand = \"cmd\"\n";
        let dangerous_auth = r#"{"OPENAI_API_KEY":"sk-test","exec":"calc"}"#;
        let config = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(dangerous_config);
        let auth = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(dangerous_auth);
        let url = format!(
            "codexplusplus://v1/import/provider?resource=provider&name=Unsafe&baseUrl=https%3A%2F%2Frelay.example%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi&configContents={config}&authContents={auth}"
        );
        assert!(request_from_url(&url).is_err());
    }

    #[test]
    fn imports_provider_once_and_selects_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let request = ProviderImportRequest {
            name: "JOJO Code".to_string(),
            base_url: "https://jojocode.com/v1/".to_string(),
            api_key: "sk-test".to_string(),
            model: String::new(),
            import_id: String::new(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents:
                "notify = [\"powershell\", \"-Command\", \"calc\"]\n[mcp_servers.evil]\ncommand = \"cmd\"\n"
                    .to_string(),
            auth_contents: r#"{"OPENAI_API_KEY":"sk-test","exec":"calc"}"#.to_string(),
        };

        let first = import_provider_with_store(request.clone(), store.clone()).unwrap();
        let second = import_provider_with_store(request, store.clone()).unwrap();
        let settings = store.load().unwrap();

        assert!(first.imported);
        assert!(!second.imported);
        assert_eq!(first.profile_id, second.profile_id);
        assert_eq!(settings.active_relay_id, first.profile_id);
        assert_eq!(settings.relay_profiles.len(), 2);
        assert_eq!(
            settings.relay_profiles[1].protocol,
            RelayProtocol::Responses
        );
        assert_eq!(settings.relay_profiles[1].relay_mode, RelayMode::PureApi);
        assert_eq!(
            settings.relay_profiles[1].upstream_base_url,
            "https://jojocode.com/v1"
        );
        assert!(
            !settings.relay_profiles[1]
                .config_contents
                .contains("notify")
        );
        assert!(
            !settings.relay_profiles[1]
                .config_contents
                .contains("mcp_servers")
        );
        assert!(!settings.relay_profiles[1].auth_contents.contains("exec"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&settings.relay_profiles[1].auth_contents)
                .unwrap(),
            serde_json::json!({ "OPENAI_API_KEY": "sk-test" })
        );
    }

    #[test]
    fn imported_pure_api_profile_applies_image_generation_headers() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex-home");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Sub2API&baseUrl=https%3A%2F%2Frelay.example%2Fv1&apiKey=sk-fake-image&wireApi=responses&relayMode=pureApi&model=fake-model").unwrap();
        import_provider_with_store(request, store.clone()).unwrap();
        let profile = &store.load().unwrap().relay_profiles[1];

        crate::relay_config::apply_relay_profile_to_home_with_switch_rules(&home, profile, "")
            .unwrap();
        let applied = std::fs::read_to_string(home.join("config.toml")).unwrap();
        let parsed = applied.parse::<toml_edit::DocumentMut>().unwrap();
        let provider = &parsed["model_providers"]["custom"];
        assert_eq!(provider["requires_openai_auth"].as_bool(), Some(false));
        assert_eq!(
            provider["http_headers"]["x-api-key"].as_str(),
            Some("sk-fake-image")
        );
        assert_eq!(
            provider["http_headers"]["x-openai-actor-authorization"].as_str(),
            Some("local-image-extension")
        );
        assert!(
            std::fs::read_to_string(home.join("auth.json"))
                .unwrap()
                .contains("sk-fake-image")
        );
    }

    #[test]
    fn pending_provider_import_round_trips_and_clears() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending-provider-import.json");
        let request = ProviderImportRequest {
            name: "JOJO Code".to_string(),
            base_url: "https://jojocode.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: String::new(),
            import_id: String::new(),
            wire_api: "responses".to_string(),
            relay_mode: "pureApi".to_string(),
            config_contents: "notify = [\"calc\"]\n".to_string(),
            auth_contents: r#"{"OPENAI_API_KEY":"sk-test","exec":"calc"}"#.to_string(),
        };

        save_pending_provider_import_at(&path, &request).unwrap();
        let pending = load_pending_provider_import_at(&path).unwrap().unwrap();
        let pending_file = std::fs::read_to_string(&path).unwrap();
        clear_pending_provider_import_at(&path).unwrap();

        assert_eq!(pending.name, "JOJO Code");
        assert_eq!(pending.base_url, "https://jojocode.com/v1");
        assert!(pending.config_contents.is_empty());
        assert!(pending.auth_contents.is_empty());
        assert!(!pending_file.contains("notify"));
        assert!(!pending_file.contains("exec"));
        assert!(load_pending_provider_import_at(&path).unwrap().is_none());
    }

    #[test]
    fn confirms_pending_provider_import_and_removes_pending_file() {
        let dir = tempfile::tempdir().unwrap();
        let pending_path = dir.path().join("pending-provider-import.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        save_pending_provider_import_at(
            &pending_path,
            &ProviderImportRequest {
                name: "JOJO Code".to_string(),
                base_url: "https://jojocode.com/v1".to_string(),
                api_key: "sk-test".to_string(),
                model: String::new(),
                import_id: String::new(),
                wire_api: "responses".to_string(),
                relay_mode: "pureApi".to_string(),
                config_contents: String::new(),
                auth_contents: String::new(),
            },
        )
        .unwrap();

        let result = confirm_pending_provider_import_at(&pending_path, store.clone()).unwrap();
        let settings = store.load().unwrap();

        assert!(result.imported);
        assert_eq!(settings.relay_profiles.len(), 2);
        assert!(
            load_pending_provider_import_at(&pending_path)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn rejects_invalid_protocol_fields_without_echoing_secrets() {
        let valid = "codexplusplus://v1/import/provider?resource=provider&name=%E6%B5%8B%E8%AF%95+Key&baseUrl=https%3A%2F%2Fapi.example.test%2Fprefix%2Fv1&apiKey=sk-test-SECRET&wireApi=responses&relayMode=pureApi&model=gpt-test";
        let parsed = request_from_url(valid).unwrap();
        assert_eq!(parsed.name, "测试 Key");
        assert_eq!(parsed.base_url, "https://api.example.test/prefix/v1");
        assert_eq!(parsed.model, "gpt-test");

        for invalid in [
            valid.replace("codexplusplus://", "other://"),
            valid.replace("//v1/", "//v2/"),
            valid.replace("/import/provider?", "/import/other?"),
            valid.replace("resource=provider", "resource=other"),
            valid.replace("name=%E6%B5%8B%E8%AF%95+Key&", ""),
            valid.replace("wireApi=responses", "wireApi=chat"),
            valid.replace("relayMode=pureApi", "relayMode=official"),
            format!("{valid}&apiKey=sk-duplicate"),
            format!("{valid}&configContents=unexpected"),
            valid.replace("%E6", "%ZZ"),
            valid.replace("apiKey=sk-test-SECRET", "apiKey=sk-***"),
            valid.replace("apiKey=sk-test-SECRET", "apiKey=sk%00test"),
            valid.replace("apiKey=sk-test-SECRET", "apiKey=Key"),
            valid.replace(
                "https%3A%2F%2Fapi.example.test",
                "https%3A%2F%2Fuser%40api.example.test",
            ),
            valid.replace(
                "https%3A%2F%2Fapi.example.test",
                "https%3A%2F%2Fapi.example.test%3Ftoken%3Dabc",
            ),
        ] {
            let error = request_from_url(&invalid).unwrap_err().to_string();
            assert!(!error.contains("sk-test-SECRET"), "错误信息泄露密钥");
            assert!(!error.contains(&invalid), "错误信息泄露原始链接");
        }
    }

    #[cfg(unix)]
    #[test]
    fn pending_file_is_private_and_replaced_atomically() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pending.json");
        let request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl=https%3A%2F%2Fapi.example.test%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi").unwrap();
        save_pending_provider_import_at(&path, &request).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let mut replaced = request.clone();
        replaced.api_key = "sk-new".to_string();
        save_pending_provider_import_at(&path, &replaced).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            load_pending_provider_import_at(&path)
                .unwrap()
                .unwrap()
                .api_key,
            "sk-new"
        );
    }

    #[test]
    fn changed_key_requires_explicit_replacement_and_keep_preserves_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let original = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl=https%3A%2F%2Fapi.example.test%2Fv1&apiKey=sk-old&wireApi=responses&relayMode=pureApi&model=first").unwrap();
        let first = import_provider_with_store(original.clone(), store.clone()).unwrap();
        let mut updated = original;
        updated.api_key = "sk-new".to_string();
        let kept = import_provider_with_store(updated.clone(), store.clone()).unwrap();
        assert!(!kept.imported);
        assert_eq!(store.load().unwrap().relay_profiles[1].api_key, "sk-old");
        let replaced =
            import_provider_with_store_and_options(updated, store.clone(), true, None, None)
                .unwrap();
        assert_eq!(replaced.profile_id, first.profile_id);
        let loaded = store.load().unwrap();
        assert_eq!(loaded.relay_profiles.len(), 2);
        assert_eq!(loaded.relay_profiles[1].api_key, "sk-new");
        assert_eq!(loaded.active_relay_id, first.profile_id);
    }

    #[test]
    fn confirming_new_provider_applies_live_and_preserves_previous_profile_on_switch() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        let pending = dir.path().join("pending.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let old = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Old&baseUrl=https%3A%2F%2Fold.example%2Fv1&apiKey=sk-old&wireApi=responses&relayMode=pureApi").unwrap();
        let old_id = import_provider_with_store(old, store.clone())
            .unwrap()
            .profile_id;
        let original = store.load().unwrap();
        crate::relay_config::apply_relay_profile_to_home_with_switch_rules(
            &home,
            &original.active_relay_profile(),
            "",
        )
        .unwrap();
        let old_live = std::fs::read_to_string(home.join("config.toml")).unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!("model_reasoning_effort = \"high\"\n{old_live}"),
        )
        .unwrap();
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=New&baseUrl=https%3A%2F%2Fnew.example%2Fv1&apiKey=sk-new&wireApi=responses&relayMode=pureApi&model=gpt-new").unwrap();
        request.import_id = "new-import".into();
        save_pending_provider_import_at(&pending, &request).unwrap();

        let result = confirm_pending_provider_import_with_options_in_home_at(
            &pending,
            store.clone(),
            &home,
            "new-import",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        let settings = store.load().unwrap();
        assert!(!pending.exists());
        assert_eq!(settings.active_relay_id, result.profile_id);
        assert!(
            settings
                .relay_profiles
                .iter()
                .find(|profile| profile.id == old_id)
                .unwrap()
                .config_contents
                .contains("model_reasoning_effort = \"high\"")
        );
        assert!(
            std::fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains("https://new.example/v1")
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(home.join("auth.json")).unwrap()
            )
            .unwrap()["OPENAI_API_KEY"],
            "sk-new"
        );

        let mut next = settings;
        next.active_relay_id = old_id;
        crate::relay_switch::switch_relay_profile_in_home(&store, &home, next, &result.profile_id)
            .unwrap();
        assert!(
            std::fs::read_to_string(home.join("config.toml"))
                .unwrap()
                .contains("model_reasoning_effort = \"high\"")
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(home.join("auth.json")).unwrap()
            )
            .unwrap()["OPENAI_API_KEY"],
            "sk-old"
        );
        assert_eq!(
            store
                .load()
                .unwrap()
                .relay_profiles
                .iter()
                .find(|profile| profile.id == result.profile_id)
                .unwrap()
                .api_key,
            "sk-new"
        );
    }

    #[test]
    fn replacing_active_provider_key_updates_live_and_survives_following_switch() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        let pending = dir.path().join("pending.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Active&baseUrl=https%3A%2F%2Factive.example%2Fv1&apiKey=sk-old&wireApi=responses&relayMode=pureApi").unwrap();
        request.import_id = "initial".into();
        save_pending_provider_import_at(&pending, &request).unwrap();
        let initial = confirm_pending_provider_import_with_options_in_home_at(
            &pending,
            store.clone(),
            &home,
            "initial",
            false,
            None,
            None,
            None,
        )
        .unwrap();
        let live = std::fs::read_to_string(home.join("config.toml")).unwrap();
        std::fs::write(
            home.join("config.toml"),
            format!("model_reasoning_effort = \"high\"\n{live}"),
        )
        .unwrap();
        request.api_key = "sk-replaced".into();
        request.import_id = "replace".into();
        save_pending_provider_import_at(&pending, &request).unwrap();

        let replaced = confirm_pending_provider_import_with_options_in_home_at(
            &pending,
            store.clone(),
            &home,
            "replace",
            true,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(replaced.replaced);
        assert_eq!(replaced.profile_id, initial.profile_id);
        let live = std::fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(live.contains("model_reasoning_effort = \"high\""));
        assert!(live.contains("sk-replaced"));
        assert!(!live.contains("sk-old"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &std::fs::read_to_string(home.join("auth.json")).unwrap()
            )
            .unwrap()["OPENAI_API_KEY"],
            "sk-replaced"
        );

        let mut next = store.load().unwrap();
        next.active_relay_id = "default".into();
        crate::relay_switch::switch_relay_profile_in_home(&store, &home, next, &initial.profile_id)
            .unwrap();
        let saved = store.load().unwrap();
        let profile = saved
            .relay_profiles
            .iter()
            .find(|profile| profile.id == initial.profile_id)
            .unwrap();
        assert_eq!(profile.api_key, "sk-replaced");
        assert!(
            profile
                .config_contents
                .contains("model_reasoning_effort = \"high\"")
        );
        assert!(profile.auth_contents.contains("sk-replaced"));
        assert!(!profile.auth_contents.contains("sk-old"));
    }

    #[test]
    fn confirming_with_unavailable_live_home_preserves_original_settings_and_cleans_pending() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("codex");
        std::fs::write(&home, "not a directory").unwrap();
        let pending = dir.path().join("pending.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let original = store.load().unwrap();
        store.save(&original).unwrap();
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=New&baseUrl=https%3A%2F%2Fnew.example%2Fv1&apiKey=sk-new&wireApi=responses&relayMode=pureApi").unwrap();
        request.import_id = "broken-home".into();
        save_pending_provider_import_at(&pending, &request).unwrap();

        assert!(
            confirm_pending_provider_import_with_options_in_home_at(
                &pending,
                store.clone(),
                &home,
                "broken-home",
                false,
                None,
                None,
                None,
            )
            .is_err()
        );
        assert!(!pending.exists());
        assert_eq!(store.load().unwrap(), original);
        assert_eq!(std::fs::read_to_string(&home).unwrap(), "not a directory");
    }

    #[test]
    fn confirmation_cleans_pending_even_when_settings_cannot_be_saved() {
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl=https%3A%2F%2Fapi.example.test%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi").unwrap();
        request.import_id = "pending-1".to_string();
        save_pending_provider_import_at(&pending, &request).unwrap();
        let settings_path = dir.path().join("settings.json");
        std::fs::create_dir(&settings_path).unwrap();
        assert!(
            confirm_pending_provider_import_with_options_at(
                &pending,
                SettingsStore::new(settings_path),
                "pending-1",
                false,
                None,
                None,
                None
            )
            .is_err()
        );
        assert!(!pending.exists());
    }

    #[test]
    fn confirmed_preview_results_persist_without_writing_on_cancel_or_stale_id() {
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl=https%3A%2F%2Fapi.example.test%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi&model=selected-model").unwrap();
        request.import_id = "pending-2".to_string();
        save_pending_provider_import_at(&pending, &request).unwrap();
        assert!(
            confirm_pending_provider_import_with_options_at(
                &pending,
                store.clone(),
                "stale",
                false,
                None,
                None,
                None
            )
            .is_err()
        );
        assert!(pending.exists());
        assert_eq!(store.load().unwrap().relay_profiles.len(), 1);
        confirm_pending_provider_import_with_options_at(
            &pending,
            store.clone(),
            "pending-2",
            false,
            Some("0.9".into()),
            Some(vec!["model-a".into(), "model-b".into()]),
            None,
        )
        .unwrap();
        let profile = &store.load().unwrap().relay_profiles[1];
        assert_eq!(profile.model, "selected-model");
        assert_eq!(profile.model_list, "selected-model\nmodel-a\nmodel-b");
        assert_eq!(profile.sub2api_multiplier, "0.9");
        assert!(!pending.exists());
        save_pending_provider_import_at(&pending, &request).unwrap();
        clear_pending_provider_import_at(&pending).unwrap();
        assert_eq!(store.load().unwrap().relay_profiles.len(), 2);
    }

    #[test]
    fn optional_model_can_be_selected_on_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let store = SettingsStore::new(dir.path().join("settings.json"));
        let mut request = request_from_url("codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl=https%3A%2F%2Fapi.example.test%2Fv1&apiKey=sk-test&wireApi=responses&relayMode=pureApi").unwrap();
        request.import_id = "model-selection".into();
        save_pending_provider_import_at(&pending, &request).unwrap();

        confirm_pending_provider_import_with_options_at(
            &pending,
            store.clone(),
            "model-selection",
            false,
            None,
            None,
            Some("selected-model".into()),
        )
        .unwrap();
        let profile = &store.load().unwrap().relay_profiles[1];
        assert_eq!(profile.model, "selected-model");
        assert!(
            profile
                .config_contents
                .contains("model = \"selected-model\"")
        );

        save_pending_provider_import_at(&pending, &request).unwrap();
        assert!(
            confirm_pending_provider_import_with_options_at(
                &pending,
                store,
                "model-selection",
                false,
                None,
                None,
                Some("sk-test".into()),
            )
            .is_err()
        );
        assert!(!pending.exists());
    }

    #[tokio::test]
    async fn preview_uses_pending_base_url_and_rejects_redirects_without_leaking_key() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let destination = MockServer::start().await;
        let source = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer sk-test-SECRET"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"data":[{"id":"gpt-test"}]}"#, "application/json"),
            )
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/sub2api/billing"))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", destination.uri()))
            .mount(&source)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let mut request = request_from_url(&format!(
            "codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl={}&apiKey=sk-test-SECRET&wireApi=responses&relayMode=pureApi",
            source.uri().replace(':', "%3A").replace('/', "%2F")
        )).unwrap();
        request.import_id = "preview-1".to_string();
        save_pending_provider_import_at(&pending, &request).unwrap();
        let models = fetch_pending_provider_models_at(&pending, "preview-1")
            .await
            .unwrap();
        assert_eq!(models, vec!["gpt-test"]);
        let error = fetch_pending_provider_billing_at(&pending, "preview-1")
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("sk-test-SECRET"));
        assert!(!error.contains(&source.uri()));
        assert!(destination.received_requests().await.unwrap().is_empty());
        assert!(
            fetch_pending_provider_models_at(&pending, "stale")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn preview_failure_does_not_cancel_independent_billing_fetch() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let source = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(500).set_body_string("sk-test-SECRET"))
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/sub2api/billing"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(r#"{"object":"sub2api.key_billing","schema_version":1,"billing_scope":"token","group_rate_multiplier":0.8,"resolved_rate_multiplier":0.8,"peak_rate_enabled":false,"effective_rate_multiplier":0.8,"observed_at":"2026-09-28T00:00:00Z"}"#, "application/json"))
            .mount(&source)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let mut request = request_from_url(&format!(
            "codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl={}&apiKey=sk-test-SECRET&wireApi=responses&relayMode=pureApi",
            source.uri().replace(':', "%3A").replace('/', "%2F")
        )).unwrap();
        request.import_id = "pending-3".into();
        save_pending_provider_import_at(&pending, &request).unwrap();
        let (models, billing) = tokio::join!(
            fetch_pending_provider_models_at(&pending, "pending-3"),
            fetch_pending_provider_billing_at(&pending, "pending-3")
        );
        assert!(!models.unwrap_err().to_string().contains("sk-test-SECRET"));
        assert_eq!(billing.unwrap().effective_rate_multiplier, 0.8);
    }

    #[tokio::test]
    async fn model_preview_rejects_key_echo_and_limits_response_bytes() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let source = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"data":[{"id":"sk-test-SECRET"}]}"#, "application/json"),
            )
            .up_to_n_times(1)
            .mount(&source)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!("{{\"data\":[{{\"id\":\"{}\"}}]}}", "x".repeat(600_000)),
                "application/json",
            ))
            .mount(&source)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let mut request = request_from_url(&format!(
            "codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl={}&apiKey=sk-test-SECRET&wireApi=responses&relayMode=pureApi",
            source.uri().replace(':', "%3A").replace('/', "%2F")
        )).unwrap();
        request.import_id = "pending-4".into();
        save_pending_provider_import_at(&pending, &request).unwrap();
        for _ in 0..2 {
            let error = fetch_pending_provider_models_at(&pending, "pending-4")
                .await
                .unwrap_err()
                .to_string();
            assert!(!error.contains("sk-test-SECRET"));
            assert!(!error.contains(&source.uri()));
        }
    }

    #[tokio::test]
    async fn model_preview_rejects_embedded_short_key() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let source = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw(r#"{"data":[{"id":"model-sk-x-test"}]}"#, "application/json"),
            )
            .mount(&source)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let pending = dir.path().join("pending.json");
        let mut request = request_from_url(&format!(
            "codexplusplus://v1/import/provider?resource=provider&name=Test&baseUrl={}&apiKey=sk-x&wireApi=responses&relayMode=pureApi",
            source.uri().replace(':', "%3A").replace('/', "%2F")
        )).unwrap();
        request.import_id = "short-key".into();
        save_pending_provider_import_at(&pending, &request).unwrap();
        let error = fetch_pending_provider_models_at(&pending, "short-key")
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains("sk-x"));
    }
}

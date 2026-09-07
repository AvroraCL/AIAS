use tauri::{Manager, Webview};
use tauri_plugin_updater::UpdaterExt;

// Use a normal updater resource so downloads still require signature verification.
#[tauri::command]
pub async fn updater_check_mirror(webview: Webview) -> Result<Option<serde_json::Value>, String> {
    let config: serde_json::Value =
        serde_json::from_str(include_str!("../tauri.conf.json")).map_err(|e| e.to_string())?;
    let endpoint = config["plugins"]["updater"]["endpoints"][1]
        .as_str()
        .ok_or("未配置备用更新源")?;
    let updater = webview
        .updater_builder()
        .endpoints(vec![endpoint
            .parse()
            .map_err(|e| format!("无效更新源：{e}"))?])
        .map_err(|e| e.to_string())?
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Ok(None);
    };
    let mut metadata = serde_json::json!({
        "currentVersion": update.current_version,
        "version": update.version,
        "date": null,
        "body": update.body,
        "rawJson": update.raw_json,
    });
    metadata["rid"] = serde_json::json!(webview.resources_table().add(update));
    Ok(Some(metadata))
}

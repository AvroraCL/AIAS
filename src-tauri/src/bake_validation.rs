//! Explicit opt-in integration runner, excluded from production builds.
use serde_json::{json, Value};
use tauri::{AppHandle, Listener, Manager};
pub fn start(app: &AppHandle) -> bool {
    let Some(path) = std::env::var_os("AIAS_BAKE_VALIDATION") else {
        return false;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let path = std::path::PathBuf::from(path);
        let result = run(app.clone(), &path).await;
        let report = match result {
            Ok(v) => json!({"passed":true,"results":v}),
            Err(e) => json!({"passed":false,"error":e}),
        };
        let _ = std::fs::write(
            path.with_extension("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        );
        app.exit(if report["passed"] == true { 0 } else { 1 });
    });
    true
}
async fn run(app: AppHandle, path: &std::path::Path) -> Result<Value, String> {
    use crate::model_bake::*;
    let config: Value = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let capabilities = bake_capabilities(app.clone()).await?;
    let imported =
        bake_import(app.clone(), config["input"].as_str().ok_or("input")?.into()).await?;
    let handle = imported["handle"].as_str().ok_or("handle")?.to_owned();
    let mesh_path = std::path::PathBuf::from(imported["meshPath"].as_str().ok_or("meshPath")?);
    let objects: Vec<usize> = imported["objects"]
        .as_array()
        .ok_or("objects")?
        .iter()
        .map(|o| o["id"].as_u64().unwrap() as usize)
        .collect();
    let reports = bake_inspect(
        app.clone(),
        handle.clone(),
        objects.clone(),
        json!({"0":0,"1":0}),
    )
    .await?;
    if !reports
        .as_array()
        .ok_or("reports")?
        .iter()
        .all(|r| r["valid"] == true)
    {
        return Err("fixture UV unexpectedly invalid".into());
    }
    let progress = std::sync::Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = progress.clone();
    let listener = app.listen("bake-progress", move |e| {
        if let Ok(v) = serde_json::from_str(e.payload()) {
            captured.lock().unwrap().push(v);
        }
    });
    let options = json!({"output":config["output"],"device":0,"objects":objects,"materials":[0,1],"channels":{"0":0,"1":0},"resolution":512,"samples":32,"distance":0.5,"margin":16,"selfOnly":false,"ao":true,"uv":true,"id":true,"bits":8});
    let first = bake_start(
        app.clone(),
        handle.clone(),
        "native-first".into(),
        options.clone(),
    )
    .await?;
    if first["files"].as_array().map(Vec::len) != Some(6)
        || !first["failures"].as_array().is_some_and(Vec::is_empty)
    {
        return Err(format!("native export failed: {first}"));
    }
    let mut slow = options.clone();
    slow["resolution"] = json!(2048);
    slow["samples"] = json!(256);
    let (a, h) = (app.clone(), handle.clone());
    let task =
        tauri::async_runtime::spawn(
            async move { bake_start(a, h, "native-cancel".into(), slow).await },
        );
    // Poll a real progress event, avoiding timing assumptions about worker startup.
    for _ in 0..400 {
        if progress.lock().unwrap().iter().any(|p| {
            p["jobId"] == "native-cancel" && p["data"]["progress"].as_f64().unwrap_or(0.) > 0.
        }) {
            break;
        }
        tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(50))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    bake_cancel("native-cancel".into())?;
    let cancelled = task.await.map_err(|e| e.to_string())??;
    if cancelled["cancelled"] != true {
        return Err(format!("cancel did not stop job: {cancelled}"));
    }
    let mut faults = Vec::new();
    for (name, suspend) in [("native-hung", true), ("native-crash", false)] {
        let (a, h) = (app.clone(), handle.clone());
        let mut slow = options.clone();
        slow["resolution"] = json!(2048);
        slow["samples"] = json!(256);
        let task =
            tauri::async_runtime::spawn(async move { bake_start(a, h, name.into(), slow).await });
        for _ in 0..400 {
            if progress
                .lock()
                .unwrap()
                .iter()
                .any(|p| p["jobId"] == name && p["data"]["progress"].as_f64().unwrap_or(0.) > 0.)
            {
                break;
            }
            tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(std::time::Duration::from_millis(50))
            })
            .await
            .map_err(|e| e.to_string())?;
        }
        validation_fault(name, suspend)?;
        if suspend {
            bake_cancel(name.into())?;
        }
        let result = task.await.map_err(|e| e.to_string())??;
        if result["failures"].as_array().is_none_or(Vec::is_empty) {
            return Err(format!("fault not reported: {result}"));
        }
        faults.push(result);
    }
    let retry = bake_start(app.clone(), handle.clone(), "native-retry".into(), options).await?;
    if retry["files"].as_array().map(Vec::len) != Some(6) {
        return Err("native retry failed".into());
    }
    let asset_result = std::sync::Arc::new(std::sync::Mutex::new(None::<Value>));
    let captured = asset_result.clone();
    let asset_listener = app.listen("bake-validation-assets", move |event| {
        *captured.lock().unwrap() = serde_json::from_str(event.payload()).ok();
    });
    let paths = json!([mesh_path, retry["files"][2]["path"]]);
    let js = format!(
        r#"(async()=>{{try{{const paths={paths};const results=[];for(const path of paths){{const url=window.__TAURI_INTERNALS__.convertFileSrc(path,'asset');const response=await fetch(url);results.push({{path,status:response.status,bytes:(await response.arrayBuffer()).byteLength}});}}await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'bake-validation-assets',payload:{{results}}}});}}catch(error){{await window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'bake-validation-assets',payload:{{error:String(error)}}}});}}}})();"#
    );
    app.get_webview_window("main")
        .ok_or("validation webview missing")?
        .eval(&js)
        .map_err(|e| e.to_string())?;
    for _ in 0..100 {
        if asset_result.lock().unwrap().is_some() {
            break;
        }
        tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(std::time::Duration::from_millis(50))
        })
        .await
        .map_err(|e| e.to_string())?;
    }
    let assets = asset_result
        .lock()
        .unwrap()
        .clone()
        .ok_or("asset protocol validation timed out")?;
    if assets["error"].is_string()
        || !assets["results"].as_array().is_some_and(|r| {
            r.len() == 2
                && r.iter()
                    .all(|v| v["status"] == 200 && v["bytes"].as_u64().unwrap_or(0) > 0)
        })
    {
        return Err(format!("asset read failed: {assets}"));
    }
    app.unlisten(asset_listener);
    app.unlisten(listener);
    bake_release(handle)?;
    if mesh_path.exists() {
        return Err("model temp directory not released".into());
    }
    let events = progress.lock().unwrap().clone();
    Ok(
        json!({"capabilities":capabilities,"import":imported,"reports":reports,"first":first,"cancelled":cancelled,"faults":faults,"retry":retry,"assets":assets,"progressEvents":events.len(),"released":true}),
    )
}

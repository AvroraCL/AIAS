//! The renderer owns no geometry parser. A disposable DXR worker owns all model data.
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, LazyLock, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
static MODELS: LazyLock<Mutex<HashMap<String, tempfile::TempDir>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static JOBS: LazyLock<Mutex<HashMap<String, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CANCEL_REQUESTS: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
#[cfg(feature = "bake-validation")]
static WORKER_IDS: LazyLock<Mutex<HashMap<String, u32>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
fn id() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}
fn worker(app: &AppHandle) -> Result<PathBuf, String> {
    let mut candidates = vec![app
        .path()
        .resource_dir()
        .map_err(|e| e.to_string())?
        .join("bake/aias-bake-worker.exe")];
    if cfg!(debug_assertions) {
        candidates.insert(
            0,
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../bake-worker/target/debug/aias-bake-worker.exe"),
        );
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../bake-worker/target/release/aias-bake-worker.exe"),
        );
    }
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .ok_or("未找到模型烘焙工作进程，请重新安装完整安装包".into())
}
fn command(app: &AppHandle) -> Result<Command, String> {
    let mut c = Command::new(worker(app)?);
    c.creation_flags(0x08000000);
    Ok(c)
}
fn execute(
    app: &AppHandle,
    args: &[&std::ffi::OsStr],
    job: &str,
    cancel: Option<&Path>,
) -> Result<Value, String> {
    let mut child = command(app)?
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动烘焙工作进程失败：{e}"))?;
    #[cfg(feature = "bake-validation")]
    WORKER_IDS.lock().unwrap().insert(job.into(), child.id());
    let stdout = child.stdout.take().ok_or("工作进程缺失输出")?;
    let stderr = child.stderr.take().ok_or("工作进程缺失错误输出")?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        for line in BufReader::new(stderr).lines() {
            if let Ok(line) = line {
                if text.len() < 16384 {
                    text.push_str(&line);
                    text.push('\n');
                }
            }
        }
        text
    });
    let mut result = None;
    let mut failure = None;
    let mut cancel_at = None;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                if let Ok(message) = serde_json::from_str::<Value>(&line) {
                    if message
                        .get("jobId")
                        .and_then(Value::as_str)
                        .is_some_and(|j| j != job)
                    {
                        continue;
                    }
                    match message["type"].as_str() {
                        Some("progress") => {
                            let _ = app.emit("bake-progress", &message);
                        }
                        Some("result") => result = Some(message["data"].clone()),
                        Some("error") => {
                            failure = Some(
                                message["error"]
                                    .as_str()
                                    .unwrap_or("工作进程错误")
                                    .to_string(),
                            )
                        }
                        _ => {
                            if message.get("devices").is_some() {
                                result = Some(message);
                            }
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if cancel.is_some_and(Path::exists) {
            let since = cancel_at.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(3) {
                let _ = child.kill();
                failure = Some("任务已取消；未响应的工作进程已终止".into());
                break;
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    #[cfg(feature = "bake-validation")]
    WORKER_IDS.lock().unwrap().remove(job);
    let _ = reader.join();
    let stderr = errors.join().unwrap_or_default();
    if let Some(e) = failure {
        return Err(e);
    }
    if !status.success() {
        return Err(format!("烘焙工作进程退出（{status}）：{stderr}"));
    }
    result.ok_or("工作进程未返回任务结果".into())
}
#[tauri::command]
pub async fn bake_capabilities(app: AppHandle) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || execute(&app, &[], "", None))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn bake_import(app: AppHandle, path: String) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = crate::safety::task_guard()?;
        let source = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
        let mut sys = sysinfo::System::new();
        sys.refresh_memory();
        let bytes = std::fs::metadata(&source).map_err(|e| e.to_string())?.len();
        if bytes.saturating_mul(20) > sys.available_memory() / 2 {
            return Err("模型文件过大，当前系统内存不足以安全导入".into());
        }
        let temp = tempfile::Builder::new()
            .prefix("aias-model-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let handle = id();
        let mut data = execute(
            &app,
            &[
                "import".as_ref(),
                source.as_os_str(),
                handle.as_ref(),
                temp.path().as_os_str(),
            ],
            &handle,
            None,
        )?;
        data["handle"] = json!(handle);
        MODELS
            .lock()
            .map_err(|_| "模型状态锁损坏")?
            .insert(handle, temp);
        Ok(data)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn bake_start(
    app: AppHandle,
    handle: String,
    job_id: String,
    mut options: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = crate::safety::task_guard()?;
        if job_id.is_empty() || job_id.len() > 100 {
            return Err("任务编号无效".into());
        }
        let started = Instant::now();
        let model_path = MODELS
            .lock()
            .map_err(|_| "模型状态锁损坏")?
            .get(&handle)
            .ok_or("模型句柄已失效，请重新导入")?
            .path()
            .join("model.json");
        let root = PathBuf::from(
            options["output"]
                .as_str()
                .filter(|s| !s.trim().is_empty())
                .ok_or("请选择输出目录")?,
        );
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let root = std::fs::canonicalize(root).map_err(|e| e.to_string())?;
        let output = root.join(format!("AIAS_bake_{}", id()));
        std::fs::create_dir(&output).map_err(|e| e.to_string())?;
        let temp = tempfile::Builder::new()
            .prefix("aias-bake-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let cancel = temp.path().join("cancel");
        let request = temp.path().join("request.json");
        options["jobId"] = json!(job_id);
        options["modelPath"] = json!(model_path);
        options["output"] = json!(output);
        options["cancelPath"] = json!(cancel);
        std::fs::write(
            &request,
            serde_json::to_vec(&options).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        JOBS.lock()
            .map_err(|_| "任务状态锁损坏")?
            .insert(job_id.clone(), cancel.clone());
        if CANCEL_REQUESTS
            .lock()
            .map_err(|_| "任务状态锁损坏")?
            .remove(&job_id)
        {
            std::fs::write(&cancel, b"cancel").map_err(|e| e.to_string())?;
        }
        let response = execute(
            &app,
            &["bake".as_ref(), request.as_os_str(), job_id.as_ref()],
            &job_id,
            Some(&cancel),
        );
        JOBS.lock().map_err(|_| "任务状态锁损坏")?.remove(&job_id);
        CANCEL_REQUESTS
            .lock()
            .map_err(|_| "任务状态锁损坏")?
            .remove(&job_id);
        match response {
            Ok(data) => Ok(data),
            Err(error) => {
                let mut data = std::fs::read(output.join("result.json"))
                    .ok()
                    .and_then(|v| serde_json::from_slice::<Value>(&v).ok())
                    .unwrap_or(json!({"jobId":job_id,"directory":output,"files":[],"failures":[]}));
                data["cancelled"] = json!(cancel.exists());
                data["elapsedMs"] = json!(started.elapsed().as_millis());
                data["peakDeviceBytes"] = Value::Null;
                data["failures"]
                    .as_array_mut()
                    .ok_or("无效任务结果")?
                    .push(json!(error));
                let mut unfinished = Vec::new();
                for material in options["materials"].as_array().into_iter().flatten() {
                    for kind in ["ao", "uv", "id"] {
                        if options[kind] == true
                            && !data["files"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|f| f["material"] == *material && f["kind"] == kind)
                        {
                            unfinished.push(json!({"material":material,"kind":kind}));
                        }
                    }
                }
                for item in &unfinished {
                    data["failures"].as_array_mut().unwrap().push(json!(format!(
                        "材质 {} 未完成 {}",
                        item["material"],
                        item["kind"].as_str().unwrap().to_uppercase()
                    )));
                }
                data["unfinished"] = json!(unfinished);
                // The worker's temporary PNGs are in this newly created, uniquely owned directory.
                for entry in std::fs::read_dir(&output)
                    .map_err(|e| e.to_string())?
                    .flatten()
                {
                    if entry.file_name().to_string_lossy().starts_with(".tmp") {
                        let _ = std::fs::remove_file(entry.path());
                    }
                }
                crate::safety::atomic_write(&output.join("result.json"), |w| {
                    use std::io::Write;
                    w.write_all(&serde_json::to_vec_pretty(&data).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())
                })?;
                Ok(data)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn bake_inspect(
    app: AppHandle,
    handle: String,
    objects: Vec<usize>,
    channels: Value,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = crate::safety::task_guard()?;
        let model_path = MODELS
            .lock()
            .map_err(|_| "模型状态锁损坏")?
            .get(&handle)
            .ok_or("模型句柄已失效")?
            .path()
            .join("model.json");
        let temp = tempfile::NamedTempFile::new().map_err(|e| e.to_string())?;
        std::fs::write(
            temp.path(),
            serde_json::to_vec(
                &json!({"modelPath":model_path,"objects":objects,"channels":channels}),
            )
            .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        execute(
            &app,
            &[
                "inspect".as_ref(),
                temp.path().as_os_str(),
                "inspect".as_ref(),
            ],
            "inspect",
            None,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub fn bake_cancel(job_id: String) -> Result<(), String> {
    if job_id.is_empty() || job_id.len() > 100 {
        return Err("任务编号无效".into());
    }
    let jobs = JOBS.lock().map_err(|_| "任务状态锁损坏")?;
    if let Some(path) = jobs.get(&job_id) {
        std::fs::write(path, b"cancel").map_err(|e| e.to_string())?;
    } else {
        let mut pending = CANCEL_REQUESTS.lock().map_err(|_| "任务状态锁损坏")?;
        if pending.len() >= 256 {
            pending.clear();
        }
        pending.insert(job_id);
    }
    Ok(())
}
#[tauri::command]
pub fn bake_release(handle: String) -> Result<(), String> {
    let _guard = crate::safety::task_guard()?;
    MODELS.lock().map_err(|_| "模型状态锁损坏")?.remove(&handle);
    Ok(())
}

#[cfg(feature = "bake-validation")]
pub fn validation_fault(job: &str, suspend: bool) -> Result<(), String> {
    type Handle = *mut std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn CloseHandle(handle: Handle) -> i32;
        fn TerminateProcess(handle: Handle, code: u32) -> i32;
    }
    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtSuspendProcess(handle: Handle) -> i32;
    }
    let pid = *WORKER_IDS
        .lock()
        .unwrap()
        .get(job)
        .ok_or("validation worker is not running")?;
    unsafe {
        let handle = OpenProcess(if suspend { 0x0800 } else { 1 }, 0, pid);
        if handle.is_null() {
            return Err("validation OpenProcess failed".into());
        }
        let ok = if suspend {
            NtSuspendProcess(handle) >= 0
        } else {
            TerminateProcess(handle, 91) != 0
        };
        CloseHandle(handle);
        if !ok {
            return Err("validation process fault injection failed".into());
        }
    }
    Ok(())
}

//! The renderer owns no geometry parser. A disposable DXR worker owns all model data.
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{mpsc, LazyLock, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
static MODELS: LazyLock<Mutex<HashMap<String, tempfile::TempDir>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RESULTS: LazyLock<Mutex<HashMap<String, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static JOBS: LazyLock<Mutex<HashMap<String, PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CANCEL_REQUESTS: LazyLock<Mutex<std::collections::HashSet<String>>> =
    LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));
/// 当前正在执行的烘焙任务 id：让全局"停止"按钮（task_cancel）能桥接到
/// 烘焙自己的取消文件机制。
static ACTIVE_BAKE: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));
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

/// 把工作进程挂入 KILL_ON_JOB_CLOSE 的 Job Object。宿主进程退出（含 panic
/// 闪退）时内核关闭 job 句柄并随之终止工作进程，烘焙中关窗不再产生继续
/// 占用 GPU/内存、往缓存写盘的孤儿进程。
#[cfg(windows)]
struct WorkerJob(windows::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for WorkerJob {
    fn drop(&mut self) {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn attach_kill_on_close(child: &std::process::Child) -> Option<WorkerJob> {
    use std::os::windows::io::AsRawHandle;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // let-else 的 scrutinee 不能直接是 unsafe 块（`} else` 解析歧义）。
    let created = unsafe { CreateJobObjectW(None, PCWSTR::null()) };
    let Ok(job) = created else {
        return None;
    };
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &info as *const _ as *const std::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    let assigned = unsafe { AssignProcessToJobObject(job, HANDLE(child.as_raw_handle() as _)) };
    if configured.is_err() || assigned.is_err() {
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(job);
        }
        return None;
    }
    Some(WorkerJob(job))
}
/// 工作进程无输出的兜底上限：正常任务会持续产出 progress/result 行，
/// 超时即视为挂死。挂死的任务会一直持有全局任务锁，必须由这里终止。
const STALL_QUICK: Duration = Duration::from_secs(60);
// 导入期有心跳喂狗（解析大模型可静默数分钟），阈值只需兜底真挂死。
const STALL_IMPORT: Duration = Duration::from_secs(300);
const STALL_BAKE: Duration = Duration::from_secs(300);

fn execute(
    app: &AppHandle,
    args: &[&std::ffi::OsStr],
    job: &str,
    cancel: Option<&Path>,
    stall: Duration,
) -> Result<Value, String> {
    let mut child = command(app)?
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动烘焙工作进程失败：{e}"))?;
    // RAII 持有到 worker 完全退出；提前返回时 Drop 也会关闭 Job Object 并
    // 终止仍存活的子进程，不再每次任务泄漏一个内核句柄。
    #[cfg(windows)]
    let _worker_job = attach_kill_on_close(&child);
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
            let Ok(line) = line else { continue };
            if text.len() < 16384 {
                text.push_str(&line);
                text.push('\n');
            }
        }
        text
    });
    let mut result = None;
    let mut failure = None;
    let mut cancel_at = None;
    let mut last_output = Instant::now();
    // stdout 读取线程结束（正常 EOF，或读取失败但进程仍存活）后不能直接
    // 退出监视循环：取消检查与 stall 看门狗必须持续到 wait() 收尸，否则
    // 卡死的工作进程会永久占用全局任务锁、取消按钮失效。
    let mut stdout_lost = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                last_output = Instant::now();
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
            Err(mpsc::RecvTimeoutError::Disconnected) => stdout_lost = true,
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
        if last_output.elapsed() > stall {
            let _ = child.kill();
            failure = Some(format!("工作进程超过 {} 秒无输出，已终止", stall.as_secs()));
            break;
        }
        if stdout_lost {
            // 断开的通道让 recv_timeout 立即返回，降级为低频轮询防忙等。
            std::thread::sleep(Duration::from_millis(200));
            if matches!(child.try_wait(), Ok(Some(_))) {
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
    tauri::async_runtime::spawn_blocking(move || execute(&app, &[], "", None, STALL_QUICK))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn bake_import(
    app: AppHandle,
    path: String,
    uv_mode: Option<String>,
) -> Result<Value, String> {
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
        let uv_mode = match uv_mode.as_deref() {
            Some("regenerateAll") => "regenerateAll",
            Some("strictSource") => "strictSource",
            _ => "preserveValid",
        };
        let args = [
            "import".as_ref(),
            source.as_os_str(),
            handle.as_ref(),
            temp.path().as_os_str(),
            uv_mode.as_ref(),
        ];
        let mut data = execute(&app, &args, &handle, None, STALL_IMPORT);
        // worker 被静默 fastfail 终止（0xc0000409/0xc0000005）在特定环境下
        // 偶发且导入幂等：自动重试一次，间歇性崩溃不再直接阻塞用户。
        if data
            .as_ref()
            .err()
            .is_some_and(|message| message.contains("0xc0000409") || message.contains("0xc0000005"))
        {
            data = execute(&app, &args, &handle, None, STALL_IMPORT);
        }
        let mut data = data?;
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
fn validate_bake_options(options: &Value) -> Result<(), String> {
    // IndexMut 写入（options["jobId"] = …）对非对象值会 panic，而 release 是
    // panic = "abort"，一个畸形请求就能闪退整个应用，必须在入口拒绝。
    if options.is_object() {
        Ok(())
    } else {
        Err("无效的烘焙参数".into())
    }
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
        validate_bake_options(&options)?;
        if job_id.is_empty() || job_id.len() > 100 {
            return Err("任务编号无效".into());
        }
        let started = Instant::now();
        // AI 降噪：优先使用随安装包提供的组件，其次使用下载副本。
        if options["denoise"].as_bool().unwrap_or(false) && options["ao"].as_bool().unwrap_or(false)
        {
            let Some(bin) = resolve_oidn_dir(&app)? else {
                return Err("AI 降噪组件未下载，请先在烘焙设置中下载降噪组件。".into());
            };
            options["oidnDir"] = json!(bin.display().to_string());
        }
        let model_path = MODELS
            .lock()
            .map_err(|_| "模型状态锁损坏")?
            .get(&handle)
            .ok_or("模型句柄已失效，请重新导入")?
            .path()
            .join("model.json");
        // 每次任务独占目录。旧结果由结果句柄持有，直到前端完成原子切换后释放。
        let root = bake_cache_root(&app)?;
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        prune_bake_cache(&root);
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
        if let Ok(mut active) = ACTIVE_BAKE.lock() {
            *active = Some(job_id.clone());
        }
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
            STALL_BAKE,
        );
        JOBS.lock().map_err(|_| "任务状态锁损坏")?.remove(&job_id);
        if let Ok(mut active) = ACTIVE_BAKE.lock() {
            *active = None;
        }
        CANCEL_REQUESTS
            .lock()
            .map_err(|_| "任务状态锁损坏")?
            .remove(&job_id);
        let mut data = match response {
            Ok(data) => data,
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
                    for kind in [
                        "ao",
                        "normal",
                        "worldNormal",
                        "curvature",
                        "position",
                        "thickness",
                        "id",
                        "uv",
                    ] {
                        let output_kind = if kind == "worldNormal" {
                            "world_normal"
                        } else {
                            kind
                        };
                        if options[kind] == true
                            && !data["files"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .any(|f| f["material"] == *material && f["kind"] == output_kind)
                        {
                            unfinished.push(json!({"material":material,"kind":output_kind}));
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
                data
            }
        };
        let result_handle = id();
        RESULTS
            .lock()
            .map_err(|_| "结果状态锁损坏")?
            .insert(result_handle.clone(), output);
        data["resultHandle"] = json!(result_handle);
        Ok(data)
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
            // 第 4 个 argv 是 jobId：worker 统一用 args.get(3) 作为消息回执，
            // 缺省时回执为空串，execute 的过滤会把全部消息丢弃（检查 UV 100% 失败）。
            &[
                "inspect".as_ref(),
                temp.path().as_os_str(),
                "inspect".as_ref(),
            ],
            "inspect",
            None,
            STALL_QUICK,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
/// 全局停止按钮的烘焙桥接：有活动烘焙时向其写入取消文件。
/// 返回是否存在活动烘焙（供日志/测试判断）。
pub(crate) fn cancel_active_bake() -> bool {
    let job = ACTIVE_BAKE.lock().ok().and_then(|guard| guard.clone());
    match job {
        Some(job_id) => {
            let _ = bake_cancel(job_id);
            true
        }
        None => false,
    }
}

const OIDN_VERSION: &str = "2.2.2";
const OIDN_ZIP_NAME: &str = "oidn-2.2.2.x64.windows.zip";
const OIDN_ZIP_SIZE: u64 = 28_934_149;
const OIDN_ZIP_SHA256: &str = "5cc8bcc2a3321ef32547c3be70d43878a41324718cabdfb151b332a2a4928297";
const OIDN_MODEL_ID: &str = "oidn";
/// CPU 降噪所需文件（其余 cuda/hip/sycl 设备与基准工具不下载）。
const OIDN_REQUIRED: &[&str] = &[
    "OpenImageDenoise.dll",
    "OpenImageDenoise_core.dll",
    "OpenImageDenoise_device_cpu.dll",
    "tbb12.dll",
];
const OIDN_EXTRACT: &[&str] = &[
    "oidn-2.2.2.x64.windows/bin/OpenImageDenoise.dll",
    "oidn-2.2.2.x64.windows/bin/OpenImageDenoise_core.dll",
    "oidn-2.2.2.x64.windows/bin/OpenImageDenoise_device_cpu.dll",
    "oidn-2.2.2.x64.windows/bin/tbb12.dll",
    "oidn-2.2.2.x64.windows/doc/LICENSE.txt",
];
const OIDN_URLS: &[&str] = &[
    "https://ghfast.top/https://github.com/OpenImageDenoise/oidn/releases/download/v2.2.2/oidn-2.2.2.x64.windows.zip",
    "https://github.com/OpenImageDenoise/oidn/releases/download/v2.2.2/oidn-2.2.2.x64.windows.zip",
];

pub(crate) fn oidn_bin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("oidn").join("bin"))
        .map_err(|error| format!("无法定位应用数据目录：{error}"))
}

fn oidn_dir_complete(dir: &Path) -> bool {
    OIDN_REQUIRED.iter().all(|name| dir.join(name).is_file())
}

fn resolve_oidn_dir(app: &AppHandle) -> Result<Option<PathBuf>, String> {
    let bundled = app
        .path()
        .resource_dir()
        .map_err(|e| format!("无法定位资源目录：{e}"))?
        .join("bake")
        .join("oidn");
    if oidn_dir_complete(&bundled) {
        return Ok(Some(bundled));
    }
    let downloaded = oidn_bin_dir(app)?;
    Ok(oidn_dir_complete(&downloaded).then_some(downloaded))
}

pub(crate) fn sha256_of_file(path: &Path) -> Result<String, String> {
    // Windows 自带 certutil，避免为一次性校验引入哈希依赖。
    let output = crate::safety::quiet_command("certutil")
        .args(["-hashfile"])
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(|line| line.trim())
        .find(|line| line.len() == 64 && line.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|line| line.to_lowercase())
        .ok_or_else(|| "无法读取文件哈希".into())
}

fn progress_oidn(app: &AppHandle, completed: u64, total: u64, file: &str) {
    crate::anime::emit_progress(
        Some(app),
        crate::anime::ModelProgress {
            model_id: OIDN_MODEL_ID.into(),
            file: file.into(),
            completed,
            total,
        },
    );
}

#[tauri::command]
pub(crate) async fn oidn_status(app: AppHandle) -> Result<serde_json::Value, String> {
    let resolved = resolve_oidn_dir(&app)?;
    let downloaded = oidn_bin_dir(&app)?;
    let source = resolved.as_ref().map(|path| {
        if *path == downloaded {
            "downloaded"
        } else {
            "bundled"
        }
    });
    Ok(json!({ "installed": resolved.is_some(), "source": source, "version": OIDN_VERSION }))
}

#[tauri::command]
pub(crate) async fn oidn_install(app: AppHandle) -> Result<serde_json::Value, String> {
    // 单飞防护：并发双装在 target.exists() 与 rename 之间存在 TOCTOU，
    // 后到者白下 25MB 并报「安装失败」假错误。
    if OIDN_INSTALLING
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return Err("降噪组件正在安装中，请稍候。".into());
    }
    let _oidn_guard = OidnInstallGuard;
    oidn_install_inner(app).await
}

static OIDN_INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
struct OidnInstallGuard;
impl Drop for OidnInstallGuard {
    fn drop(&mut self) {
        OIDN_INSTALLING.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

async fn oidn_install_inner(app: AppHandle) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // 拿到单飞锁后重查：并发的另一个请求可能刚完成安装
        if let Some(path) = resolve_oidn_dir(&app)? {
            let downloaded = oidn_bin_dir(&app)?;
            let source = if path == downloaded {
                "downloaded"
            } else {
                "bundled"
            };
            return Ok(json!({ "installed": true, "source": source, "version": OIDN_VERSION }));
        }
        let tmp = tempfile::Builder::new()
            .prefix("aias-oidn-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let archive = tmp.path().join(OIDN_ZIP_NAME);
        let mut last_error = String::from("没有可用的下载地址");
        for url in OIDN_URLS {
            match crate::anime::curl_download(url, &archive, Some(OIDN_ZIP_SIZE), &|done, total| {
                progress_oidn(&app, done, total, OIDN_ZIP_NAME);
            }) {
                Ok(()) => {
                    last_error = String::new();
                    break;
                }
                Err(error) => last_error = error,
            }
        }
        if !last_error.is_empty() {
            return Err(format!("下载降噪组件失败：{last_error}"));
        }
        let actual = sha256_of_file(&archive)?;
        if actual != OIDN_ZIP_SHA256 {
            let _ = fs::remove_file(&archive);
            return Err("降噪组件校验失败（SHA256 不匹配），请重试下载。".into());
        }
        let extract_root = tmp.path().join("extract");
        std::fs::create_dir_all(&extract_root).map_err(|e| e.to_string())?;
        for entry in OIDN_EXTRACT {
            let output = crate::safety::quiet_command("tar")
                .arg("-xf")
                .arg(&archive)
                .arg("-C")
                .arg(&extract_root)
                .arg(entry)
                .output()
                .map_err(|error| format!("无法启动 tar 解压：{error}"))?;
            if !output.status.success() {
                return Err(format!(
                    "解压降噪组件失败：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        }
        let source_root = extract_root.join(format!("oidn-{OIDN_VERSION}.x64.windows"));
        let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&app_data).map_err(|e| e.to_string())?;
        let staging = app_data.join(format!("oidn-install-{}", id()));
        // staging 的任何一步失败都要清掉半成品，否则反复失败会在 AppData
        // 里累积一个个 ~25MB 的 oidn-install-* 目录。
        let staged = (|| -> Result<(), String> {
            let staging_bin = staging.join("bin");
            std::fs::create_dir_all(&staging_bin).map_err(|e| e.to_string())?;
            for name in OIDN_REQUIRED {
                fs::copy(source_root.join("bin").join(name), staging_bin.join(name))
                    .map_err(|e| format!("复制 {name} 失败：{e}"))?;
            }
            fs::copy(
                source_root.join("doc").join("LICENSE.txt"),
                staging.join("OIDN-LICENSE.txt"),
            )
            .map_err(|e| e.to_string())?;
            if !oidn_dir_complete(&staging_bin) {
                return Err("降噪组件文件不完整".into());
            }
            Ok(())
        })();
        if staged.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        staged?;
        let target = app_data.join("oidn");
        let backup = app_data.join(format!("oidn-backup-{}", id()));
        if target.exists() {
            fs::rename(&target, &backup).map_err(|e| format!("无法替换旧降噪组件：{e}"))?;
        }
        if let Err(error) = fs::rename(&staging, &target) {
            if backup.exists() {
                let _ = fs::rename(&backup, &target);
            }
            return Err(format!("安装降噪组件失败：{error}"));
        }
        let _ = fs::remove_dir_all(&backup);
        let _ = fs::remove_file(&archive);
        Ok(json!({ "installed": true, "source": "downloaded", "version": OIDN_VERSION }))
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

#[tauri::command]
pub async fn bake_result_release(result_handle: String) -> Result<(), String> {
    // bake_export 全程持有 RESULTS 锁（大结果复制可达数秒）：同步命令会在
    // 主线程阻塞等锁冻结 UI，等待必须发生在工作线程。
    tauri::async_runtime::spawn_blocking(move || {
        let path = RESULTS
            .lock()
            .map_err(|_| "结果状态锁损坏")?
            .remove(&result_handle);
        if let Some(path) = path {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(crate) fn bake_cache_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("bake-cache"))
        .map_err(|error| format!("无法定位应用数据目录：{error}"))
}

fn prune_bake_cache(root: &Path) {
    // 锁中毒时恢复而非按空保留集执行：宁可多留，也不能把活动结果目录误删。
    let retained: std::collections::HashSet<PathBuf> = RESULTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .cloned()
        .collect();
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(24 * 60 * 60))
        .unwrap_or(SystemTime::UNIX_EPOCH);
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        // RESULTS 存的是 bake_start 里 canonicalize 过的路径（\??\ 前缀），
        // read_dir 给的是普通路径：不归一化则保护集合永不命中，24h 后
        // 仍被前端持有的活动结果会被静默误删。
        let canonical = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if retained.contains(&canonical) || retained.contains(&path) {
            continue;
        }
        let stale = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .is_some_and(|modified| modified < cutoff);
        if stale {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub(crate) fn prune_stale_cache_on_startup(app: &AppHandle) {
    if let Ok(root) = bake_cache_root(app) {
        if root.is_dir() {
            prune_bake_cache(&root);
        }
    }
}

/// 把缓存目录内的贴图复制到用户选择的目标文件夹。源必须位于缓存目录内，
/// 防止任意路径读取；同名文件直接覆盖（重复导出同目录是常态）。
/// ID 贴图的颜色图例与贴图同目录生成；导出时自动附带，否则 ID 贴图
/// 离开缓存目录后无法解读颜色对应的材质。
fn with_legend(files: &[String]) -> Vec<String> {
    let mut all = files.to_vec();
    if let Some(first) = files.first() {
        if let Some(parent) = Path::new(first).parent() {
            let legend = parent.join("material-colors.json");
            if legend.is_file() && !all.iter().any(|f| Path::new(f) == legend) {
                all.push(legend.display().to_string());
            }
        }
    }
    all
}

fn export_files(
    cache_root: &Path,
    files: &[String],
    directory: &Path,
    policy: &str,
) -> Result<usize, String> {
    let root = std::fs::canonicalize(cache_root)
        .map_err(|error| format!("烘焙缓存目录不可用：{error}"))?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let destination = std::fs::canonicalize(directory).map_err(|e| e.to_string())?;
    if root.starts_with(&destination) || destination.starts_with(&root) {
        return Err(
            "导出目录不能位于烘焙缓存目录内，也不能是缓存目录的上级：不同任务的贴图会互相覆盖，请另选文件夹。"
                .into(),
        );
    }
    let mut exported = 0usize;
    for file in files {
        let source = std::fs::canonicalize(file)
            .map_err(|error| format!("贴图文件不存在：{}（{error}）", file))?;
        if !source.starts_with(&root) {
            return Err(format!("拒绝导出缓存目录之外的文件：{}", source.display()));
        }
        let name = source
            .file_name()
            .ok_or_else(|| format!("无效的文件名：{}", source.display()))?;
        std::fs::copy(
            &source,
            crate::safety::conflict_free(directory.join(name), policy),
        )
        .map_err(|error| format!("导出 {} 失败：{error}", name.to_string_lossy()))?;
        exported += 1;
    }
    Ok(exported)
}

#[tauri::command]
pub async fn bake_export(
    app: AppHandle,
    files: Option<Vec<String>>,
    result_handle: Option<String>,
    directory: String,
) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if directory.trim().is_empty() {
            return Err("请选择导出目标文件夹。".into());
        }
        // 结果目录彼此独立，导出可以和新烘焙并行。句柄锁保留到复制完成，
        // 避免前端在新结果切换时释放正在导出的旧目录。
        let retained_results = result_handle
            .as_ref()
            .map(|_| RESULTS.lock().map_err(|_| "结果状态锁损坏"))
            .transpose()?;
        let files = if let Some(handle) = result_handle {
            let result_dir = retained_results
                .as_ref()
                .and_then(|results| results.get(&handle))
                .cloned()
                .ok_or("烘焙结果已释放，请重新烘焙")?;
            let result: Value = serde_json::from_reader(std::io::BufReader::new(
                std::fs::File::open(result_dir.join("result.json")).map_err(|e| e.to_string())?,
            ))
            .map_err(|e| e.to_string())?;
            result["files"]
                .as_array()
                .into_iter()
                .flatten()
                .chain(result["artifacts"].as_array().into_iter().flatten())
                .filter_map(|item| item["path"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        } else {
            files.unwrap_or_default()
        };
        if files.is_empty() {
            return Err("没有可导出的贴图。".into());
        }
        let root = bake_cache_root(&app)?;
        let files = with_legend(&files);
        let policy = crate::conflict_policy(&app);
        let exported = export_files(&root, &files, Path::new(&directory), &policy)?;
        Ok(json!({ "exported": exported, "directory": directory }))
    })
    .await
    .map_err(|e| e.to_string())?
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bake_options_must_be_an_object() {
        assert!(validate_bake_options(&json!("x")).is_err());
        assert!(validate_bake_options(&json!([])).is_err());
        assert!(validate_bake_options(&json!(null)).is_err());
        assert!(validate_bake_options(&json!(42)).is_err());
        assert!(validate_bake_options(&json!(true)).is_err());
        assert!(validate_bake_options(&json!({})).is_ok());
        assert!(validate_bake_options(&json!({"output": "C:/tmp", "ao": true})).is_ok());
    }

    #[test]
    fn export_copies_cache_files_and_rejects_outside_sources() {
        let cache = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let a = cache.path().join("1_ao.png");
        let b = cache.path().join("1_uv.png");
        std::fs::write(&a, b"ao").unwrap();
        std::fs::write(&b, b"uv").unwrap();
        let outside = cache.path().parent().unwrap().join("outside.png");
        std::fs::write(&outside, b"x").unwrap();

        let exported = export_files(
            cache.path(),
            &[a.display().to_string(), b.display().to_string()],
            output.path(),
            "overwrite",
        )
        .unwrap();
        assert_eq!(exported, 2);
        assert_eq!(
            std::fs::read(output.path().join("1_ao.png")).unwrap(),
            b"ao"
        );
        assert_eq!(
            std::fs::read(output.path().join("1_uv.png")).unwrap(),
            b"uv"
        );

        let rejected = export_files(
            cache.path(),
            &[outside.display().to_string()],
            output.path(),
            "overwrite",
        );
        assert!(rejected.unwrap_err().contains("缓存目录之外"));
        let cache_child = cache.path().join("manual-export");
        let rejected_destination = export_files(
            cache.path(),
            &[a.display().to_string()],
            &cache_child,
            "overwrite",
        );
        assert!(rejected_destination
            .unwrap_err()
            .contains("不能位于烘焙缓存目录内"));
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn with_legend_appends_material_colors_from_same_directory() {
        let cache = tempfile::tempdir().unwrap();
        let png = cache.path().join("3_ao.png");
        let legend = cache.path().join("material-colors.json");
        std::fs::write(&png, b"png").unwrap();
        std::fs::write(&legend, b"[]").unwrap();
        let files = vec![png.display().to_string()];
        let with = with_legend(&files);
        assert_eq!(with.len(), 2, "legend in same directory is appended");
        assert!(with.last().unwrap().ends_with("material-colors.json"));

        let no_legend_dir = tempfile::tempdir().unwrap();
        let lone = no_legend_dir.path().join("4_ao.png");
        std::fs::write(&lone, b"png").unwrap();
        assert_eq!(with_legend(&[lone.display().to_string()]).len(), 1);
    }

    #[test]
    fn export_overwrites_same_named_targets() {
        let cache = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let file = cache.path().join("2_id.png");
        std::fs::write(&file, b"first").unwrap();
        export_files(
            cache.path(),
            &[file.display().to_string()],
            output.path(),
            "overwrite",
        )
        .unwrap();
        std::fs::write(&file, b"second").unwrap();
        export_files(
            cache.path(),
            &[file.display().to_string()],
            output.path(),
            "overwrite",
        )
        .unwrap();
        assert_eq!(
            std::fs::read(output.path().join("2_id.png")).unwrap(),
            b"second"
        );
    }

    #[test]
    fn oidn_requires_the_complete_runtime_set() {
        let directory = tempfile::tempdir().unwrap();
        assert!(!oidn_dir_complete(directory.path()));
        for name in OIDN_REQUIRED {
            std::fs::write(directory.path().join(name), b"dll").unwrap();
        }
        assert!(oidn_dir_complete(directory.path()));
        std::fs::remove_file(directory.path().join("tbb12.dll")).unwrap();
        assert!(!oidn_dir_complete(directory.path()));
    }
}

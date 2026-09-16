mod bake;
mod denoise;
mod gpu;
mod model;
#[cfg(test)]
mod tests;

// ---------------------------------------------------------------------------
// 崩溃自诊断：native 静默 abort（0xc0000409，stderr 无输出）无法从宿主侧
// 定位。这里挂四类钩子，把现场写到 %TEMP%ias-worker-crash\：
//   1. 未处理异常（含 fastfail 之外的访问违例等）→ 自写 minidump
//   2. abort/SIGABRT → 强制捕获 Rust 回溯
//   3. UCRT 非法参数 → 记录后继续走默认终止
//   4. 纯虚调用 → 同上
#[cfg(windows)]
fn install_crash_diagnostics() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ENTERED: AtomicBool = AtomicBool::new(false);

    fn crash_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("aias-worker-crash");
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    unsafe extern "C" fn on_sigabort(sig: i32) {
        if ENTERED.swap(true, Ordering::SeqCst) {
            std::process::abort();
        }
        let bt = std::backtrace::Backtrace::force_capture();
        let text = format!(
            "SIGABRT (signal {sig})
pid={}
backtrace:
{}
",
            std::process::id(),
            bt
        );
        let _ = std::fs::write(crash_dir().join(format!("abort-{}.log", std::process::id())), text);
        // 还原默认处理，继续正常 abort 流程
        unsafe extern "C" {
            fn signal(sig: i32, handler: usize) -> usize;
        }
        unsafe { signal(sig, 0) };
    }

    unsafe extern "C" fn on_invalid_parameter(
        expr: *const u16,
        _func: *const u16,
        _file: *const u16,
        _line: u32,
        _reserved: usize,
    ) {
        let expr_text = if expr.is_null() {
            String::new()
        } else {
            let mut end = 0usize;
            unsafe {
                while *expr.add(end) != 0 {
                    end += 1;
                }
            }
            String::from_utf16_lossy(std::slice::from_raw_parts(expr, end))
        };
        let _ = std::fs::write(
            crash_dir().join(format!("invalid-param-{}.log", std::process::id())),
            format!("UCRT 非法参数：{expr_text}
backtrace:
{}", std::backtrace::Backtrace::force_capture()),
        );
    }

    unsafe extern "C" fn on_purecall() {
        let _ = std::fs::write(
            crash_dir().join(format!("purecall-{}.log", std::process::id())),
            format!("纯虚调用
backtrace:
{}", std::backtrace::Backtrace::force_capture()),
        );
    }

    unsafe extern "system" fn unhandled_filter(
        info: *const core::ffi::c_void,
    ) -> i32 {
        if ENTERED.swap(true, Ordering::SeqCst) {
            return 0; // EXCEPTION_CONTINUE_SEARCH
        }
        // dbghelp!MiniDumpWriteDump 自写带线程栈的 dump（MiniDumpNormal |
        // WithThreadInfo | WithIndirectlyReferencedMemory），足够还原调用栈。
        let dir = crash_dir();
        let dump_path = dir.join(format!("crash-{}.dmp", std::process::id()));
        if let Ok(lib) = unsafe { libloading::Library::new("dbghelp.dll") } {
            type MiniDumpWriteDump = unsafe extern "system" fn(
                *mut core::ffi::c_void, // 进程句柄
                u32,                    // 进程 id
                *mut core::ffi::c_void, // 文件句柄
                u32,                    // dump 类型
                *const core::ffi::c_void, // 异常参数
                *const core::ffi::c_void, // 用户流
                *const core::ffi::c_void, // 回调
            ) -> i32;
            if let Ok(dump) = unsafe { lib.get::<MiniDumpWriteDump>(b"MiniDumpWriteDump") } {
                if let Ok(file) = std::fs::File::create(&dump_path) {
                    use std::os::windows::io::AsRawHandle;
                    let handle = file.as_raw_handle() as *mut core::ffi::c_void;
                    const MINI_DUMP_NORMAL: u32 = 0;
                    const MINI_DUMP_WITH_THREAD_INFO: u32 = 0x1000;
                    const MINI_DUMP_WITH_INDIRECTLY_REFERENCED_MEMORY: u32 = 0x4;
                    unsafe {
                        dump(
                            handle,
                            std::process::id(),
                            info as *mut core::ffi::c_void,
                            MINI_DUMP_NORMAL
                                | MINI_DUMP_WITH_THREAD_INFO
                                | MINI_DUMP_WITH_INDIRECTLY_REFERENCED_MEMORY,
                            std::ptr::null(),
                            std::ptr::null(),
                            std::ptr::null(),
                        );
                    }
                }
            }
        }
        // EXCEPTION_POINTERS 偏移 0 是 ExceptionRecord 指针，其偏移 0 才是码
        let code = unsafe {
            let record = *(info as *const *const u32);
            if record.is_null() {
                0u32
            } else {
                *record
            }
        } as u64;
        let _ = std::fs::write(
            dir.join(format!("unhandled-{}.log", std::process::id())),
            format!("未处理异常 code={code:#x} dump={}
", dump_path.display()),
        );
        0 // EXCEPTION_CONTINUE_SEARCH：交给 WER 继续收尾
    }

    unsafe {
        unsafe extern "C" {
            fn SetUnhandledExceptionFilter(f: *const core::ffi::c_void) -> *const core::ffi::c_void;
            fn signal(sig: i32, handler: usize) -> usize;
            fn _set_invalid_parameter_handler(h: *const core::ffi::c_void) -> *const core::ffi::c_void;
            fn _set_purecall_handler(h: *const core::ffi::c_void) -> *const core::ffi::c_void;
        }
        SetUnhandledExceptionFilter(unhandled_filter as *const core::ffi::c_void);
        signal(22, on_sigabort as *const core::ffi::c_void as usize);
        _set_invalid_parameter_handler(on_invalid_parameter as *const core::ffi::c_void);
        _set_purecall_handler(on_purecall as *const core::ffi::c_void);
    }
}

fn main() {
    #[cfg(windows)]
    install_crash_diagnostics();
    if std::env::args().any(|s| s == "--gpu-smoke") {
        let result = (|| -> Result<(), String> {
            let vertices = [
                [-2., 0., -2.],
                [2., 0., -2.],
                [0., 0., 2.],
                [-2., 0.5, -2.],
                [0., 0.5, 2.],
                [2., 0.5, -2.],
            ];
            let mut gpu = gpu::Gpu::new(0, &vertices, &[0, 1], false)?;
            let surfaces = [gpu::Surface {
                position: [0., 0., 0.],
                normal: [0., 1., 0.],
                object: 0,
                pixel: 0,
            }];
            let mutual = gpu.trace(&surfaces, 128, 2., 0.0001, false, || false)?;
            let self_only = gpu.trace(&surfaces, 128, 2., 0.0001, true, || false)?;
            println!(
                "{}",
                serde_json::json!({"gpuMutualHits":mutual,"gpuSelfHits":self_only})
            );
            if mutual[0] < 32 || self_only[0] != 0 {
                return Err("GPU smoke result failed".into());
            }
            Ok(())
        })();
        if let Err(e) = result {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return;
    }
    let args: Vec<_> = std::env::args().collect();
    if args.len() >= 3 {
        let job = args.get(3).cloned().unwrap_or_default();
        let result = (|| -> Result<serde_json::Value, String> {
            match args[1].as_str() {
                "inspect" => {
                    let request: serde_json::Value = serde_json::from_reader(
                        std::fs::File::open(&args[2]).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    let model: model::Model = serde_json::from_reader(std::io::BufReader::new(
                        std::fs::File::open(request["modelPath"].as_str().ok_or("缺失模型路径")?)
                            .map_err(|e| e.to_string())?,
                    ))
                    .map_err(|e| e.to_string())?;
                    let objects: Vec<usize> = serde_json::from_value(request["objects"].clone())
                        .map_err(|e| e.to_string())?;
                    let channels: std::collections::BTreeMap<usize, u32> =
                        serde_json::from_value(request["channels"].clone())
                            .map_err(|e| e.to_string())?;
                    let reports: Vec<_> = channels
                        .into_iter()
                        .map(|(m, c)| model::inspect(&model, m, c, &objects))
                        .collect();
                    serde_json::to_value(reports).map_err(|e| e.to_string())
                }
                "import" => {
                    // model::load 解析原始 OBJ/GLB，在首条进度事件（0.55）之前
                    // 可能静默数分钟；期间由心跳行喂宿主的 stall 看门狗，防止
                    // 大模型导入被误杀。心跳上限 30 分钟，超过视为真挂死，
                    // 停止心跳交还看门狗终止。
                    let heartbeat_done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let heartbeat_flag = heartbeat_done.clone();
                    let heartbeat_job = job.clone();
                    let heartbeat = std::thread::spawn(move || {
                        let mut elapsed = 0u64;
                        while !heartbeat_flag.load(std::sync::atomic::Ordering::Relaxed) {
                            std::thread::sleep(std::time::Duration::from_secs(15));
                            if heartbeat_flag.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                            elapsed += 15;
                            if elapsed > 1800 {
                                break;
                            }
                            println!(
                                "{}",
                                serde_json::json!({"jobId":heartbeat_job,"type":"progress","data":{"phase":"读取模型几何…","progress":0.05}})
                            );
                        }
                    });
                    let imported = (|| -> Result<serde_json::Value, String> {
                    println!(
                        "{}",
                        serde_json::json!({"jobId":job,"type":"progress","data":{"phase":"读取模型","progress":0.02}})
                    );
                    let mut model = model::load(std::path::Path::new(&args[2]))?;
                    let dir = std::path::Path::new(args.get(4).ok_or("缺失输出目录")?);
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    let uv_mode = args.get(5).map(String::as_str).unwrap_or("preserveValid");
                    println!(
                        "{}",
                        serde_json::json!({"jobId":job,"type":"progress","data":{"phase":"检查并准备 UV","progress":0.55}})
                    );
                    let selected_channels = model::prepare_uvs_with_progress(
                        &mut model,
                        uv_mode,
                        |position, total| {
                            println!(
                                "{}",
                                serde_json::json!({"jobId":job,"type":"progress","data":{"phase":format!("检查并准备 UV · {}/{}",position+1,total),"progress":0.55 + 0.25 * position as f64 / total.max(1) as f64}})
                            );
                        },
                    )?;
                    bake::atomic_json(&dir.join("model.json"), &model)?;
                    println!(
                        "{}",
                        serde_json::json!({"jobId":job,"type":"progress","data":{"phase":"生成三维预览","progress":0.82}})
                    );
                    let preview = model::write_preview(&model, dir)?;
                    let objects: Vec<_> = model.objects.iter().map(|o| o.id).collect();
                    let materials:Vec<_>=model.materials.iter().filter(|m|model.triangles.iter().any(|t|t.material==m.id)).map(|m|{
                    let mut channels=std::collections::BTreeSet::new();for t in model.triangles.iter().filter(|t|t.material==m.id){channels.extend(t.uvs.keys().copied());}channels.insert(0);
                    let generated = model.generated_channels.get(&m.id).copied();
                    let reports = channels.iter().map(|c| { let mut value=serde_json::to_value(model::inspect(&model,m.id,*c,&objects)).unwrap(); value["generated"]=serde_json::json!(generated==Some(*c)); value }).collect::<Vec<_>>();
                    serde_json::json!({"id":m.id,"name":m.name,"channels":reports,"selectedChannel":selected_channels.get(&m.id).copied().unwrap_or(0)})
                }).collect();
                    Ok(
                        serde_json::json!({"name":model.name,"objects":model.objects,"materials":materials,"bounds":model.bounds,"units":model.units,"meshPath":dir.join("model.json"),"preview":preview,"triangleCount":model.triangles.len(),"degenerateFaces":model.degenerate_faces,"degenerateExamples":model.degenerate_examples,"warnings":model.warnings,"sourceFormat":model.source_format,"generatedChannels":model.generated_channels}),
                    )
                    })();
                    heartbeat_done.store(true, std::sync::atomic::Ordering::Relaxed);
                    let _ = heartbeat.join();
                    imported
                }
                "bake" => {
                    let options: bake::Options = serde_json::from_reader(
                        std::fs::File::open(&args[2]).map_err(|e| e.to_string())?,
                    )
                    .map_err(|e| e.to_string())?;
                    let result = bake::run(&options, |data| {
                        println!(
                            "{}",
                            serde_json::json!({"jobId":job,"type":"progress","data":data})
                        )
                    })?;
                    serde_json::to_value(result).map_err(|e| e.to_string())
                }
                _ => Err("未知工作进程命令".into()),
            }
        })();
        match result {
            Ok(data) => println!(
                "{}",
                serde_json::json!({"jobId":job,"type":"result","data":data})
            ),
            Err(error) => {
                println!(
                    "{}",
                    serde_json::json!({"jobId":job,"type":"error","error":error})
                );
                std::process::exit(1);
            }
        }
        return;
    }
    match gpu::capabilities() {
        Ok(devices) => println!("{}", serde_json::json!({"devices":devices})),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

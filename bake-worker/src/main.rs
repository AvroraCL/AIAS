mod bake;
mod denoise;
mod gpu;
mod model;
#[cfg(test)]
mod tests;
fn main() {
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
                                serde_json::json!({"jobId":heartbeat_job,"type":"heartbeat"})
                            );
                        }
                    });
                    let imported = (|| -> Result<serde_json::Value, String> {
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

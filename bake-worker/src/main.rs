mod bake;
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
            let mut gpu = gpu::Gpu::new(0, &vertices, &[0, 1])?;
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
                    let model: model::Model = serde_json::from_reader(
                        std::fs::File::open(request["modelPath"].as_str().ok_or("缺失模型路径")?)
                            .map_err(|e| e.to_string())?,
                    )
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
                    let model = model::load(std::path::Path::new(&args[2]))?;
                    let dir = std::path::Path::new(args.get(4).ok_or("缺失输出目录")?);
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                    bake::atomic_json(&dir.join("model.json"), &model)?;
                    let objects: Vec<_> = model.objects.iter().map(|o| o.id).collect();
                    let materials:Vec<_>=model.materials.iter().filter(|m|model.triangles.iter().any(|t|t.material==m.id)).map(|m|{
                    let mut channels=std::collections::BTreeSet::new();for t in model.triangles.iter().filter(|t|t.material==m.id){channels.extend(t.uvs.keys().copied());}channels.insert(0);
                    serde_json::json!({"id":m.id,"name":m.name,"channels":channels.iter().map(|c|model::inspect(&model,m.id,*c,&objects)).collect::<Vec<_>>()})
                }).collect();
                    Ok(
                        serde_json::json!({"name":model.name,"objects":model.objects,"materials":materials,"bounds":model.bounds,"units":model.units,"meshPath":dir.join("model.json"),"triangleCount":model.triangles.len()}),
                    )
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

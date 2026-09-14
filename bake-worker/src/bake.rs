use crate::{
    gpu::{Gpu, Surface},
    model::{inspect, Model},
};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    pub job_id: String,
    pub model_path: PathBuf,
    pub output: PathBuf,
    pub cancel_path: PathBuf,
    pub device: u32,
    pub objects: Vec<usize>,
    pub materials: Vec<usize>,
    pub channels: BTreeMap<usize, u32>,
    pub resolution: u32,
    pub samples: u32,
    pub distance: f32,
    pub margin: u32,
    pub self_only: bool,
    pub ao: bool,
    pub uv: bool,
    pub id: bool,
    pub bits: u8,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResultSet {
    pub peak_device_bytes: u64,
    pub job_id: String,
    pub directory: PathBuf,
    pub files: Vec<Output>,
    pub failures: Vec<String>,
    pub cancelled: bool,
    pub elapsed_ms: u128,
}
#[derive(Serialize, Deserialize)]
pub struct Output {
    pub material: usize,
    pub kind: String,
    pub path: PathBuf,
}
/// 清洗非法字符、截断并去掉首尾空格/点；空结果返回空串（调用方决定回退名）。
fn clean_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    s.trim_matches([' ', '.']).to_string()
}

/// 模型名清洗（空回退 "model"）；生产路径的材质命名走 material_stems，
/// 此函数仅供测试断言历史契约。
#[cfg(test)]
pub fn safe_name(name: &str) -> String {
    let s = clean_name(name);
    if s.is_empty() {
        "model".into()
    } else {
        s
    }
}

/// SP 风格的输出命名：按材质名分文件，不带模型名与对象信息。
/// 重名材质（清洗后同名）整批加 `_{id}` 区分，依据全部材质统计，不随勾选漂移。
pub fn material_stems(materials: &[crate::model::Named]) -> Vec<String> {
    let names: Vec<String> = materials
        .iter()
        .map(|material| clean_name(&material.name))
        .collect();
    materials
        .iter()
        .enumerate()
        .map(|(id, _)| {
            let name = &names[id];
            if name.is_empty() {
                return format!("material_{id}");
            }
            if names.iter().filter(|other| *other == name).count() > 1 {
                format!("{name}_{id}")
            } else {
                name.clone()
            }
        })
        .collect()
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut f = tempfile::NamedTempFile::new_in(path.parent().ok_or("缺失父目录")?)
        .map_err(|e| e.to_string())?;
    // serde_json 逐 token 写；未缓冲时每个 token 一次 WriteFile，大模型的
    // model.json 会产生千万级系统调用（实测 103MB 的导入被拖到几十秒）。
    {
        let mut writer = std::io::BufWriter::new(f.as_file_mut());
        serde_json::to_writer(&mut writer, value).map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    f.as_file().sync_all().map_err(|e| e.to_string())?;
    f.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
fn save(path: &Path, image: image::DynamicImage) -> Result<(), String> {
    use image::ImageEncoder as _;
    let mut f = tempfile::NamedTempFile::new_in(path.parent().ok_or("缺失父目录")?)
        .map_err(|e| e.to_string())?;
    {
        let writer = std::io::BufWriter::new(f.as_file_mut());
        // 默认 zlib-6 编码在批量烘焙里占大头（4K 一批可达分钟级）；改用与
        // 主应用一致的 fdeflate 快速档，视觉无损、体积略增。
        image::codecs::png::PngEncoder::new_with_quality(
            writer,
            image::codecs::png::CompressionType::Fast,
            image::codecs::png::FilterType::Adaptive,
        )
        .write_image(
            image.as_bytes(),
            image.width(),
            image.height(),
            image.color().into(),
        )
        .map_err(|e| e.to_string())?;
    }
    f.as_file().sync_all().map_err(|e| e.to_string())?;
    f.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
pub fn color(material: usize) -> [u8; 4] {
    let value = (material as u32 + 1).wrapping_mul(0x9e3779) & 0xffffff;
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}
pub fn run(
    options: &Options,
    mut progress: impl FnMut(serde_json::Value),
) -> Result<ResultSet, String> {
    let start = Instant::now();
    if ![512, 1024, 2048, 4096].contains(&options.resolution)
        || ![32, 64, 128, 256].contains(&options.samples)
        || ![8, 16].contains(&options.bits)
        || options.margin > 128
        || !options.distance.is_finite()
        || options.distance <= 0.
    {
        return Err("烘焙参数不合法".into());
    }
    if options.objects.is_empty()
        || options.materials.is_empty()
        || !(options.ao || options.uv || options.id)
    {
        return Err("请选择对象、材质和输出类型".into());
    }
    let file = std::fs::File::open(&options.model_path).map_err(|e| e.to_string())?;
    let model: Model =
        serde_json::from_reader(std::io::BufReader::new(file)).map_err(|e| e.to_string())?;
    if options.objects.iter().any(|i| *i >= model.objects.len())
        || options
            .materials
            .iter()
            .any(|i| *i >= model.materials.len())
    {
        return Err("对象或材质编号无效".into());
    }
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let required = (options.resolution as u64).pow(2) * 96 + (model.triangles.len() as u64) * 1024;
    if required > system.available_memory() * 7 / 10 {
        return Err(format!(
            "系统内存不足：预计需要 {} MiB，可用 {} MiB",
            required / 1048576,
            system.available_memory() / 1048576
        ));
    }
    std::fs::create_dir_all(&options.output).map_err(|e| e.to_string())?;
    let mut result = ResultSet {
        job_id: options.job_id.clone(),
        directory: options.output.clone(),
        ..Default::default()
    };
    // GPU 加速结构惰性构建：坏 UV 模型会在逐材质校验里提前失败，不应白付
    // DXR 初始化与 BLAS 构建（大模型数秒到十几秒），错误信息也不该被
    // 显存类问题抢先。
    let mut gpu: Option<Gpu> = None;
    let stems = material_stems(&model.materials);
    for (position, material) in options.materials.iter().copied().enumerate() {
        if options.cancel_path.exists() {
            result.cancelled = true;
            break;
        }
        let channel = options.channels.get(&material).copied().unwrap_or(0);
        let report = inspect(&model, material, channel, &options.objects);
        let prefix = stems[material].clone();
        let attempt = (|| -> Result<(), String> {
            let (surfaces, covered, wire) = raster(
                &model,
                material,
                channel,
                &options.objects,
                options.resolution,
                options.uv,
            )?;
            if options.uv {
                let path = options.output.join(format!("{prefix}_uv.png"));
                save(
                    &path,
                    image::DynamicImage::ImageRgba8(
                        image::RgbaImage::from_raw(options.resolution, options.resolution, wire)
                            .ok_or("UV 图像尺寸错误")?,
                    ),
                )?;
                result.files.push(Output {
                    material,
                    kind: "uv".into(),
                    path,
                });
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            if !report.valid && (options.ao || options.id) {
                return Err(format!(
                    "UV 校验失败（{} 处）：{}",
                    report.issue_count,
                    report
                        .issues
                        .iter()
                        .take(3)
                        .map(|i| format!("对象 {} 面 {} {}", i.object, i.face, i.kind))
                        .collect::<Vec<_>>()
                        .join("；")
                ));
            }
            if surfaces.is_empty() && (options.ao || options.id) {
                return Err("所选材质没有像素覆盖".into());
            }
            let nearest = if options.ao || options.id {
                dilate(&covered, options.resolution as usize, options.margin)
            } else {
                vec![]
            };
            if options.id {
                let c = color(material);
                let mut pixels = vec![0; covered.len() * 4];
                for (i, source) in nearest.iter().enumerate() {
                    if *source != u32::MAX {
                        pixels[i * 4..i * 4 + 4].copy_from_slice(&c);
                    }
                }
                let path = options.output.join(format!("{prefix}_id.png"));
                save(
                    &path,
                    image::DynamicImage::ImageRgba8(
                        image::RgbaImage::from_raw(options.resolution, options.resolution, pixels)
                            .ok_or("ID 图像尺寸错误")?,
                    ),
                )?;
                result.files.push(Output {
                    material,
                    kind: "id".into(),
                    path,
                });
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            if options.ao {
                if gpu.is_none() {
                    progress(serde_json::json!({"phase":"构建 GPU 加速结构","progress":0}));
                    let selected: Vec<_> = model
                        .triangles
                        .iter()
                        .filter(|t| options.objects.contains(&t.object))
                        .collect();
                    let vertices: Vec<_> = selected.iter().flat_map(|t| t.positions).collect();
                    let objects: Vec<_> = selected.iter().map(|t| t.object as u32).collect();
                    gpu = Some(Gpu::new(options.device, &vertices, &objects)?);
                }
                let gpu = gpu.as_mut().ok_or("GPU 加速结构未初始化")?;
                let mut values = vec![1f32; covered.len()];
                let block = gpu.block_size;
                let diagonal = (Vec3::from_array(model.bounds[1])
                    - Vec3::from_array(model.bounds[0]))
                .length();
                let bias = (diagonal * 1e-5).max(1e-7).min(options.distance * 0.01);
                // 进度事件按"进展 ≥1% 或距上次 ≥100ms"节流：4096² 时 chunk 数
                // 可达上万，逐条跨进程→Rust→IPC→WebView 四跳纯属空耗。
                let mut last_emit = Instant::now();
                let mut last_percent = -1i64;
                for (chunk_index, chunk) in surfaces.chunks(block).enumerate() {
                    let hits = gpu.trace(
                        chunk,
                        options.samples,
                        options.distance,
                        bias,
                        options.self_only,
                        || options.cancel_path.exists(),
                    )?;
                    result.peak_device_bytes = result.peak_device_bytes.max(gpu.peak_device_bytes);
                    for (surface, hits) in chunk.iter().zip(hits) {
                        values[surface.pixel as usize] = 1. - hits as f32 / options.samples as f32;
                    }
                    let done = (chunk_index * block + chunk.len()) as f64 / surfaces.len() as f64;
                    let percent = (done * 100.0) as i64;
                    if percent != last_percent && last_emit.elapsed().as_millis() >= 100 {
                        last_emit = Instant::now();
                        last_percent = percent;
                        progress(
                            serde_json::json!({"phase":format!("材质 {} · GPU AO",material),"material":material,"progress":(position as f64+done)/options.materials.len() as f64,"blockSize":block}),
                        );
                    }
                }
                progress(
                    serde_json::json!({"phase":format!("材质 {} · GPU AO",material),"material":material,"progress":(position as f64+1.0)/options.materials.len() as f64,"blockSize":block}),
                );
                let path = options.output.join(format!("{prefix}_ao.png"));
                if options.bits == 16 {
                    let data: Vec<u16> = nearest
                        .iter()
                        .map(|s| {
                            let value = if *s == u32::MAX { 1. } else { values[*s as usize] };
                            (value * 65535.).round() as u16
                        })
                        .collect();
                    save(
                        &path,
                        image::DynamicImage::ImageLuma16(
                            image::ImageBuffer::from_raw(
                                options.resolution,
                                options.resolution,
                                data,
                            )
                            .ok_or("AO 尺寸错误")?,
                        ),
                    )?;
                } else {
                    let data: Vec<u8> = nearest
                        .iter()
                        .map(|s| {
                            let value = if *s == u32::MAX { 1. } else { values[*s as usize] };
                            (value * 255.).round() as u8
                        })
                        .collect();
                    save(
                        &path,
                        image::DynamicImage::ImageLuma8(
                            image::GrayImage::from_raw(
                                options.resolution,
                                options.resolution,
                                data,
                            )
                            .ok_or("AO 尺寸错误")?,
                        ),
                    )?;
                }
                result.files.push(Output {
                    material,
                    kind: "ao".into(),
                    path,
                });
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            Ok(())
        })();
        if let Err(e) = attempt {
            result.failures.push(format!("材质 {material}：{e}"));
            if options.cancel_path.exists() {
                result.cancelled = true;
            }
        }
        result.elapsed_ms = start.elapsed().as_millis();
        atomic_json(&options.output.join("result.json"), &result)?;
        if result.cancelled {
            break;
        }
    }
    if options.id {
        let legend:Vec<_>=options.materials.iter().map(|i|serde_json::json!({"material":i,"name":model.materials[*i].name,"rgba":color(*i)})).collect();
        atomic_json(&options.output.join("material-colors.json"), &legend)?;
    }
    if result.cancelled {
        let complete: std::collections::HashSet<_> = result
            .files
            .iter()
            .filter(|f| {
                f.kind
                    == if options.ao {
                        "ao"
                    } else if options.id {
                        "id"
                    } else {
                        "uv"
                    }
            })
            .map(|f| f.material)
            .collect();
        for m in &options.materials {
            if !complete.contains(m) {
                result.failures.push(format!("材质 {m} 未完成（取消）"));
            }
        }
    }
    result.elapsed_ms = start.elapsed().as_millis();
    atomic_json(&options.output.join("result.json"), &result)?;
    Ok(result)
}
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}
pub fn raster(
    model: &Model,
    material: usize,
    channel: u32,
    objects: &[usize],
    size: u32,
    draw_wire: bool,
) -> Result<(Vec<Surface>, Vec<bool>, Vec<u8>), String> {
    let n = size as usize;
    let mut covered = vec![false; n * n];
    let mut surfaces = vec![];
    let mut wire = if draw_wire {
        vec![0; n * n * 4]
    } else {
        vec![]
    };
    for t in model
        .triangles
        .iter()
        .filter(|t| t.material == material && objects.contains(&t.object))
    {
        let Some(uv) = t.uvs.get(&channel) else {
            continue;
        };
        if uv.iter().flatten().any(|v| !v.is_finite()) {
            continue;
        }
        let uv = uv.map(|p| Vec2::new(p[0] * size as f32, (1. - p[1]) * size as f32));
        if draw_wire {
            for edge in 0..3 {
                line(&mut wire, n, uv[edge], uv[(edge + 1) % 3]);
            }
        }
        let det = cross(uv[1] - uv[0], uv[2] - uv[0]);
        if det.abs() < 1e-10 {
            continue;
        }
        let min = uv
            .iter()
            .fold(Vec2::splat(f32::INFINITY), |a, b| a.min(*b))
            .floor()
            .max(Vec2::ZERO);
        let max = uv
            .iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), |a, b| a.max(*b))
            .ceil()
            .min(Vec2::splat(size as f32));
        for y in min.y as usize..max.y as usize {
            for x in min.x as usize..max.x as usize {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w1 = cross(p - uv[0], uv[2] - uv[0]) / det;
                let w2 = cross(uv[1] - uv[0], p - uv[0]) / det;
                let w0 = 1. - w1 - w2;
                if w0 < -1e-6 || w1 < -1e-6 || w2 < -1e-6 {
                    continue;
                }
                let pixel = y * n + x;
                if covered[pixel] {
                    continue;
                }
                covered[pixel] = true;
                let weights = [w0, w1, w2];
                let mut position = Vec3::ZERO;
                let mut normal = Vec3::ZERO;
                for k in 0..3 {
                    position += Vec3::from_array(t.positions[k]) * weights[k];
                    normal += Vec3::from_array(t.normals[k]) * weights[k];
                }
                surfaces.push(Surface {
                    position: position.to_array(),
                    normal: normal.normalize_or_zero().to_array(),
                    object: t.object as u32,
                    pixel: pixel as u32,
                });
            }
        }
    }
    Ok((surfaces, covered, wire))
}
fn line(pixels: &mut [u8], n: usize, a: Vec2, b: Vec2) {
    // Clip before stepping, so extreme out-of-range UVs cannot cause unbounded work.
    let d = b - a;
    let mut lo = 0f32;
    let mut hi = 1f32;
    for (p, q) in [
        (-d.x, a.x),
        (d.x, n as f32 - a.x),
        (-d.y, a.y),
        (d.y, n as f32 - a.y),
    ] {
        if p == 0. {
            if q < 0. {
                return;
            }
        } else {
            let r = q / p;
            if p < 0. {
                lo = lo.max(r);
            } else {
                hi = hi.min(r);
            }
        }
    }
    if lo > hi {
        return;
    }
    let a = a + d * lo;
    let b = a + d * (hi - lo);
    let steps = (b - a).abs().max_element().ceil() as usize;
    for k in 0..=steps {
        let p = a + (b - a) * (k as f32 / steps.max(1) as f32);
        let x = (p.x.floor() as i64).clamp(0, n as i64 - 1) as usize;
        let y = (p.y.floor() as i64).clamp(0, n as i64 - 1) as usize;
        pixels[(y * n + x) * 4..(y * n + x) * 4 + 4].copy_from_slice(&[224, 231, 239, 255]);
    }
}
/// 内边距扩张：每个像素给出扩张后区域内最近覆盖像素的索引，
/// u32::MAX 表示扩张后仍未覆盖。哨兵 u32 替代 Option<usize>——
/// Option 无 niche 优化占 16 B/槽，4096² 下多占约 200 MB。
pub fn dilate(covered: &[bool], size: usize, margin: u32) -> Vec<u32> {
    let mut nearest: Vec<u32> = covered
        .iter()
        .enumerate()
        .map(|(i, c)| if *c { i as u32 } else { u32::MAX })
        .collect();
    let mut queue: VecDeque<u32> = VecDeque::new();
    let mut distance = vec![u32::MAX; covered.len()];
    for (i, c) in covered.iter().enumerate() {
        if *c {
            distance[i] = 0;
            queue.push_back(i as u32);
        }
    }
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        if distance[i] >= margin {
            continue;
        }
        let x = i % size;
        let y = i / size;
        for (dx, dy) in [
            (-1, -1),
            (0, -1),
            (1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ] {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx < 0 || ny < 0 || nx >= size as isize || ny >= size as isize {
                continue;
            }
            let j = ny as usize * size + nx as usize;
            if nearest[j] == u32::MAX {
                nearest[j] = nearest[i];
                distance[j] = distance[i] + 1;
                queue.push_back(j as u32);
            }
        }
    }
    nearest
}

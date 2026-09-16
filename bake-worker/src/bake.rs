use crate::{
    gpu::{Gpu, Surface},
    model::{inspect, Model},
};
use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
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
    /// AI 降噪（OIDN）：开启时烘焙完成后对 AO 灰度执行降噪。
    #[serde(default)]
    pub denoise: bool,
    /// 降噪组件 DLL 所在目录（由主应用按需下载后传入）。
    #[serde(default)]
    pub oidn_dir: Option<String>,
    pub distance: f32,
    pub margin: u32,
    pub self_only: bool,
    pub ao: bool,
    pub uv: bool,
    pub id: bool,
    #[serde(default)]
    pub normal: bool,
    #[serde(default)]
    pub world_normal: bool,
    #[serde(default)]
    pub curvature: bool,
    #[serde(default)]
    pub position: bool,
    #[serde(default)]
    pub thickness: bool,
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
    /// 失败/未完成材质的结构化列表，供前端在材质列表上打徽标。
    #[serde(default)]
    pub failed_materials: Vec<usize>,
    pub cancelled: bool,
    pub elapsed_ms: u128,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub selected_channels: BTreeMap<usize, u32>,
}
#[derive(Serialize, Deserialize)]
pub struct Artifact {
    pub kind: String,
    pub path: PathBuf,
}
#[derive(Serialize, Deserialize)]
pub struct Output {
    pub material: usize,
    pub kind: String,
    pub path: PathBuf,
}
/// 清洗非法字符、按字节预算截断并去掉首尾空格/点。截断按 UTF-8 字节数而非
/// 字符数：80 个汉字 = 240 字节，叠加缓存根路径会超 Windows 默认 260 路径
/// 上限，导致该材质全部贴图保存失败。空结果返回空串（调用方决定回退名）。
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
        // 120 字节给目录树与 "_world_normal.png" 这类后缀留出余量。
        .scan(0usize, |bytes, c| {
            let len = c.len_utf8();
            if *bytes + len > 120 {
                return None;
            }
            *bytes += len;
            Some(c)
        })
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
    let mut used = std::collections::HashSet::new();
    materials
        .iter()
        .enumerate()
        .map(|(id, _)| {
            let name = &names[id];
            let mut stem = if name.is_empty() {
                format!("material_{id}")
            } else if names.iter().filter(|other| *other == name).count() > 1 {
                format!("{name}_{id}")
            } else {
                name.clone()
            };
            // 消歧结果仍可能与另一材质的字面名重合（["Gold","Gold","Gold_0"]
            // → 两个 "Gold_0"），按已用集合兜底追加后缀直到唯一，杜绝输出
            // 文件静默互相覆盖。
            while !used.insert(stem.clone()) {
                stem = format!("{stem}_{id}");
            }
            stem
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
    persist_with_retry(f, path)?;
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
    persist_with_retry(f, path)?;
    Ok(())
}
pub fn color(material: usize) -> [u8; 4] {
    let value = (material as u32 + 1).wrapping_mul(0x9e3779) & 0xffffff;
    [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255]
}

fn unit_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// 8 位标量输出的 TPDF 抖动（两路均匀噪声相减，幅度 ±1 LSB，均值 0）：
/// AO/厚度的缓坡渐变在 8 位下会产生量化条带，抖动把它们打散为不可见噪点。
pub(crate) fn dither_lsb(index: usize) -> f32 {
    let mix = |mut x: u64| {
        x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        ((x >> 40) & 0xFFFF) as f32 / 65536.0
    };
    mix(index as u64) + mix((index as u64).wrapping_add(0x5DEE_CE66)) - 1.0
}

fn encode_surface_map(
    surfaces: &[Surface],
    nearest: &[u32],
    mut encode: impl FnMut(&Surface) -> [u8; 4],
) -> Vec<u8> {
    let mut pixels = vec![0; nearest.len() * 4];
    for surface in surfaces {
        let offset = surface.pixel as usize * 4;
        pixels[offset..offset + 4].copy_from_slice(&encode(surface));
    }
    for (pixel, source) in nearest.iter().copied().enumerate() {
        if source == u32::MAX {
            continue;
        }
        let source = source as usize * 4;
        let value = [
            pixels[source],
            pixels[source + 1],
            pixels[source + 2],
            pixels[source + 3],
        ];
        pixels[pixel * 4..pixel * 4 + 4].copy_from_slice(&value);
    }
    pixels
}

/// Windows 上刚写完的文件可能被杀软/索引器短暂持有，persist 的 rename 会
/// 撞"拒绝访问"(os error 5)。带退避重试，每次失败取回 NamedTempFile 句柄，
/// 清除这类瞬时失败而不损数据。
fn persist_with_retry(f: tempfile::NamedTempFile, path: &Path) -> Result<(), String> {
    let mut file = f;
    let mut delay = 50u64;
    let mut last = String::new();
    for _ in 0..5 {
        match file.persist(path) {
            Ok(_) => return Ok(()),
            Err(e) => {
                last = e.error.to_string();
                file = e.file;
                std::thread::sleep(std::time::Duration::from_millis(delay));
                delay = (delay * 2).min(800);
            }
        }
    }
    Err(last)
}

pub(crate) fn curvature_map(surfaces: &[Surface], nearest: &[u32], size: usize) -> Vec<u8> {
    let values = curvature_values(surfaces, nearest, size);
    let mut pixels = vec![0u8; nearest.len() * 4];
    for (rgba, value) in pixels.chunks_exact_mut(4).zip(&values) {
        let v = unit_byte(*value);
        rgba[..4].copy_from_slice(&[v, v, v, 255]);
    }
    pixels
}

/// 16 位曲率图：磨损/边缘污垢遮罩用途下对条带敏感，动态范围 ×256。
pub(crate) fn curvature_map_16(surfaces: &[Surface], nearest: &[u32], size: usize) -> Vec<u16> {
    let values = curvature_values(surfaces, nearest, size);
    let mut pixels = vec![0u16; nearest.len() * 4];
    for (rgba, value) in pixels.chunks_exact_mut(4).zip(&values) {
        let v = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
        rgba[..4].copy_from_slice(&[v, v, v, 65535]);
    }
    pixels
}

/// 曲率值计算（0.5 中性背景 + 折痕处的符号偏移）。
fn curvature_values(surfaces: &[Surface], nearest: &[u32], size: usize) -> Vec<f32> {
    let mut surface_at = vec![u32::MAX; nearest.len()];
    for (index, surface) in surfaces.iter().enumerate() {
        surface_at[surface.pixel as usize] = index as u32;
    }
    let mut values = vec![0.5f32; nearest.len()];
    for surface in surfaces {
        let pixel = surface.pixel as usize;
        let x = pixel % size;
        let y = pixel / size;
        let position = Vec3::from_array(surface.position);
        let normal = Vec3::from_array(surface.normal).normalize_or_zero();
        // 先收集 8 邻域样本（符号贡献 + 3D 距离），再按距离中位数 3 倍剔除
        // 离群邻居：UV 岛在纹理空间相邻但 3D 空间相距很远，不剔除会在所有
        // 岛边上描出假曲率边。
        let mut dots = [0f32; 8];
        let mut dists = [1f64; 8];
        let mut sample_count = 0usize;
        for (dx, dy) in [
            (-1isize, 0isize),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ] {
            let nx = x as isize + dx;
            let ny = y as isize + dy;
            if nx < 0 || ny < 0 || nx >= size as isize || ny >= size as isize {
                continue;
            }
            let other_index = surface_at[ny as usize * size + nx as usize];
            if other_index == u32::MAX {
                continue;
            }
            let other = surfaces[other_index as usize];
            let delta = Vec3::from_array(other.position) - position;
            if delta.length_squared() <= 1e-20 {
                continue;
            }
            let delta_normal = Vec3::from_array(other.normal).normalize_or_zero() - normal;
            dots[sample_count] = delta_normal.dot(delta.normalize());
            dists[sample_count] = delta.length() as f64;
            sample_count += 1;
        }
        if sample_count == 0 {
            continue;
        }
        let mut sorted = dists[..sample_count].to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let cutoff = sorted[sample_count / 2] * 3.0;
        let mut signed = 0.0;
        let mut count = 0.0;
        for i in 0..sample_count {
            if dists[i] > cutoff {
                continue;
            }
            signed += dots[i];
            count += 1.0;
        }
        if count == 0.0 {
            continue;
        }
        values[pixel] = 0.5 + signed / count * 6.0;
    }
    // margin 填充在值域完成：最近覆盖像素的曲率值复制到扩张区。
    for (pixel, source) in nearest.iter().copied().enumerate() {
        if source == u32::MAX {
            continue;
        }
        values[pixel] = values[source as usize];
    }
    values
}

fn write_rgba_map(
    options: &Options,
    result: &mut ResultSet,
    material: usize,
    prefix: &str,
    kind: &str,
    pixels: Vec<u8>,
) -> Result<(), String> {
    let path = options.output.join(format!("{prefix}_{kind}.png"));
    save(
        &path,
        image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(options.resolution, options.resolution, pixels)
                .ok_or("Mesh Map 图像尺寸错误")?,
        ),
    )?;
    result.files.push(Output {
        material,
        kind: kind.into(),
        path,
    });
    atomic_json(&options.output.join("result.json"), result)
}

/// 表面图像素：8 位常规路径 / 16 位（position/world_normal 在 bits=16 时）。
enum SurfacePixels {
    Bits8(Vec<u8>),
    Bits16(Vec<u16>),
}

/// 写表面编码图（AO 以外的 Mesh Map），按位深落盘 Rgba8/Rgba16。
fn write_surface_map(
    options: &Options,
    result: &mut ResultSet,
    material: usize,
    prefix: &str,
    kind: &str,
    pixels: SurfacePixels,
) -> Result<(), String> {
    let path = options.output.join(format!("{prefix}_{kind}.png"));
    let image = match pixels {
        SurfacePixels::Bits8(data) => image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(options.resolution, options.resolution, data)
                .ok_or("Mesh Map 图像尺寸错误")?,
        ),
        SurfacePixels::Bits16(data) => image::DynamicImage::ImageRgba16(
            image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                options.resolution,
                options.resolution,
                data,
            )
            .ok_or("Mesh Map 图像尺寸错误")?,
        ),
    };
    save(&path, image)?;
    result.files.push(Output {
        material,
        kind: kind.into(),
        path,
    });
    atomic_json(&options.output.join("result.json"), result)
}

/// 16 位版表面编码：编码闭包直接输出 0–65535 通道值。
fn encode_surface_map_16(
    surfaces: &[Surface],
    nearest: &[u32],
    mut encode: impl FnMut(&Surface) -> [u16; 4],
) -> Vec<u16> {
    let mut pixels = vec![0u16; nearest.len() * 4];
    for (i, surface) in surfaces.iter().enumerate() {
        pixels[i * 4..i * 4 + 4].copy_from_slice(&encode(surface));
    }
    // margin 像素取最近覆盖像素的编码值（与 8 位版同语义）
    for (pixel, source) in nearest.iter().copied().enumerate() {
        if source == u32::MAX {
            continue;
        }
        let source = source as usize * 4;
        let value = [
            pixels[source],
            pixels[source + 1],
            pixels[source + 2],
            pixels[source + 3],
        ];
        pixels[pixel * 4..pixel * 4 + 4].copy_from_slice(&value);
    }
    pixels
}

fn save_scalar_map(
    options: &Options,
    path: &Path,
    nearest: &[u32],
    values: &[f32],
    background: f32,
) -> Result<(), String> {
    if options.bits == 16 {
        let data = nearest
            .iter()
            .map(|source| {
                let value = if *source == u32::MAX {
                    background
                } else {
                    values[*source as usize]
                };
                (value.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
            .collect::<Vec<_>>();
        save(
            path,
            image::DynamicImage::ImageLuma16(
                image::ImageBuffer::from_raw(options.resolution, options.resolution, data)
                    .ok_or("灰度 Mesh Map 尺寸错误")?,
            ),
        )
    } else {
        let data = nearest
            .iter()
            .enumerate()
            .map(|(index, source)| {
                if *source == u32::MAX {
                    unit_byte(background)
                } else {
                    // 覆盖像素加 TPDF 抖动防条带；背景保持精确值。
                    unit_byte(values[*source as usize] + dither_lsb(index) / 255.0)
                }
            })
            .collect::<Vec<_>>();
        save(
            path,
            image::DynamicImage::ImageLuma8(
                image::GrayImage::from_raw(options.resolution, options.resolution, data)
                    .ok_or("灰度 Mesh Map 尺寸错误")?,
            ),
        )
    }
}

fn completion_kind(options: &Options) -> &'static str {
    if options.thickness {
        "thickness"
    } else if options.ao {
        "ao"
    } else if options.curvature {
        "curvature"
    } else if options.position {
        "position"
    } else if options.world_normal {
        "world_normal"
    } else if options.normal {
        "normal"
    } else if options.id {
        "id"
    } else {
        "uv"
    }
}

fn enabled_map_count(options: &Options) -> usize {
    [
        options.uv,
        options.id,
        options.normal,
        options.world_normal,
        options.position,
        options.curvature,
        options.ao,
        options.thickness,
    ]
    .into_iter()
    .filter(|enabled| *enabled)
    .count()
    .max(1)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_bake_progress(
    progress: &mut impl FnMut(serde_json::Value),
    phase: String,
    stage: &str,
    map: &str,
    material: usize,
    material_position: usize,
    material_total: usize,
    map_index: usize,
    map_total: usize,
    within_map: f64,
    block_size: Option<usize>,
) {
    let completed = material_position * map_total + map_index;
    let total = (material_total * map_total).max(1);
    // Reserve the final 4% for legends, remapped model artifacts and the manifest.
    // This keeps the UI below 100% while files are still being finalized.
    let overall = (completed as f64 + within_map.clamp(0.0, 1.0)) / total as f64 * 0.96;
    progress(serde_json::json!({
        "phase": phase,
        "stage": stage,
        "map": map,
        "material": material,
        "materialPosition": material_position + 1,
        "materialTotal": material_total,
        "mapPosition": (map_index + 1).min(map_total),
        "mapTotal": map_total,
        "progress": overall,
        "blockSize": block_size,
    }));
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
        || !(options.ao
            || options.uv
            || options.id
            || options.normal
            || options.world_normal
            || options.curvature
            || options.position
            || options.thickness)
    {
        return Err("请选择材质和输出类型".into());
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
    // 输出目录在 %APPDATA%（通常系统盘），4K×多材质×多图可写满磁盘白烧数十
    // 分钟 GPU。按未压缩体积 6 折预估 PNG 总量（噪声内容压缩率差；ID 这类
    // 平色图远小于此），不足时提前报错而不是逐材质失败。
    let pixels = u64::from(options.resolution).pow(2);
    let rgba_maps = usize::from(options.id)
        + usize::from(options.normal)
        + usize::from(options.world_normal)
        + usize::from(options.curvature)
        + usize::from(options.position);
    let gray_maps = usize::from(options.ao) + usize::from(options.thickness);
    let estimate = (pixels * 4 * rgba_maps as u64
        + pixels * u64::from(options.bits / 8) * gray_maps as u64)
        * 3
        / 5
        * options.materials.len() as u64;
    let disks = sysinfo::Disks::new_with_refreshed_list();
    let output = options.output.canonicalize().unwrap_or_else(|_| options.output.clone());
    let disk = disks
        .list()
        .iter()
        .filter(|d| {
            output.as_os_str().as_encoded_bytes().starts_with(
                d.mount_point().as_os_str().as_encoded_bytes(),
            )
        })
        .max_by_key(|d| d.mount_point().as_os_str().len());
    if let Some(disk) = disk {
        let available = disk.available_space();
        if estimate > available {
            return Err(format!(
                "磁盘空间不足：预计输出约 {} MiB，{} 可用 {} MiB",
                estimate / 1048576,
                disk.mount_point().display(),
                available / 1048576
            ));
        }
    }
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
    let material_total = options.materials.len();
    let map_total = enabled_map_count(options);
    // 降噪组件整批只加载一次：每材质重复 LoadLibrary/FreeLibrary 并两次翻转
    // 进程 cwd，是批量降噪失败（OIDN 错误码 3）的头号嫌疑，也白白拖慢开跑。
    let denoiser = if options.denoise && options.ao {
        let oidn_dir = options
            .oidn_dir
            .as_deref()
            .ok_or("已启用 AI 降噪但缺少降噪组件目录")?;
        Some(crate::denoise::Oidn::load(std::path::Path::new(oidn_dir))?)
    } else {
        None
    };
    // OIDN CPU 降噪单次可达分钟级且期间零输出；心跳行喂宿主的 stall 看门狗，
    // 防止低速机上正常的降噪被"无输出超时"误杀。
    let denoising = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(denoiser.is_some()));
    let heartbeat = if denoiser.is_some() {
        let flag = denoising.clone();
        Some(std::thread::spawn(move || {
            // 短步长轮询停止标志：粗睡眠会让 run() 结束时的 join 最多空等一个周期。
            let mut beat_elapsed = 0u64;
            loop {
                if !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
                if !flag.load(std::sync::atomic::Ordering::Relaxed) {
                    break;
                }
                beat_elapsed += 1;
                // 每 120 步（60 秒）一行心跳，喂宿主的 stall 看门狗。
                if beat_elapsed % 120 == 0 {
                    println!("{}", serde_json::json!({ "type": "heartbeat" }));
                }
            }
        }))
    } else {
        None
    };
    for (position, material) in options.materials.iter().copied().enumerate() {
        if options.cancel_path.exists() {
            result.cancelled = true;
            break;
        }
        let channel = options.channels.get(&material).copied().unwrap_or(0);
        let report = inspect(&model, material, channel, &options.objects);
        let prefix = stems[material].clone();
        let material_label = format!(
            "材质 {} · {}",
            material,
            model.materials[material].name.trim()
        );
        let attempt = (|| -> Result<(), String> {
            let mut map_index = 0usize;
            emit_bake_progress(
                &mut progress,
                format!("{material_label} · 准备 UV 像素"),
                "raster",
                "",
                material,
                position,
                material_total,
                map_index,
                map_total,
                0.0,
                None,
            );
            let (surfaces, covered, wire) = raster(
                &model,
                material,
                channel,
                &options.objects,
                options.resolution,
                options.uv,
            )?;
            if options.uv {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · UV 线框"),
                    "mesh_map",
                    "uv",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
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
                map_index += 1;
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            let data_maps = options.ao
                || options.id
                || options.normal
                || options.world_normal
                || options.curvature
                || options.position
                || options.thickness;
            if !report.valid && data_maps {
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
            if surfaces.is_empty() && data_maps {
                return Err("所选材质没有像素覆盖".into());
            }
            let nearest = if data_maps {
                dilate(&covered, options.resolution as usize, options.margin)
            } else {
                vec![]
            };
            if options.id {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 材质 ID"),
                    "mesh_map",
                    "id",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
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
                map_index += 1;
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            if options.normal {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 切线空间法线"),
                    "mesh_map",
                    "normal",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
                write_rgba_map(
                    options,
                    &mut result,
                    material,
                    &prefix,
                    "normal",
                    encode_surface_map(&surfaces, &nearest, |_| [128, 128, 255, 255]),
                )?;
                map_index += 1;
            }
            if options.world_normal {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 世界空间法线"),
                    "mesh_map",
                    "world_normal",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
                let encode_normal_16 = |surface: &Surface| -> [u16; 4] {
                    let normal = Vec3::from_array(surface.normal).normalize_or_zero();
                    let channel = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    [
                        channel(normal.x * 0.5 + 0.5),
                        channel(normal.y * 0.5 + 0.5),
                        channel(normal.z * 0.5 + 0.5),
                        65535,
                    ]
                };
                let encode_normal_8 = |surface: &Surface| -> [u8; 4] {
                    let normal = Vec3::from_array(surface.normal).normalize_or_zero();
                    [
                        unit_byte(normal.x * 0.5 + 0.5),
                        unit_byte(normal.y * 0.5 + 0.5),
                        unit_byte(normal.z * 0.5 + 0.5),
                        255,
                    ]
                };
                let pixels = if options.bits == 16 {
                    SurfacePixels::Bits16(encode_surface_map_16(&surfaces, &nearest, encode_normal_16))
                } else {
                    SurfacePixels::Bits8(encode_surface_map(&surfaces, &nearest, encode_normal_8))
                };
                write_surface_map(
                    options,
                    &mut result,
                    material,
                    &prefix,
                    "world_normal",
                    pixels,
                )?;
                map_index += 1;
            }
            if options.position {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 位置"),
                    "mesh_map",
                    "position",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
                let min = Vec3::from_array(model.bounds[0]);
                let span = (Vec3::from_array(model.bounds[1]) - min).max(Vec3::splat(1e-12));
                let encode_position_16 = |surface: &Surface| -> [u16; 4] {
                    let value = (Vec3::from_array(surface.position) - min) / span;
                    let channel = |v: f32| (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
                    [
                        channel(value.x),
                        channel(value.y),
                        channel(value.z),
                        65535,
                    ]
                };
                let encode_position_8 = |surface: &Surface| -> [u8; 4] {
                    let value = (Vec3::from_array(surface.position) - min) / span;
                    [
                        unit_byte(value.x),
                        unit_byte(value.y),
                        unit_byte(value.z),
                        255,
                    ]
                };
                let pixels = if options.bits == 16 {
                    SurfacePixels::Bits16(encode_surface_map_16(&surfaces, &nearest, encode_position_16))
                } else {
                    SurfacePixels::Bits8(encode_surface_map(&surfaces, &nearest, encode_position_8))
                };
                write_surface_map(
                    options,
                    &mut result,
                    material,
                    &prefix,
                    "position",
                    pixels,
                )?;
                map_index += 1;
            }
            if options.curvature {
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 曲率"),
                    "mesh_map",
                    "curvature",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.1,
                    None,
                );
                let pixels = if options.bits == 16 {
                    SurfacePixels::Bits16(curvature_map_16(
                        &surfaces,
                        &nearest,
                        options.resolution as usize,
                    ))
                } else {
                    SurfacePixels::Bits8(curvature_map(
                        &surfaces,
                        &nearest,
                        options.resolution as usize,
                    ))
                };
                write_surface_map(
                    options,
                    &mut result,
                    material,
                    &prefix,
                    "curvature",
                    pixels,
                )?;
                map_index += 1;
            }
            if options.ao {
                if gpu.is_none() {
                    emit_bake_progress(
                        &mut progress,
                        "构建 GPU 加速结构".into(),
                        "prepare_gpu",
                        "ao",
                        material,
                        position,
                        material_total,
                        map_index,
                        map_total,
                        0.0,
                        None,
                    );
                    let selected: Vec<_> = model
                        .triangles
                        .iter()
                        .filter(|t| options.objects.contains(&t.object))
                        .collect();
                    let vertices: Vec<_> = selected.iter().flat_map(|t| t.positions).collect();
                    let objects: Vec<_> = selected.iter().map(|t| t.object as u32).collect();
                    gpu = Some(Gpu::new(options.device, &vertices, &objects, options.self_only)?);
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
                        emit_bake_progress(
                            &mut progress,
                            format!("{material_label} · GPU AO"),
                            "ao",
                            "ao",
                            material,
                            position,
                            material_total,
                            map_index,
                            map_total,
                            done * 0.84,
                            Some(block),
                        );
                    }
                }
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · GPU AO"),
                    "ao",
                    "ao",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.84,
                    Some(block),
                );
                if options.denoise {
                    emit_bake_progress(
                        &mut progress,
                        format!("{material_label} · AI 降噪"),
                        "denoise",
                        "ao",
                        material,
                        position,
                        material_total,
                        map_index,
                        map_total,
                        0.88,
                        None,
                    );
                    let denoiser = denoiser
                        .as_ref()
                        .ok_or("已启用 AI 降噪但缺少降噪组件目录")?;
                    // 降噪前用 dilate 最近覆盖值预填 margin：OIDN 是卷积滤波，
                    // 未覆盖像素的 1.0 背景会把岛边 AO 拉亮并跨岛渗色。
                    for (i, s) in nearest.iter().enumerate() {
                        if *s != u32::MAX && *s != i as u32 {
                            values[i] = values[*s as usize];
                        }
                    }
                    let covered_original: Vec<f32> = values.clone();
                    denoiser.denoise_gray(
                        &mut values,
                        options.resolution as usize,
                        options.resolution as usize,
                    )?;
                    // 覆盖像素写回原始值：OIDN 不应模糊本来就正确的数据，
                    // 只让 margin 保留平滑填充的结果。
                    for (i, s) in nearest.iter().enumerate() {
                        if *s != u32::MAX {
                            values[i] = covered_original[i];
                        }
                    }
                }
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 保存 AO"),
                    "save",
                    "ao",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.94,
                    None,
                );
                let path = options.output.join(format!("{prefix}_ao.png"));
                if options.bits == 16 {
                    let data: Vec<u16> = nearest
                        .iter()
                        .map(|s| {
                            let value = if *s == u32::MAX {
                                1.
                            } else {
                                values[*s as usize]
                            };
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
                        .enumerate()
                        .map(|(index, s)| {
                            let value = if *s == u32::MAX {
                                1.
                            } else {
                                // TPDF 抖动打散 8 位缓坡条带；背景保持精确 255。
                                values[*s as usize] + dither_lsb(index) / 255.0
                            };
                            (value * 255.).round().clamp(0., 255.) as u8
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
                map_index += 1;
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            if options.thickness {
                if gpu.is_none() {
                    emit_bake_progress(
                        &mut progress,
                        "构建 GPU 加速结构".into(),
                        "prepare_gpu",
                        "thickness",
                        material,
                        position,
                        material_total,
                        map_index,
                        map_total,
                        0.0,
                        None,
                    );
                    let selected: Vec<_> = model
                        .triangles
                        .iter()
                        .filter(|t| options.objects.contains(&t.object))
                        .collect();
                    let vertices: Vec<_> = selected.iter().flat_map(|t| t.positions).collect();
                    let objects: Vec<_> = selected.iter().map(|t| t.object as u32).collect();
                    gpu = Some(Gpu::new(options.device, &vertices, &objects, options.self_only)?);
                }
                let gpu = gpu.as_mut().ok_or("GPU 加速结构未初始化")?;
                let diagonal = (Vec3::from_array(model.bounds[1])
                    - Vec3::from_array(model.bounds[0]))
                .length();
                let bias = (diagonal * 1e-5).max(1e-7).min(options.distance * 0.01);
                let mut values = vec![0f32; covered.len()];
                let block = gpu.block_size;
                let mut last_emit = Instant::now();
                let mut last_percent = -1i64;
                for (chunk_index, chunk) in surfaces.chunks(block).enumerate() {
                    let sums = gpu.trace_thickness(
                        chunk,
                        options.samples,
                        options.distance,
                        bias,
                        options.self_only,
                        || options.cancel_path.exists(),
                    )?;
                    result.peak_device_bytes = result.peak_device_bytes.max(gpu.peak_device_bytes);
                    for (surface, sum) in chunk.iter().zip(sums) {
                        values[surface.pixel as usize] =
                            sum as f32 / (options.samples as f32 * 65535.0);
                    }
                    let done = (chunk_index * block + chunk.len()) as f64 / surfaces.len() as f64;
                    let percent = (done * 100.0) as i64;
                    if percent != last_percent && last_emit.elapsed().as_millis() >= 100 {
                        last_emit = Instant::now();
                        last_percent = percent;
                        emit_bake_progress(
                            &mut progress,
                            format!("{material_label} · GPU 厚度"),
                            "thickness",
                            "thickness",
                            material,
                            position,
                            material_total,
                            map_index,
                            map_total,
                            done * 0.92,
                            Some(block),
                        );
                    }
                }
                emit_bake_progress(
                    &mut progress,
                    format!("{material_label} · 保存厚度"),
                    "save",
                    "thickness",
                    material,
                    position,
                    material_total,
                    map_index,
                    map_total,
                    0.94,
                    Some(block),
                );
                let path = options.output.join(format!("{prefix}_thickness.png"));
                save_scalar_map(options, &path, &nearest, &values, 0.0)?;
                result.files.push(Output {
                    material,
                    kind: "thickness".into(),
                    path,
                });
                map_index += 1;
                atomic_json(&options.output.join("result.json"), &result)?;
            }
            emit_bake_progress(
                &mut progress,
                format!("{material_label} · 已完成"),
                "material_complete",
                "",
                material,
                position,
                material_total,
                map_index.saturating_sub(1),
                map_total,
                1.0,
                None,
            );
            Ok(())
        })();
        if let Err(e) = attempt {
            // 用户主动取消不是失败：GPU trace 的"任务已取消"错误不进失败名单
            //（未完成清单由下方 cancelled 分支统一列出），避免与 UV 校验失败、
            // 显存不足这类真实错误混在一起。
            if options.cancel_path.exists() {
                result.cancelled = true;
                result.failed_materials.push(material);
            } else {
                result.failures.push(format!("{material_label}：{e}"));
                result.failed_materials.push(material);
            }
        }
        result.elapsed_ms = start.elapsed().as_millis();
        atomic_json(&options.output.join("result.json"), &result)?;
        if result.cancelled {
            break;
        }
    }
    denoising.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Some(heartbeat) = heartbeat {
        let _ = heartbeat.join();
    }
    if !result.cancelled {
        progress(serde_json::json!({
            "phase": "整理贴图、模型与清单",
            "stage": "finalize",
            "map": "",
            "materialPosition": material_total,
            "materialTotal": material_total,
            "mapPosition": map_total,
            "mapTotal": map_total,
            "progress": 0.97,
        }));
    }
    if options.id {
        let legend:Vec<_>=options.materials.iter().map(|i|serde_json::json!({"material":i,"name":model.materials[*i].name,"rgba":color(*i)})).collect();
        atomic_json(&options.output.join("material-colors.json"), &legend)?;
    }
    if result.cancelled {
        let complete: std::collections::HashSet<_> = result
            .files
            .iter()
            .filter(|f| f.kind == completion_kind(options))
            .map(|f| f.material)
            .collect();
        for m in &options.materials {
            if !complete.contains(m) {
                result.failed_materials.push(*m);
            }
        }
    }
    result.elapsed_ms = start.elapsed().as_millis();
    result.selected_channels = options.channels.clone();
    if !model.generated_channels.is_empty() {
        for path in crate::model::export_bake_model(&model, &options.channels, &options.output)? {
            result.artifacts.push(Artifact {
                kind: "model".into(),
                path,
            });
        }
    }
    let manifest_path = options.output.join("bake-manifest.json");
    let texture_manifest: Vec<_> = result.files.iter().map(|file| serde_json::json!({
        "material":file.material,"kind":file.kind,"file":file.path.file_name().unwrap_or_default().to_string_lossy()
    })).collect();
    let artifact_manifest: Vec<_> = result
        .artifacts
        .iter()
        .map(|file| {
            serde_json::json!({
                "kind":file.kind,"file":file.path.file_name().unwrap_or_default().to_string_lossy()
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "model": model.name,
        "sourceFormat": model.source_format,
        "selectedChannels": options.channels,
        "generatedChannels": model.generated_channels,
        "meshMapConventions": {
            "padding": "margin px of exact euclidean nearest-covered dilation; background: ao/thickness constant, curvature 0.5",
            "ao": "linear grayscale; 1 = unoccluded",
            "normal": "OpenGL tangent space; +Y",
            "world_normal": "RGB = world XYZ remapped from -1..1 to 0..1",
            "curvature": "signed grayscale; 0.5 = flat, dark = concave, light = convex",
            "position": "RGB = model-bounds normalized world XYZ",
            "thickness": "linear grayscale; normalized inward hit distance",
            "id": "RGBA material color; see material-colors.json",
            "uv": "diagnostic wireframe only"
        },
        "textures": texture_manifest,
        "artifacts": artifact_manifest,
    });
    atomic_json(&manifest_path, &manifest)?;
    result.artifacts.push(Artifact {
        kind: "manifest".into(),
        path: manifest_path,
    });
    atomic_json(&options.output.join("result.json"), &result)?;
    if !result.cancelled {
        progress(serde_json::json!({
            "phase": "结果已就绪",
            "stage": "finalize",
            "map": "",
            "materialPosition": material_total,
            "materialTotal": material_total,
            "mapPosition": map_total,
            "mapTotal": map_total,
            "progress": 1.0,
        }));
    }
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
///
/// 精确欧氏距离变换（Felzenszwalb 1D 两遍：先列后行），带源下标跟踪。
/// 旧 8 邻域 BFS 是切比雪夫度量：对角方向环宽多延伸约 41%，padding 环
/// 宽不均且平局源选取有方向偏差。阈值按四舍五入像素距（√d² < margin+0.5）
/// 判定，margin=1 时对角邻域（d²=2）仍在环内，与旧行为兼容。
pub fn dilate(covered: &[bool], size: usize, margin: u32) -> Vec<u32> {
    let n = covered.len();
    let inf: u32 = 1 << 30;
    let mut f = vec![0u32; n];
    let mut src_in = vec![0u32; n];
    for (i, c) in covered.iter().enumerate() {
        if *c {
            f[i] = 0;
            src_in[i] = i as u32;
        } else {
            f[i] = inf;
            src_in[i] = u32::MAX;
        }
    }
    // 两遍 DT 各线（列/行）彼此独立，用 rayon 按线并行：
    // 列遍历是跨步访存，串行会拖慢整个烘焙（2048² 下实测 +0.9s）。
    // 列结果以转置布局存放：第一遍按 x 得到连续可变块，第二遍共享只读。
    use rayon::prelude::*;
    let mut col_d_t = vec![0u32; n]; // col_d_t[x * size + y]
    let mut col_src_t = vec![0u32; n];
    col_d_t
        .par_chunks_mut(size)
        .zip(col_src_t.par_chunks_mut(size))
        .zip(f.par_chunks(size))
        .zip(src_in.par_chunks(size))
        .enumerate()
        .for_each(|(_, (((d_chunk, s_chunk), f_col), src_col))| {
            let (d, s) = dt_1d_sq(f_col, src_col);
            d_chunk.copy_from_slice(&d);
            s_chunk.copy_from_slice(&s);
        });
    // 第二遍：每行沿 x 对「列内距离 + 水平位移平方」再做 1D 变换。
    // 平方欧氏距离可分离，两遍组合即精确 2D 欧氏最近源。
    let threshold_cmp: u32 = ((2 * (margin as u64) + 1).pow(2)).min(u32::MAX as u64) as u32;
    let mut nearest = vec![u32::MAX; n];
    nearest
        .par_chunks_mut(size)
        .enumerate()
        .for_each(|(y, row_out)| {
            let row: Vec<u32> = (0..size).map(|x| col_d_t[x * size + y]).collect();
            let row_src: Vec<u32> = (0..size).map(|x| col_src_t[x * size + y]).collect();
            let (d, s) = dt_1d_sq(&row, &row_src);
            for x in 0..size {
                let index = y * size + x;
                if covered[index] {
                    row_out[x] = index as u32;
                } else if d[x] * 4 < threshold_cmp {
                    row_out[x] = s[x];
                }
            }
        });
    nearest
}

/// 一维平方欧氏距离变换（Felzenszwalb & Huttenlocher 下包络法）：
/// d[q] = min_p(f[p] + (p-q)²)，并跟踪最近源下标。i64 中间量防平方溢出
/// 与负差值下溢；z[0] = i64::MIN 保证首抛物线永不被弹出。
pub(crate) fn dt_1d_sq(f: &[u32], src_in: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let n = f.len();
    let mut d = vec![0u32; n];
    let mut src = vec![0u32; n];
    if n == 0 {
        return (d, src);
    }
    if n == 1 {
        d[0] = f[0];
        src[0] = src_in[0];
        return (d, src);
    }
    let mut v = vec![0usize; n]; // 包络中抛物线的顶点位置
    let mut z = vec![0i64; n + 1]; // 相邻抛物线边界的平方距
    let mut env_src = vec![0u32; n];
    let mut k = 0usize;
    v[0] = 0;
    z[0] = i64::MIN;
    z[1] = i64::MAX;
    env_src[0] = src_in[0];
    for q in 1..n {
        let fq = f[q] as i64 + (q * q) as i64;
        let mut s = (fq - (f[v[k]] as i64 + (v[k] * v[k]) as i64)) / (2 * (q - v[k]) as i64);
        while s <= z[k] {
            k -= 1;
            s = (fq - (f[v[k]] as i64 + (v[k] * v[k]) as i64)) / (2 * (q - v[k]) as i64);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = i64::MAX;
        env_src[k] = src_in[q];
    }
    let mut k = 0usize;
    for q in 0..n {
        while z[k + 1] < q as i64 {
            k += 1;
        }
        d[q] = ((q as i64 - v[k] as i64).pow(2) + f[v[k]] as i64) as u32;
        src[q] = env_src[k];
    }
    (d, src)
}

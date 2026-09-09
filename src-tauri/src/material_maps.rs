//! Local, deterministic material-map conversion. Height is data, not scene depth.
use base64::Engine;
use image::{DynamicImage, ImageBuffer, Luma, Rgb, RgbImage};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, io::Cursor, path::{Path, PathBuf}};
use tauri::AppHandle;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct Parameters {
    pub channel: String,
    pub smoothing: u32,
    pub contrast: f32,
    pub invert: bool,
    pub strength: f32,
    pub convention: String,
    pub boundary: String,
    pub bits: u8,
    pub also_height: bool,
}
impl Default for Parameters {
    fn default() -> Self {
        Self { channel: "luminance".into(), smoothing: 0, contrast: 1.0, invert: false,
            strength: 1.0, convention: "opengl".into(), boundary: "clamp".into(), bits: 16, also_height: false }
    }
}
impl Parameters {
    fn validate(&self) -> Result<(), String> {
        if !["luminance", "r", "g", "b", "alpha"].contains(&self.channel.as_str())
            || !["opengl", "directx"].contains(&self.convention.as_str())
            || !["clamp", "wrap"].contains(&self.boundary.as_str())
            || ![8, 16].contains(&self.bits) || self.smoothing > 20
            || !self.contrast.is_finite() || !(0.0..=4.0).contains(&self.contrast)
            || !self.strength.is_finite() || !(0.0..=10.0).contains(&self.strength) {
            return Err("贴图参数无效：平滑 0–20、对比度 0–4、强度 0–10、位深 8/16。".into());
        }
        Ok(())
    }
}

fn srgb_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}
fn coord(i: i64, len: u32, wrap: bool) -> u32 {
    if wrap { i.rem_euclid(i64::from(len)) as u32 } else { i.clamp(0, i64::from(len) - 1) as u32 }
}

struct HeightField { width: u32, height: u32, values: Vec<f32>, alpha: Vec<f32> }

fn height_field(source: &DynamicImage, p: &Parameters) -> HeightField {
    let is_gray = matches!(source, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageLuma16(_) | DynamicImage::ImageLumaA16(_));
    // Float conversion preserves all 16-bit samples; do not pass through rgba8.
    let rgba = source.to_rgba32f();
    let (w, h) = rgba.dimensions();
    let mut values = Vec::with_capacity((w as usize) * h as usize);
    let mut alpha = Vec::with_capacity(values.capacity());
    for pixel in rgba.pixels() {
        let value = match p.channel.as_str() {
            "r" => pixel[0], "g" => pixel[1], "b" => pixel[2], "alpha" => pixel[3],
            _ if is_gray => pixel[0],
            _ => 0.2126 * srgb_linear(pixel[0]) + 0.7152 * srgb_linear(pixel[1]) + 0.0722 * srgb_linear(pixel[2]),
        };
        let value = ((value - 0.5) * p.contrast + 0.5).clamp(0.0, 1.0);
        let value = if p.invert { 1.0 - value } else { value };
        values.push(value);
        alpha.push(pixel[3]);
    }
    if p.smoothing > 0 {
        values = smooth_height(&values, &alpha, w, h, p.smoothing, p.boundary == "wrap");
    }
    // Invisible texels have neutral height. Blend partial coverage only once.
    for (value, a) in values.iter_mut().zip(&alpha) { *value = 0.5 + (*value - 0.5) * a; }
    HeightField { width: w, height: h, values, alpha }
}

// Separable sliding box filter with alpha-weighted samples. O(pixels + radius*edges).
fn smooth_height(values: &[f32], alpha: &[f32], w: u32, h: u32, radius: u32, wrap: bool) -> Vec<f32> {
    let mut weighted: Vec<f32> = values.iter().zip(alpha).map(|(v, a)| v * a).collect();
    let mut weights = alpha.to_vec();
    for vertical in [false, true] {
        let mut out_v = vec![0.0; values.len()];
        let mut out_a = vec![0.0; values.len()];
        let (lines, length) = if vertical { (w, h) } else { (h, w) };
        for line in 0..lines {
            let index = |position: i64| {
                let pos = coord(position, length, wrap);
                if vertical { (pos * w + line) as usize } else { (line * w + pos) as usize }
            };
            let r = radius as i64;
            let (mut sum_v, mut sum_a) = (0.0f64, 0.0f64);
            for offset in -r..=r { sum_v += weighted[index(offset)] as f64; sum_a += weights[index(offset)] as f64; }
            let count = (2 * radius + 1) as f64;
            for pos in 0..length {
                let i = index(pos as i64);
                out_v[i] = (sum_v / count) as f32;
                out_a[i] = (sum_a / count) as f32;
                sum_v += weighted[index(pos as i64 + r + 1)] as f64 - weighted[index(pos as i64 - r)] as f64;
                sum_a += weights[index(pos as i64 + r + 1)] as f64 - weights[index(pos as i64 - r)] as f64;
            }
        }
        weighted = out_v; weights = out_a;
    }
    weighted.iter().zip(&weights).map(|(v, a)| if *a > 1e-6 { (v / a).clamp(0.0, 1.0) } else { 0.5 }).collect()
}

fn normals(field: &HeightField, p: &Parameters) -> RgbImage {
    let (w, h) = (field.width, field.height);
    let wrap = p.boundary == "wrap";
    RgbImage::from_fn(w, h, |x, y| {
        let i = (y * w + x) as usize;
        // At transparent boundaries substitute the center height for invisible neighbors.
        // Thus a flat opaque decal does not acquire a raised rim from its alpha mask.
        let sample = |sx: i64, sy: i64| {
            let j = (coord(sy, h, wrap) * w + coord(sx, w, wrap)) as usize;
            let a = field.alpha[j];
            let center_a = field.alpha[i];
            let center = if center_a > 1e-6 { (field.values[i] - 0.5) / center_a + 0.5 } else { 0.5 };
            let height = if a > 1e-6 { (field.values[j] - 0.5) / a + 0.5 } else { center };
            center + (height - center) * a
        };
        let dx = (sample(x as i64 + 1, y as i64) - sample(x as i64 - 1, y as i64)) * 0.5;
        let dy = (sample(x as i64, y as i64 + 1) - sample(x as i64, y as i64 - 1)) * 0.5;
        // Image Y points down; tangent-space OpenGL Y points up.
        let nx = -dx * p.strength * field.alpha[i];
        let ny = dy * p.strength * field.alpha[i];
        let norm = (nx * nx + ny * ny + 1.0).sqrt();
        let encode = |v: f32| ((v * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
        let green = encode(ny / norm);
        Rgb([encode(nx / norm), if p.convention == "directx" { 255 - green } else { green }, encode(1.0 / norm)])
    })
}

fn height_image(field: &HeightField, bits: u8) -> DynamicImage {
    if bits == 8 {
        DynamicImage::ImageLuma8(ImageBuffer::from_fn(field.width, field.height,
            |x, y| Luma([(field.values[(y * field.width + x) as usize] * 255.0).round() as u8])))
    } else {
        DynamicImage::ImageLuma16(ImageBuffer::from_fn(field.width, field.height,
            |x, y| Luma([(field.values[(y * field.width + x) as usize] * 65535.0).round() as u16])))
    }
}

fn decode(path: &Path) -> Result<DynamicImage, String> {
    let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_lowercase();
    if !["png", "jpg", "jpeg", "webp", "tga"].contains(&extension.as_str()) { return Err("支持 PNG、JPEG、WebP、TGA 素材。".into()); }
    let (w, h) = image::image_dimensions(path).map_err(crate::to_string_error)?;
    crate::safety::memory_budget(w, h, 64)?;
    let mut source = image::open(path).map_err(crate::to_string_error)?;
    if let Some(orientation) = crate::anime::exif_orientation(path)? { source.apply_orientation(orientation); }
    Ok(source)
}

fn png_data(image: &DynamicImage) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).map_err(crate::to_string_error)?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Preview {
    source: String, normal: String, height: String, width: u32, height_pixels: u32,
}
#[tauri::command]
pub(crate) async fn material_maps_preview(input: String, parameters: Parameters) -> Result<Preview, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = crate::safety::task_guard()?;
        preview(&input, &parameters)
    })
    .await
    .map_err(crate::to_string_error)?
}
fn preview(input: &str, parameters: &Parameters) -> Result<Preview, String> {
        parameters.validate()?;
        let image = decode(Path::new(&input))?;
        let source = if image.width().max(image.height()) > 1024 { image.resize(1024, 1024, image::imageops::FilterType::Triangle) } else { image };
        let field = height_field(&source, &parameters);
        Ok(Preview { source: png_data(&source)?, normal: png_data(&DynamicImage::ImageRgb8(normals(&field, &parameters)))?,
            height: png_data(&height_image(&field, 8))?, width: source.width(), height_pixels: source.height() })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RunOptions {
    pub files: Vec<String>, pub output_path: String, pub kind: String, pub parameters: Parameters,
}
fn stem(input: &Path) -> Result<String, String> {
    let name = input.file_stem().and_then(|v| v.to_str()).ok_or("素材文件名无效")?;
    let prefix = if name.to_lowercase().ends_with("_basecolor") { &name[..name.len() - "_basecolor".len()] } else { name };
    if prefix.is_empty() { return Err("素材名称不能只有 _basecolor。".into()); }
    Ok(prefix.into())
}
fn path_key(path: &Path) -> String { path.to_string_lossy().replace('/', "\\").to_lowercase() }

// Preflight every final output before processing any input. Canonical parents detect
// relative paths and directory aliases; existing output canonicalization detects symlinks.
fn output_plan(options: &RunOptions) -> Result<Vec<Vec<PathBuf>>, String> {
    options.parameters.validate()?;
    if !["normal", "height"].contains(&options.kind.as_str()) || options.files.is_empty() { return Err("请选择生成类型和素材。".into()); }
    let root = std::fs::canonicalize(&options.output_path).map_err(crate::to_string_error)?;
    let inputs: HashSet<String> = options.files.iter().map(|f| std::fs::canonicalize(f).map(|p| path_key(&p)).map_err(crate::to_string_error)).collect::<Result<_, _>>()?;
    let mut seen = HashSet::new();
    options.files.iter().map(|input| {
        let name = stem(Path::new(input))?;
        let kinds = if options.kind == "normal" && options.parameters.also_height { vec!["normal", "height"] } else { vec![options.kind.as_str()] };
        kinds.into_iter().map(|kind| {
            let path = root.join(format!("{name}_{kind}.png"));
            let key = path_key(&path);
            if !seen.insert(key.clone()) { return Err(format!("输出文件重名：{name}_{kind}.png，请重命名或分批导出。")); }
            let resolved = std::fs::canonicalize(&path).map(|p| path_key(&p)).unwrap_or(key);
            if inputs.contains(&resolved) { return Err(format!("输出会覆盖输入素材：{}", path.display())); }
            Ok(path)
        }).collect()
    }).collect()
}

fn save(image: &DynamicImage, path: &Path) -> Result<(), String> {
    crate::safety::atomic_write(path, |writer| image.write_to(writer, image::ImageFormat::Png).map_err(crate::to_string_error))
}
fn generate(app: Option<&AppHandle>, options: RunOptions) -> Result<crate::TaskResult, String> {
    if options.output_path.trim().is_empty() { return Err("请选择输出目录。".into()); }
    std::fs::create_dir_all(&options.output_path).map_err(crate::to_string_error)?;
    let plan = output_plan(&options)?;
    let mut outputs = Vec::new(); let mut logs = Vec::new(); let mut completed = 0;
    for (index, (input, targets)) in options.files.iter().zip(plan).enumerate() {
        let progress = |fraction: f64, phase: &str| {
            if let Some(app) = app { crate::emit_task_progress_percent(app, index, options.files.len(),
                format!("第 {}/{} 张 · {} · {phase}", index + 1, options.files.len(), Path::new(input).file_name().unwrap_or_default().to_string_lossy()),
                Some((index as f64 + fraction) / options.files.len() as f64 * 100.0)); }
        };
        let result = (|| {
            progress(0.0, "读取素材");
            let source = decode(Path::new(input))?;
            progress(0.2, "计算高度");
            let field = height_field(&source, &options.parameters);
            progress(0.6, "生成贴图");
            let normal = if options.kind == "normal" { Some(DynamicImage::ImageRgb8(normals(&field, &options.parameters))) } else { None };
            let height = if options.kind == "height" || options.parameters.also_height { Some(height_image(&field, options.parameters.bits)) } else { None };
            progress(0.85, "保存 PNG");
            for (i, target) in targets.iter().enumerate() {
                let image = if i == 0 && normal.is_some() { normal.as_ref().unwrap() } else { height.as_ref().unwrap() };
                save(image, target)?;
                outputs.push(target.display().to_string());
            }
            Ok::<_, String>(())
        })();
        match result { Ok(()) => { completed += 1; logs.push(format!("完成 {input}")); }, Err(error) => logs.push(format!("失败 {input}：{error}")) }
        progress(1.0, "该素材处理结束");
    }
    if completed == 0 { return Err(logs.join("\n")); }
    Ok(crate::TaskResult { completed, total: options.files.len(), logs, outputs })
}
#[tauri::command]
pub(crate) async fn material_maps_generate(app: AppHandle, options: RunOptions) -> Result<crate::TaskResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _task = crate::safety::task_guard()?;
        generate(Some(&app), options)
    })
    .await
    .map_err(crate::to_string_error)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
    fn gray(w: u32, h: u32, sample: impl Fn(u32, u32) -> u16) -> DynamicImage {
        DynamicImage::ImageLuma16(ImageBuffer::from_fn(w, h, |x,y| Luma([sample(x,y)])))
    }
    fn area() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("测试区/临时输出/material-map-tests");
        std::fs::create_dir_all(&root).unwrap(); tempfile::tempdir_in(root).unwrap()
    }
    #[test]
    fn flat_single_pixel_and_zero_strength_are_safe() {
        for (w,h) in [(1,1),(1,9),(9,1),(7,7)] {
            let source = gray(w,h,|_,_|22000);
            for boundary in ["clamp", "wrap"] {
                let p = Parameters { boundary: boundary.into(), smoothing: 20, ..Default::default() };
                let map = normals(&height_field(&source,&p), &p);
                assert!(map.pixels().all(|p| p.0 == [128,128,255]));
            }
        }
        let p = Parameters { strength: 0.0, ..Default::default() };
        assert!(normals(&height_field(&gray(9,9,|x,y| ((x+y)*3000) as u16), &p), &p).pixels().all(|p| p.0 == [128,128,255]));
    }
    #[test]
    fn ramps_conventions_and_inversion_have_correct_directions() {
        let mut p = Parameters::default();
        let source = gray(9,9,|x,y| ((x+y)*3000) as u16);
        let gl = normals(&height_field(&source,&p),&p);
        assert!(gl.get_pixel(4,4)[0] < 128); assert!(gl.get_pixel(4,4)[1] > 128);
        p.convention = "directx".into();
        let dx = normals(&height_field(&source,&p),&p);
        for (a,b) in gl.pixels().zip(dx.pixels()) { assert_eq!(a[0],b[0]); assert_eq!(a[2],b[2]); assert_eq!(a[1],255-b[1]); }
        p.convention = "opengl".into(); p.invert = true;
        let reversed = normals(&height_field(&source,&p),&p);
        assert!(reversed.get_pixel(4,4)[0] > 128); assert!(reversed.get_pixel(4,4)[1] < 128);
    }
    #[test]
    fn sixteen_bit_grayscale_roundtrips_without_gamma_or_normalization() {
        let source = gray(256,256,|x,y| (y*256+x) as u16);
        let result = height_image(&height_field(&source,&Parameters::default()),16);
        assert_eq!(source.as_luma16().unwrap(),result.as_luma16().unwrap());
        let p = Parameters { channel:"r".into(), ..Default::default() };
        let rgb = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1,1,Rgb([12345u16,20000,40000])));
        assert_eq!(height_image(&height_field(&rgb,&p),16).as_luma16().unwrap().get_pixel(0,0)[0],12345);
    }
    #[test]
    fn luminance_linearizes_color_but_explicit_channels_are_data() {
        let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(1,1,Rgb([128,128,128])));
        let p=Parameters::default();
        assert!((height_field(&source,&p).values[0]-0.21586).abs()<0.0001);
        let p=Parameters{channel:"r".into(),..p};
        assert!((height_field(&source,&p).values[0]-128.0/255.0).abs()<0.0001);
    }
    #[test]
    fn transparent_color_does_not_create_rims() {
        let source = DynamicImage::ImageRgba8(image::RgbaImage::from_fn(12,8,|x,_| if x<6 {image::Rgba([255,255,255,255])}else{image::Rgba([255,0,255,0])}));
        for smoothing in [0,3,20] {
            let p=Parameters{smoothing,..Default::default()}; let field=height_field(&source,&p);
            assert_eq!(field.values[10],0.5);
            assert!(normals(&field,&p).pixels().all(|p|p.0==[128,128,255]));
        }
        let source = DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1,1,image::Rgba([255,255,255,128])));
        assert!((height_field(&source,&Parameters::default()).values[0]-(0.5+0.5*128.0/255.0)).abs()<1e-6);
    }
    #[test]
    fn wrapping_samples_opposite_edge() {
        assert_eq!(coord(-1,5,true),4); assert_eq!(coord(5,5,true),0);
        let source=gray(5,1,|x,_| (x*12000) as u16);
        let p=Parameters::default(); let clamped=normals(&height_field(&source,&p),&p);
        let p=Parameters{boundary:"wrap".into(),..p};let wrapped=normals(&height_field(&source,&p),&p);
        assert!(clamped.get_pixel(0,0)[0]<128); assert!(wrapped.get_pixel(0,0)[0]>128);
    }
    #[test]
    fn invalid_parameters_are_rejected() {
        for value in [f32::NAN,f32::INFINITY,-1.0,11.0] { assert!(Parameters{strength:value,..Default::default()}.validate().is_err()); }
        assert!(Parameters{bits:12,..Default::default()}.validate().is_err());
        assert!(Parameters{smoothing:21,..Default::default()}.validate().is_err());
        assert!(crate::safety::memory_budget(u32::MAX,u32::MAX,64).is_err());
    }
    #[test]
    fn preflight_handles_final_names_and_input_conflicts() {
        let temp=area(); let root=temp.path();
        for name in ["hero.png","hero_basecolor.png","hero_normal.png"] { gray(2,2,|_,_|32768).save(root.join(name)).unwrap(); }
        let mut options=RunOptions { files:vec![root.join("hero.png").display().to_string(),root.join("hero_basecolor.png").display().to_string()],output_path:root.display().to_string(),kind:"normal".into(),parameters:Parameters::default() };
        assert!(output_plan(&options).unwrap_err().contains("重名"));
        options.files[1]=root.join("hero_normal.png").display().to_string();
        assert!(output_plan(&options).unwrap_err().contains("覆盖输入"));
        options.files=vec![root.join("hero_basecolor.png").display().to_string()];
        assert!(output_plan(&options).unwrap()[0][0].ends_with("hero_normal.png"));
    }
    #[test]
    fn generation_writes_sixteen_bit_height_and_pbr_named_normal() {
        let temp=area();let input=temp.path().join("tile_basecolor.png");
        gray(7,5,|x,y|(10000+x*700+y*300) as u16).save(&input).unwrap();
        let result=generate(None,RunOptions {files:vec![input.display().to_string()],output_path:temp.path().display().to_string(),kind:"normal".into(),parameters:Parameters{also_height:true,..Default::default()}}).unwrap();
        assert_eq!(result.completed,1);assert_eq!(result.outputs.len(),2);
        assert!(result.outputs.iter().any(|p| p.ends_with("tile_normal.png")));
        assert_eq!(image::open(temp.path().join("tile_height.png")).unwrap().color(),image::ColorType::L16);
        assert_eq!(image::open(temp.path().join("tile_normal.png")).unwrap().color(),image::ColorType::Rgb8);
    }
    #[test]
    #[ignore = "manual real-material verification within test area"]
    fn real_material_export() {
        let root=Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("测试区");
        let input=root.join("测试用图片/142101839_p0.jpg");
        let out=root.join("AB测试结果/13_material_maps");
        let result=generate(None,RunOptions{files:vec![input.display().to_string()],output_path:out.display().to_string(),kind:"normal".into(),parameters:Parameters{also_height:true,strength:3.0,..Default::default()}}).unwrap();
        assert_eq!(result.completed,1);
        let preview=preview(&input.display().to_string(),&Parameters{strength:3.0,..Default::default()}).unwrap();
        assert!(preview.width.max(preview.height_pixels)<=1024);
        std::fs::write(out.join("preview.json"),serde_json::to_vec(&preview).unwrap()).unwrap();
        let source=decode(&input).unwrap().resize(1024,1024,image::imageops::FilterType::Triangle);
        let params=Parameters{strength:3.0,..Default::default()};let field=height_field(&source,&params);
        source.save(out.join("preview-source.png")).unwrap();
        normals(&field,&params).save(out.join("preview-normal.png")).unwrap();
        height_image(&field,8).save(out.join("preview-height.png")).unwrap();
        // Existing PBR grouping requires the other three maps. Verify the normal
        // suffix by generating a self-contained fixture group alongside outputs.
        let group=out.join("pbr-integration");std::fs::create_dir_all(&group).unwrap();
        for name in ["tile_basecolor.png","tile_roughness.png","tile_metallic.png"] {
            RgbImage::from_pixel(4,4,Rgb([128,128,255])).save(group.join(name)).unwrap();
        }
        std::fs::copy(out.join("142101839_p0_normal.png"),group.join("tile_normal.png")).unwrap();
        let groups=crate::find_texture_groups(&group).unwrap();
        assert_eq!(groups.len(),1);
        assert_eq!(groups[0].prefix,"tile");
        assert!(groups[0].files.normal.ends_with("tile_normal.png"));
        assert_eq!(image::open(out.join("142101839_p0_height.png")).unwrap().color(),image::ColorType::L16);
        assert_eq!(image::open(out.join("142101839_p0_normal.png")).unwrap().dimensions(),(1676,2000));
        println!("outputs={:?}",result.outputs);
    }
}

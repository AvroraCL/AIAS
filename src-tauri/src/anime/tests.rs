#[cfg(test)]
mod toonout_tests {
    use image::imageops::FilterType;
    use image::{ImageBuffer, Luma, RgbImage, Rgba, RgbaImage};
    use ort::session::{builder::GraphOptimizationLevel, Session};
    use ort::value::{Tensor, ValueType};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter};
    use crate::anime::*;

    #[test]
    fn probability_luma_applies_the_requested_confidence_threshold() {
        assert_eq!(
            probability_luma(&[0.0, 0.25, 0.5, 1.0], 0.3),
            [0, 0, 255, 255]
        );
    }

    #[test]
    fn small_foreground_islands_are_removed_without_eroding_the_subject() {
        let mut matte = vec![0.0; 36];
        for index in [7, 8, 13, 14, 15] {
            matte[index] = 1.0;
        }
        matte[35] = 1.0;

        remove_small_foreground_components(&mut matte, 6, 6, 2);

        assert_eq!(matte[35], 0.0, "单像素孤岛应被移除");
        for index in [7, 8, 13, 14, 15] {
            assert_eq!(matte[index], 1.0, "主体连通块不能被侵蚀");
        }
    }

    #[test]
    fn background_islands_are_removed_only_when_small_and_far_from_the_subject() {
        // 44×8：左上 8×8 主体（64px，2% 阈值 = 面积 1 的块才可删）。
        let mut matte = vec![0.0; 44 * 8];
        let fill = |matte: &mut [f32], points: &[(usize, usize)]| {
            for (x, y) in points {
                matte[y * 44 + x] = 1.0;
            }
        };
        let main: Vec<(usize, usize)> = (0..8)
            .flat_map(|y| (0..8).map(move |x| (x, y)))
            .collect();
        fill(&mut matte, &main);
        fill(&mut matte, &[(42, 7)]); // 间距 34 > 32 且面积达标 → 移除
        fill(&mut matte, &[(10, 4)]); // 间距 2，紧贴 → 保留
        fill(&mut matte, &[(42, 0), (43, 0)]); // 间距 34 但面积超主件 2% → 保留

        remove_background_islands(&mut matte, 44, 8);

        assert_eq!(matte[7 * 44 + 42], 0.0, "远处单像素孤岛应被移除");
        assert_eq!(matte[4 * 44 + 10], 1.0, "紧贴主体的块不能被移除");
        assert_eq!(matte[44], 1.0, "面积超阈值的远处块不能被移除");
        for (x, y) in &main {
            assert_eq!(matte[y * 44 + x], 1.0, "主体不能被侵蚀");
        }
    }

    #[test]
    fn alpha_metrics_split_interior_boundary_and_exterior_errors() {
        let mut gt = RgbaImage::from_pixel(21, 21, image::Rgba([0, 0, 0, 0]));
        for y in 6..15 {
            for x in 6..15 {
                gt.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
            }
        }
        let mut result = gt.clone();
        result.get_pixel_mut(10, 10)[3] = 0; // 主体内部漏抠
        result.get_pixel_mut(6, 10)[3] = 0; // 轮廓带漏抠
        result.get_pixel_mut(0, 0)[3] = 255; // 远处背景残留

        let metrics = alpha_metrics(&gt, &result);

        assert!(metrics.interior_miss_mean > 0.0, "主体内部漏抠应被计入");
        assert!(metrics.boundary_mae > 0.0, "轮廓带漏抠应被计入");
        assert!(metrics.exterior_leak_mean > 0.0, "远处背景残留应被计入");
    }

    #[test]
    fn instance_gate_keeps_only_the_requested_chebyshev_radius() {
        let mut matte = vec![0.0; 7 * 7];
        matte[3 * 7 + 3] = 1.0;
        let gate = dilated_instance_gate(&matte, 7, 7, 2);

        assert!(gate[3 * 7 + 3], "实例中心必须保留");
        assert!(gate[1 * 7 + 1], "Chebyshev 距离 2 的像素必须保留");
        assert!(!gate[0], "Chebyshev 距离 3 的像素必须排除");
    }

    #[test]
    fn enclosed_background_holes_are_filled_without_touching_the_outer_background() {
        let mut matte = vec![1.0; 25];
        matte[12] = 0.0;
        matte[0] = 0.0;

        fill_small_background_holes(&mut matte, 5, 5, 2);

        assert_eq!(matte[12], 1.0, "被主体包围的小孔应被填充");
        assert_eq!(matte[0], 0.0, "与画面边缘相连的背景不能被填充");
    }

    #[test]
    fn toonout_complex_background_signal_catches_a_translucent_border_leak() {
        let (width, height) = (512, 512);
        let mut matte = vec![0.36; (width * height) as usize];
        for y in 24..height - 24 {
            for x in 24..width - 24 {
                matte[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(toonout_likely_failed(&matte, width, height));
    }

    #[test]
    fn toonout_complex_background_signal_also_catches_one_sided_leaks() {
        let (width, height) = (512, 512);
        let mut matte = vec![0.35; (width * height) as usize];
        // 前景约 53%，模拟背景只从一侧/四周漏进来而非整图误判。
        for y in 70..442 {
            for x in 70..442 {
                matte[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(toonout_likely_failed(&matte, width, height));
    }

    #[test]
    fn cleaner_fallback_accepts_a_clear_but_not_excessive_foreground_reduction() {
        let (width, height) = (512, 512);
        let mut toonout = vec![0.35; (width * height) as usize];
        let mut general = vec![0.05; (width * height) as usize];
        for y in 64..448 {
            for x in 64..448 {
                toonout[(y * width + x) as usize] = 1.0;
            }
        }
        for y in 72..440 {
            for x in 72..440 {
                general[(y * width + x) as usize] = 1.0;
            }
        }

        assert!(matte_is_substantially_cleaner(&general, &toonout, width, height));
    }

    #[test]
    fn main_subject_score_prefers_a_large_centered_candidate_over_a_tiny_high_confidence_one() {
        let centered = CharacterCandidate {
            confidence: 0.86,
            stride: 8,
            row: 0,
            col: 0,
            x1: 180.0,
            y1: 120.0,
            x2: 820.0,
            y2: 880.0,
        };
        let edge = CharacterCandidate {
            confidence: 0.99,
            stride: 8,
            row: 0,
            col: 0,
            x1: 0.0,
            y1: 40.0,
            x2: 240.0,
            y2: 360.0,
        };

        assert!(
            main_subject_score(centered, 1000, 1000) > main_subject_score(edge, 1000, 1000),
            "主体排序应让大且居中的候选胜过边缘小候选"
        );
    }

    #[test]
    fn toonout_preprocess_fills_the_entire_model_input() {
        let source = RgbImage::from_pixel(1447, 2036, image::Rgb([17, 49, 91]));
        let resized = resize_birefnet_input(&source, 1024, 1024);
        assert_eq!(resized.dimensions(), (1024, 1024));
        assert_eq!(*resized.get_pixel(0, 0), image::Rgb([17, 49, 91]));
        assert_eq!(*resized.get_pixel(512, 512), image::Rgb([17, 49, 91]));
        assert_eq!(*resized.get_pixel(1023, 1023), image::Rgb([17, 49, 91]));
    }

    #[test]
    fn general_catalog_is_the_verified_1024_profile() {
        let general = model_spec("birefnet-general").expect("General 模型必须已注册");
        let file = general.files.first().expect("General 必须有模型文件");

        assert_eq!(file.name, "birefnet-general-1024-fp16.onnx");
        assert_eq!(file.size, BIREFNET_GENERAL_1024_FP16_SIZE);
        assert!(model_uses_native_edge_alpha(general.id));
        assert!(!model_uses_native_edge_alpha("toonout"));
    }

    #[test]
    fn anime_specialist_catalog_is_the_verified_animeseg_profile() {
        let specialist = model_spec("anime-specialist").expect("AnimeSeg 模型必须已注册");
        let file = specialist.files.first().expect("AnimeSeg 必须有模型文件");

        assert_eq!(file.name, "birefnext-aniseg-int8-v0.1.onnx");
        assert_eq!(file.size, ANIME_SPECIALIST_SIZE);
        assert!(model_uses_native_edge_alpha(specialist.id));
        assert!(matches!(specialist.kind, ModelKind::BiRefNet { matting: false }));
    }

    #[test]
    fn horizontal_flip_mean_restores_the_original_pixel_order() {
        let direct = [1.0, 0.0, 0.2, 0.8];
        // flipped[0..2] 属于原图第二列，flipped[2..4] 属于原图第一列。
        let flipped = [0.4, 0.6, 0.7, 0.3];
        let combined = mean_with_horizontal_flip(&direct, &flipped, 2, 2);

        assert_eq!(combined, vec![0.8, 0.2, 0.25, 0.75]);
    }

    /// 端到端验证 ToonOut：下载 470MB 模型 + 真实推理，仅在手动运行：
    /// `cargo test toonout -- --ignored --nocapture`
    #[test]
    #[ignore = "下载并运行 470MB 模型，手动执行"]
    fn toonout_end_to_end() {
        let base = std::env::temp_dir().join("aias-toonout-e2e");
        fs::create_dir_all(&base).unwrap();
        download_model(None, &base, "toonout").unwrap();

        // 深蓝背景上一块亮橙色圆角主体，模型应保留主体、清除背景。
        let mut img = RgbImage::new(512, 384);
        for y in 0..384 {
            for x in 0..512 {
                img.put_pixel(x, y, image::Rgb([28, 34, 58]));
            }
        }
        for y in 96..288 {
            for x in 128..384 {
                img.put_pixel(x, y, image::Rgb([244, 150, 58]));
            }
        }
        let input = base.join("in.png");
        let output = base.join("out.png");
        img.save(&input).unwrap();

        cutout(&base, "toonout", &input, &output).unwrap();
        let out = image::open(&output).unwrap().to_rgba8();
        assert_eq!(out.dimensions(), (512, 384));

        let alpha = |x: u32, y: u32| out.get_pixel(x, y)[3];
        let center = alpha(256, 192);
        let corner = alpha(8, 8);
        assert!(center > 220, "主体中心应不透明，实际 {center}");
        assert!(corner < 60, "背景角应接近透明，实际 {corner}");
        println!("center={center} corner={corner}");
    }

    /// 用指定图片检查 ToonOut 质量：
    /// `AIAS_TOONOUT_TEST_IMAGE=/path/to/image cargo test toonout_real_image -- --ignored --nocapture`
    #[test]
    #[ignore = "运行 470MB 模型推理，手动执行"]
    fn toonout_real_image() {
        let base = std::env::temp_dir().join("aias-toonout-e2e");
        fs::create_dir_all(&base).unwrap();
        download_model(None, &base, "toonout").unwrap();

        let input_path = std::env::var_os("AIAS_TOONOUT_TEST_IMAGE")
            .map(PathBuf::from)
            .expect("请设置 AIAS_TOONOUT_TEST_IMAGE 指向测试图片");
        let rgb = image::open(&input_path).unwrap().to_rgb8();
        println!("input {}x{}", rgb.width(), rgb.height());

        ensure_ort_runtime(&base).unwrap();
        let path = models_dir(&base).join("birefnet-toonout-fp16.onnx");
        let session = build_session(&path, true).unwrap();
        println!("inputs: {:?}", session.inputs());
        println!("outputs: {:?}", session.outputs());

        let matte = run_birefnet(&base, "toonout", false, &rgb).unwrap();
        let (w, h) = rgb.dimensions();
        let (mut strong, mut weak, mut mid) = (0usize, 0usize, 0usize);
        let (mut border_sum, mut border_n) = (0f64, 0f64);
        for y in 0..h {
            for x in 0..w {
                let value = matte[y as usize * w as usize + x as usize];
                if value > 0.9 {
                    strong += 1
                } else if value < 0.1 {
                    weak += 1
                } else {
                    mid += 1
                }
                if x < 16 || y < 16 || x >= w - 16 || y >= h - 16 {
                    border_sum += f64::from(value);
                    border_n += 1.0;
                }
            }
        }
        let total = (w * h) as usize;
        println!(
            "strong={}% weak={}% mid={}% border_mean={:.4}",
            strong * 100 / total,
            weak * 100 / total,
            mid * 100 / total,
            border_sum / border_n
        );

        let output = base.join("out_real.png");
        cutout(&base, "toonout", &input_path, &output).unwrap();
        println!("saved {}", output.display());
    }

    /// A/B 基准：对 AIAS_AB_INPUT（图像或目录）跑指定模型；若同时给出
    /// AIAS_AB_GT，则以其 alpha 作为人工真值，输出可复核的指标和五联预览。
    /// 例如：
    /// `AIAS_AB_INPUT=F:\\...\\原图.png AIAS_AB_GT=F:\\...\\人工抠图版.png \
    ///   AIAS_AB_OUTPUT=F:\\...\\AB测试结果 AIAS_AB_TAG=P5_baseline \
    ///   cargo test --release anime::toonout_tests::ab_reference -- --ignored --exact --nocapture`
    /// 可用 `AIAS_AB_MODELS=advanced` 限定单模型，避免 A/B 验证被其他模型的
    /// 显存需求阻断；默认仍依次运行 toonout、simple、advanced。
    #[test]
    #[ignore = "手动执行：真实模型推理"]
    fn ab_reference() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input_path = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试"));
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file());
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "base".into());
        let selected_models: Vec<String> = std::env::var("AIAS_AB_MODELS")
            .unwrap_or_else(|_| "toonout,simple,advanced".into())
            .split(',')
            .map(str::trim)
            .filter(|model| !model.is_empty())
            .map(str::to_owned)
            .collect();
        assert!(
            !selected_models.is_empty(),
            "AIAS_AB_MODELS 至少需要指定一个模型"
        );
        for model in &selected_models {
            assert!(
                MODELS.iter().any(|spec| spec.id == model),
                "AIAS_AB_MODELS 不支持模型：{model}"
            );
        }
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab").join(&tag));
        fs::create_dir_all(&out_dir).unwrap();
        ensure_ort_runtime(&base).unwrap();

        let mut inputs = if input_path.is_file() {
            vec![input_path]
        } else {
            fs::read_dir(&input_path)
                .unwrap()
                .flatten()
                .map(|entry| entry.path())
                .collect::<Vec<_>>()
        };
        inputs.sort();
        let gt_alpha = gt_path.as_ref().map(|path| {
            image::open(path)
                .unwrap_or_else(|error| panic!("无法读取人工抠图真值 {}：{error}", path.display()))
                .to_rgba8()
        });
        let mut report = String::from(
            "AIAS alpha A/B report\n"
        );
        report.push_str(&format!("tag={tag}\n"));
        if let Some(path) = &gt_path {
            report.push_str(&format!("ground_truth={}\n", path.display()));
        }

        for path in inputs {
            let ext = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp") {
                continue;
            }
            if gt_path.as_ref().is_some_and(|gt| gt == &path) {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_string();
            let source = gt_alpha.as_ref().map(|_| {
                image::open(&path)
                    .unwrap_or_else(|error| panic!("无法读取原图 {}：{error}", path.display()))
                    .to_rgb8()
            });
            if let (Some(gt), Some(source)) = (&gt_alpha, &source) {
                assert_eq!(
                    gt.dimensions(),
                    source.dimensions(),
                    "人工真值与原图尺寸必须一致：{}",
                    path.display()
                );
                let alignment = source_gt_alignment(source, gt);
                let alignment_line = format!(
                    "{stem}\talignment\tsolid_pixels={}\trgb_mae={:.4}\trgb_changed_ratio={:.5}\n",
                    alignment.solid_pixels,
                    alignment.rgb_mae,
                    alignment.rgb_changed_ratio,
                );
                print!("{alignment_line}");
                report.push_str(&alignment_line);
            }
            for model in &selected_models {
                let started = std::time::Instant::now();
                let output = out_dir.join(format!("{stem}_{model}.png"));
                let outcome = cutout_with_fallback(&base, model, &path, &output)
                    .unwrap_or_else(|error| panic!("{model} {stem}: {error}"));
                println!(
                    "{tag} {} {} 耗时 {:?} 实际模型={} 回退={} -> {}",
                    stem,
                    model,
                    started.elapsed(),
                    outcome.model_used,
                    outcome.fallback,
                    output.display()
                );
                let result = image::open(&output).unwrap().to_rgba8();
                let alpha: Vec<f32> = result
                    .pixels()
                    .map(|pixel| pixel[3] as f32 / 255.0)
                    .collect();
                println!(
                    "{tag} {} {} 前景占比={:.3} 边缘均值={:.3}",
                    stem,
                    model,
                    foreground_ratio(&alpha, result.width(), result.height()),
                    border_band_mean(&alpha, result.width(), result.height(), 24)
                );
                let preview = out_dir.join(format!("{stem}_{model}_preview.jpg"));
                if let Some(gt) = &gt_alpha {
                    assert_eq!(
                        gt.dimensions(),
                        result.dimensions(),
                        "人工真值与输入尺寸必须一致：{}",
                        path.display()
                    );
                    let metrics = alpha_metrics(gt, &result);
                    let line = format!(
                        "{stem}\t{model}\tactual={}\tmae={:.5}\tbg_leak_mean={:.5}\tbg_leak_ratio={:.5}\tfg_miss_mean={:.5}\tfg_miss_ratio={:.5}\tedge_mae={:.5}\tiou={:.5}\n",
                        outcome.model_used,
                        metrics.mae,
                        metrics.background_leak_mean,
                        metrics.background_leak_ratio,
                        metrics.foreground_miss_mean,
                        metrics.foreground_miss_ratio,
                        metrics.edge_mae,
                        metrics.iou,
                    );
                    print!("{line}");
                    report.push_str(&line);
                    let region_line = format!(
                        "{stem}\t{model}\tregions\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
                        metrics.interior_miss_mean,
                        metrics.boundary_mae,
                        metrics.exterior_leak_mean,
                    );
                    print!("{region_line}");
                    report.push_str(&region_line);
                    if let Some(source) = &source {
                        let heatmap = out_dir.join(format!("{stem}_{model}_alpha_error.png"));
                        alpha_error_heatmap(source, gt, &result, &heatmap);
                    }
                    if std::env::var_os("AIAS_AB_DEBUG").is_some() {
                        let raw_path = output.with_file_name(format!("{stem}_{model}_matte_raw.png"));
                        if raw_path.is_file() {
                            let raw = image::open(&raw_path)
                                .expect("原始遮罩可读")
                                .to_luma8();
                            let raw_metrics = alpha_metrics_luma(gt, &raw);
                            let raw_line = format!(
                                "{stem}\t{model}\tstage=raw\tmae={:.5}\tbg_leak_mean={:.5}\tbg_leak_ratio={:.5}\tfg_miss_mean={:.5}\tfg_miss_ratio={:.5}\tedge_mae={:.5}\tiou={:.5}\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
                                raw_metrics.mae,
                                raw_metrics.background_leak_mean,
                                raw_metrics.background_leak_ratio,
                                raw_metrics.foreground_miss_mean,
                                raw_metrics.foreground_miss_ratio,
                                raw_metrics.edge_mae,
                                raw_metrics.iou,
                                raw_metrics.interior_miss_mean,
                                raw_metrics.boundary_mae,
                                raw_metrics.exterior_leak_mean,
                            );
                            print!("{raw_line}");
                            report.push_str(&raw_line);
                        }
                    }
                    ab_gt_preview(&path, gt, &result, &preview);
                } else {
                    ab_preview(&path, &result, &preview);
                }
            }
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("A/B 指标报告写出");
    }

    /// 开发期翻转增强对照：同一 General 1024 分别推理原图与水平翻转图，将后者
    /// 翻回后做均值融合。该方法不接入正式流程，先确认双倍推理时间是否换来
    /// 可量化的边缘收益，避免凭直觉牺牲批处理速度。
    #[test]
    #[ignore = "手动执行：General 1024 翻转增强对照"]
    fn ab_general_1024_flip_tta() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\flip-tta"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "flip-tta".into());
        fs::create_dir_all(&out_dir).expect("创建翻转增强输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped = image::imageops::flip_horizontal(&original);
        let flipped_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &flipped)
            .expect("翻转图 General 1024 推理");
        let (width, height) = (w as usize, h as usize);
        let mut restored_flipped_mask = vec![0.0; direct_mask.len()];
        for y in 0..height {
            for x in 0..width {
                restored_flipped_mask[y * width + x] = flipped_mask[y * width + (width - 1 - x)];
            }
        }
        let blend = |flipped_weight: f32| {
            direct_mask
                .iter()
                .zip(restored_flipped_mask.iter())
                .map(|(direct, mirrored)| direct * (1.0 - flipped_weight) + mirrored * flipped_weight)
                .collect::<Vec<f32>>()
        };
        let tta_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);
        let flip_quarter_mask = blend(0.25);
        let flip_sixty_mask = blend(0.60);
        let flip_three_quarter_mask = blend(0.75);
        let flip_ninety_mask = blend(0.90);
        let flip_only_mask = restored_flipped_mask;
        println!("{tag} flip TTA inference elapsed {:?}", started.elapsed());

        let direct = finalize_cutout_image(&original, direct_mask, true);
        let tta = finalize_cutout_image(&original, tta_mask, true);
        let flip_quarter = finalize_cutout_image(&original, flip_quarter_mask, true);
        let flip_sixty = finalize_cutout_image(&original, flip_sixty_mask, true);
        let flip_three_quarter = finalize_cutout_image(&original, flip_three_quarter_mask, true);
        let flip_ninety = finalize_cutout_image(&original, flip_ninety_mask, true);
        let flip_only = finalize_cutout_image(&original, flip_only_mask, true);
        let direct_label = "birefnet-general-1024-direct";
        let flip_quarter_label = "birefnet-general-1024-flip-025";
        let tta_label = "birefnet-general-1024-flip-mean";
        let flip_sixty_label = "birefnet-general-1024-flip-060";
        let flip_three_quarter_label = "birefnet-general-1024-flip-075";
        let flip_ninety_label = "birefnet-general-1024-flip-090";
        let flip_only_label = "birefnet-general-1024-flip-only";
        for (label, result) in [
            (direct_label, &direct),
            (flip_quarter_label, &flip_quarter),
            (tta_label, &tta),
            (flip_sixty_label, &flip_sixty),
            (flip_three_quarter_label, &flip_three_quarter),
            (flip_ninety_label, &flip_ninety),
            (flip_only_label, &flip_only),
        ] {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("翻转增强结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
        }
        let mut report = format!(
            "AIAS General 1024 flip TTA A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=direct vs 25% / 50% / 60% / 75% / 90% / 100% horizontal-flip alpha blend; development only\n",
            input.display(),
            gt_path.display(),
        );
        append_instance_metrics(&mut report, direct_label, &gt, &direct);
        append_instance_metrics(&mut report, flip_quarter_label, &gt, &flip_quarter);
        append_instance_metrics(&mut report, tta_label, &gt, &tta);
        append_instance_metrics(&mut report, flip_sixty_label, &gt, &flip_sixty);
        append_instance_metrics(&mut report, flip_three_quarter_label, &gt, &flip_three_quarter);
        append_instance_metrics(&mut report, flip_ninety_label, &gt, &flip_ninety);
        append_instance_metrics(&mut report, flip_only_label, &gt, &flip_only);
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("翻转增强指标报告写出");
    }

    /// 开发期方向增强复核：在已经验证有效的左右翻转之外，额外测试上下翻转和
    /// 四向均值。动漫人物通常具有明显的竖直朝向，上下翻转可能反而破坏语义；
    /// 因此这里只产生 A/B 证据，绝不在未胜出的情况下增加正式处理耗时。
    #[test]
    #[ignore = "手动执行：General 1024 方向增强对照"]
    fn ab_general_1024_orientation_tta() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\orientation-tta"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "orientation-tta".into());
        fs::create_dir_all(&out_dir).expect("创建方向增强输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let restore = |mask: &[f32], flip_x: bool, flip_y: bool| {
            let (width, height) = (w as usize, h as usize);
            assert_eq!(mask.len(), width * height, "翻转 alpha 尺寸无效");
            let mut restored = vec![0.0; mask.len()];
            for y in 0..height {
                for x in 0..width {
                    let source_x = if flip_x { width - 1 - x } else { x };
                    let source_y = if flip_y { height - 1 - y } else { y };
                    restored[y * width + x] = mask[source_y * width + source_x];
                }
            }
            restored
        };
        let average = |masks: &[&[f32]]| {
            assert!(!masks.is_empty(), "至少需要一张 alpha");
            let mut merged = vec![0.0; masks[0].len()];
            for mask in masks {
                assert_eq!(mask.len(), merged.len(), "alpha 尺寸必须一致");
                for (target, value) in merged.iter_mut().zip(mask.iter()) {
                    *target += value;
                }
            }
            let divisor = masks.len() as f32;
            for value in &mut merged {
                *value /= divisor;
            }
            merged
        };

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let horizontal_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_horizontal(&original),
            )
            .expect("左右翻转 General 1024 推理"),
            true,
            false,
        );
        let vertical_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_vertical(&original),
            )
            .expect("上下翻转 General 1024 推理"),
            false,
            true,
        );
        let both_mask = restore(
            &run_birefnet_single_for_test(
                &base,
                "birefnet-general",
                false,
                &image::imageops::flip_vertical(&image::imageops::flip_horizontal(&original)),
            )
            .expect("双向翻转 General 1024 推理"),
            true,
            true,
        );
        println!("{tag} orientation TTA inference elapsed {:?}", started.elapsed());

        // 先完成所有融合，随后才把 direct_mask 移交给正式后处理。
        let horizontal_mean = average(&[&direct_mask, &horizontal_mask]);
        let vertical_mean = average(&[&direct_mask, &vertical_mask]);
        let four_way_mean = average(&[&direct_mask, &horizontal_mask, &vertical_mask, &both_mask]);
        let direct = finalize_cutout_image(&original, direct_mask, true);
        let horizontal = finalize_cutout_image(&original, horizontal_mean, true);
        let vertical = finalize_cutout_image(&original, vertical_mean, true);
        let four_way = finalize_cutout_image(&original, four_way_mean, true);
        let results = [
            ("birefnet-general-1024-direct", &direct),
            ("birefnet-general-1024-horizontal-mean", &horizontal),
            ("birefnet-general-1024-vertical-mean", &vertical),
            ("birefnet-general-1024-four-way-mean", &four_way),
        ];
        let mut report = format!(
            "AIAS General 1024 orientation TTA A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=direct vs horizontal vs vertical vs four-way mean; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("方向增强结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("方向增强指标报告写出");
    }

    /// 开发期边缘收缩对照：人工真值显示当前残差主要落在外轮廓，故以已经胜出
    /// 的水平翻转 alpha 为共同输入，比较温和的 alpha 曲线和激进的 1px 最小值
    /// 腐蚀。没有跨图/真值回归支持时，任何一种都不得进入正式链路。
    #[test]
    #[ignore = "手动执行：General 1024 边缘收缩对照"]
    fn ab_general_1024_edge_contract() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\edge-contract"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "edge-contract".into());
        fs::create_dir_all(&out_dir).expect("创建边缘收缩输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped_mask = run_birefnet_single_for_test(
            &base,
            "birefnet-general",
            false,
            &image::imageops::flip_horizontal(&original),
        )
        .expect("翻转图 General 1024 推理");
        let tta_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);
        println!("{tag} edge-contract inference elapsed {:?}", started.elapsed());

        let (width, height) = (w as usize, h as usize);
        let mut min3_mask = vec![0.0; tta_mask.len()];
        for y in 0..height {
            for x in 0..width {
                let mut minimum = 1.0f32;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        minimum = minimum.min(tta_mask[yy * width + xx]);
                    }
                }
                min3_mask[y * width + x] = minimum;
            }
        }

        let baseline = finalize_cutout_image_with_alpha_gamma(&original, tta_mask.clone(), true, 1.0);
        let gamma_110 = finalize_cutout_image_with_alpha_gamma(&original, tta_mask.clone(), true, 1.10);
        let gamma_125 = finalize_cutout_image_with_alpha_gamma(&original, tta_mask, true, 1.25);
        let min3 = finalize_cutout_image_with_alpha_gamma(&original, min3_mask, true, 1.0);
        // `finalize_cutout_image` 的正式阈值发生在同一后处理阶段；这里在最终
        // alpha 上模拟更高阈值，先快速测出主角完整度和背景幽灵的取舍，再决定
        // 是否值得调整正式阈值。后续组件清理按 >0.5 工作，对这些候选无额外影响。
        let raise_alpha_floor = |source: &RgbaImage, floor: f32| {
            let mut result = source.clone();
            for pixel in result.pixels_mut() {
                if pixel[3] as f32 / 255.0 < floor {
                    pixel[3] = 0;
                }
            }
            result
        };
        let floor_020 = raise_alpha_floor(&baseline, 0.20);
        let floor_030 = raise_alpha_floor(&baseline, 0.30);
        let floor_040 = raise_alpha_floor(&baseline, 0.40);
        let floor_050 = raise_alpha_floor(&baseline, 0.50);
        let results = [
            ("birefnet-general-1024-horizontal-baseline", &baseline),
            ("birefnet-general-1024-horizontal-gamma-110", &gamma_110),
            ("birefnet-general-1024-horizontal-gamma-125", &gamma_125),
            ("birefnet-general-1024-horizontal-min3", &min3),
            ("birefnet-general-1024-horizontal-floor-020", &floor_020),
            ("birefnet-general-1024-horizontal-floor-030", &floor_030),
            ("birefnet-general-1024-horizontal-floor-040", &floor_040),
            ("birefnet-general-1024-horizontal-floor-050", &floor_050),
        ];
        let mut report = format!(
            "AIAS General 1024 edge contract A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=horizontal TTA baseline vs alpha gamma vs 1px min erosion vs alpha floors; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("边缘收缩结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("边缘收缩指标报告写出");
    }

    /// 对已胜出的 AnimeSeg 专精模型单独校准轮廓。General 的边缘收缩结论不能
    /// 外推给它：两者的残差形态不同，必须在修正后的人工真值上独立比较。
    #[test]
    #[ignore = "手动执行：AnimeSeg 边缘收缩对照"]
    fn ab_anime_specialist_edge_contract() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\anime-specialist-edge-contract"));
        let tag = std::env::var("AIAS_AB_TAG")
            .unwrap_or_else(|_| "anime-specialist-edge-contract".into());
        fs::create_dir_all(&out_dir).expect("创建边缘收缩输出目录");
        assert!(
            is_model_ready(&base, "anime-specialist"),
            "测试需要 AnimeSeg 专精模型"
        );

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let mask = run_birefnet(&base, "anime-specialist", false, &original)
            .expect("AnimeSeg 专精模型推理");
        println!("{tag} edge-contract inference elapsed {:?}", started.elapsed());

        let (width, height) = (w as usize, h as usize);
        let mut min3_mask = vec![0.0; mask.len()];
        for y in 0..height {
            for x in 0..width {
                let mut minimum = 1.0f32;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        minimum = minimum.min(mask[yy * width + xx]);
                    }
                }
                min3_mask[y * width + x] = minimum;
            }
        }

        let baseline = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.0);
        let gamma_102 = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.02);
        let gamma_105 = finalize_cutout_image_with_alpha_gamma(&original, mask.clone(), true, 1.05);
        let gamma_110 = finalize_cutout_image_with_alpha_gamma(&original, mask, true, 1.10);
        let min3 = finalize_cutout_image_with_alpha_gamma(&original, min3_mask, true, 1.0);
        let raise_alpha_floor = |source: &RgbaImage, floor: f32| {
            let mut result = source.clone();
            for pixel in result.pixels_mut() {
                if pixel[3] as f32 / 255.0 < floor {
                    pixel[3] = 0;
                }
            }
            result
        };
        let floor_010 = raise_alpha_floor(&baseline, 0.10);
        let floor_020 = raise_alpha_floor(&baseline, 0.20);
        let results = [
            ("anime-specialist-baseline", &baseline),
            ("anime-specialist-gamma-102", &gamma_102),
            ("anime-specialist-gamma-105", &gamma_105),
            ("anime-specialist-gamma-110", &gamma_110),
            ("anime-specialist-min3", &min3),
            ("anime-specialist-floor-010", &floor_010),
            ("anime-specialist-floor-020", &floor_020),
        ];
        let mut report = format!(
            "AIAS AnimeSeg edge contract A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=single-pass baseline vs gentle alpha gamma vs 1px min erosion vs alpha floors; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("边缘收缩结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("边缘收缩指标报告写出");
    }

    /// 开发期高分辨率复核：全图 1024 擅长理解「哪个才是主体」，重叠分块则能
    /// 以更高的有效像素密度看轮廓。这里故意同时输出纯分块和 50% 融合版；
    /// 若分块带来背景误检，指标会直接暴露，不能仅凭局部发丝看起来更锐利就上线。
    #[test]
    #[ignore = "手动执行：General 1024 重叠分块对照"]
    fn ab_general_1024_tiled_refine() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\tiled-refine"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "tiled-refine".into());
        fs::create_dir_all(&out_dir).expect("创建分块复核输出目录");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 General 1024 模型");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let starts = |length: u32, side: u32, stride: u32| {
            if length <= side {
                return vec![0];
            }
            let mut positions = vec![0];
            let last = length - side;
            while *positions.last().expect("至少一个分块起点") + stride < last {
                positions.push(*positions.last().expect("分块起点") + stride);
            }
            if *positions.last().expect("分块起点") != last {
                positions.push(last);
            }
            positions
        };

        // 2048px 原图块送入固定 1024 输入，提供约 2 倍于全图的局部有效密度；
        // 512px 重叠配合 256px 线性羽化，避免直接拼接出现接缝。
        const TILE: u32 = 2048;
        const STRIDE: u32 = 1536;
        const FEATHER: u32 = 256;
        let xs = starts(w, TILE, STRIDE);
        let ys = starts(h, TILE, STRIDE);
        let started = std::time::Instant::now();
        let direct_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &original)
            .expect("原图 General 1024 推理");
        let flipped_mask = run_birefnet_single_for_test(
            &base,
            "birefnet-general",
            false,
            &image::imageops::flip_horizontal(&original),
        )
        .expect("翻转图 General 1024 推理");
        let global_mask = mean_with_horizontal_flip(&direct_mask, &flipped_mask, w, h);

        let mut tiled_sum = vec![0.0f32; (w * h) as usize];
        let mut tiled_weight = vec![0.0f32; (w * h) as usize];
        for &top in &ys {
            for &left in &xs {
                let tile_w = TILE.min(w - left);
                let tile_h = TILE.min(h - top);
                let tile = image::imageops::crop_imm(&original, left, top, tile_w, tile_h).to_image();
                let tile_mask = run_birefnet_single_for_test(&base, "birefnet-general", false, &tile)
                    .expect("分块 General 1024 推理");
                for y in 0..tile_h {
                    let fy = if top > 0 && y < FEATHER {
                        y as f32 / FEATHER as f32
                    } else if top + tile_h < h && tile_h - 1 - y < FEATHER {
                        (tile_h - 1 - y) as f32 / FEATHER as f32
                    } else {
                        1.0
                    };
                    for x in 0..tile_w {
                        let fx = if left > 0 && x < FEATHER {
                            x as f32 / FEATHER as f32
                        } else if left + tile_w < w && tile_w - 1 - x < FEATHER {
                            (tile_w - 1 - x) as f32 / FEATHER as f32
                        } else {
                            1.0
                        };
                        let index = ((top + y) * w + left + x) as usize;
                        let weight = fx * fy;
                        tiled_sum[index] += tile_mask[(y * tile_w + x) as usize] * weight;
                        tiled_weight[index] += weight;
                    }
                }
            }
        }
        let tiled_mask: Vec<f32> = tiled_sum
            .iter()
            .zip(tiled_weight.iter())
            .map(|(sum, weight)| if *weight > 0.0 { sum / weight } else { 0.0 })
            .collect();
        println!(
            "{tag} tiled refine inference elapsed {:?}; grid={}x{}",
            started.elapsed(),
            xs.len(),
            ys.len()
        );

        let blended_mask: Vec<f32> = global_mask
            .iter()
            .zip(tiled_mask.iter())
            .map(|(global, tiled)| (global + tiled) * 0.5)
            .collect();
        let global = finalize_cutout_image(&original, global_mask, true);
        let tiled = finalize_cutout_image(&original, tiled_mask, true);
        let blended = finalize_cutout_image(&original, blended_mask, true);
        let results = [
            ("birefnet-general-1024-horizontal-global", &global),
            ("birefnet-general-1024-tiled", &tiled),
            ("birefnet-general-1024-global-tiled-mean", &blended),
        ];
        let mut report = format!(
            "AIAS General 1024 tiled refine A/B report\ntag={tag}\ninput={}\nground_truth={}\nstrategy=global horizontal TTA vs 2048px overlapping tiles vs 50% alpha mean; development only\n",
            input.display(),
            gt_path.display(),
        );
        for (label, result) in results {
            result
                .save(out_dir.join(format!("{stem}_{label}.png")))
                .expect("分块复核结果写出");
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
            append_instance_metrics(&mut report, label, &gt, result);
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("分块复核指标报告写出");
    }

    /// 开发期本地官方导出验证：不注册、不下载、不暴露到正式 UI。将同一张原图
    /// 分别跑当前正式 general 完整管线和本机 HR 导出的原始 alpha，确认 HR 的
    /// 实际输入尺寸、显存可行性与遮罩质量后，才决定是否值得进入后续正式实验。
    #[test]
    #[ignore = "手动执行：本地 HR BiRefNet 对照"]
    fn ab_local_birefnet_reference() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\local-hr"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "local-hr".into());
        let local_label = std::env::var("AIAS_AB_LOCAL_LABEL").unwrap_or_else(|_| "hr".into());
        assert!(
            !local_label.is_empty()
                && local_label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')),
            "AIAS_AB_LOCAL_LABEL 只能包含 ASCII 字母、数字、-、_"
        );
        let model_path = std::env::var_os("AIAS_AB_LOCAL_MODEL")
            .map(PathBuf::from)
            .unwrap_or_else(|| models_dir(&base).join("BiRefNet_HR-general-epoch_130.onnx"));
        assert!(model_path.is_file(), "本地 HR 模型不可读：{}", model_path.display());
        fs::create_dir_all(&out_dir).expect("创建 HR 对照输出目录");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        let (w, h) = original.dimensions();
        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");

        let started = std::time::Instant::now();
        let hr_alpha = run_birefnet_local_file(
            &base,
            &format!("ab-local-{local_label}"),
            &model_path,
            true,
            false,
            &original,
        )
        .unwrap_or_else(|error| panic!("本地 HR BiRefNet 推理失败：{error}"));
        println!("{tag} local HR inference elapsed {:?}", started.elapsed());
        assert_eq!(hr_alpha.len(), (w * h) as usize, "HR alpha 尺寸无效");

        let mut hr_result = RgbaImage::new(w, h);
        let mut hr_matte = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, (pixel, matte_pixel)) in hr_result
            .pixels_mut()
            .zip(hr_matte.pixels_mut())
            .enumerate()
        {
            let color = original.get_pixel((index as u32) % w, (index as u32) / w);
            let alpha = (hr_alpha[index] * 255.0).round().clamp(0.0, 255.0) as u8;
            *pixel = Rgba([color[0], color[1], color[2], alpha]);
            *matte_pixel = image::Luma([alpha]);
        }
        let local_result_label = format!("birefnet-{local_label}_raw");
        let hr_path = out_dir.join(format!("{stem}_{local_result_label}.png"));
        hr_result.save(&hr_path).expect("HR 原始 alpha 写出");
        hr_matte
            .save(out_dir.join(format!("{stem}_{local_result_label}_matte.png")))
            .expect("HR alpha 遮罩写出");
        let local_final_label = format!("birefnet-{local_label}_final");
        // 本开发用例的本地导出均为 1024 输入，使用与正式 General 1024 相同的
        // 原生边缘策略，避免再引入低分辨率模型才需要的引导滤波。
        let local_final = finalize_cutout_image(&original, hr_alpha, true);
        local_final
            .save(out_dir.join(format!("{stem}_{local_final_label}.png")))
            .expect("本地模型正式后处理结果写出");

        // 同目录重跑当前正式结果，让本地模型与完整正式管线可独立复核。
        let general_path = out_dir.join(format!("{stem}_birefnet-general_final.png"));
        cutout_with_fallback(&base, "birefnet-general", &input, &general_path)
            .expect("正式 general 参照推理");
        let general = image::open(&general_path).expect("正式 general 可读").to_rgba8();

        let alignment = source_gt_alignment(&original, &gt);
        let mut report = format!(
            "AIAS local BiRefNet A/B report\ntag={tag}\ninput={}\nground_truth={}\nlocal_model={}\ncomparison=local raw/final alpha vs current general final pipeline\nalignment_rgb_mae={:.4}\nalignment_changed_ratio={:.5}\n",
            input.display(),
            gt_path.display(),
            model_path.display(),
            alignment.rgb_mae,
            alignment.rgb_changed_ratio,
        );
        append_instance_metrics(&mut report, "birefnet-general_final", &gt, &general);
        append_instance_metrics(
            &mut report,
            &local_result_label,
            &gt,
            &hr_result,
        );
        append_instance_metrics(&mut report, &local_final_label, &gt, &local_final);
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("HR 对照指标报告写出");

        for (label, result) in [
            ("birefnet-general_final", &general),
            (local_result_label.as_str(), &hr_result),
            (local_final_label.as_str(), &local_final),
        ] {
            ab_gt_preview(
                &input,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                result,
                &out_dir.join(format!("{stem}_{label}_alpha_error.png")),
            );
        }
    }

    /// 开发期原型：固定 BiRefNet general 的 alpha，只用经「主角重排序」后的
    /// 动漫实例遮罩限制其有效范围。`clean` 与 `preserve` 只改变同一实例遮罩的
    /// 膨胀半径，借此量化背景残留和主体误删的取舍；不接入正式 UI 或输出流程。
    #[test]
    #[ignore = "手动执行：主角实例约束 A/B 原型"]
    fn ab_instance_constraint() {
        assert!(
            ab_main_subject_selector_enabled(),
            "请设置 AIAS_AB_INSTANCE_SELECTOR=main-subject"
        );
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        let input = std::env::var_os("AIAS_AB_INPUT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_INPUT 必须指向一张原图");
        let gt_path = std::env::var_os("AIAS_AB_GT")
            .map(PathBuf::from)
            .filter(|path| path.is_file())
            .expect("AIAS_AB_GT 必须指向人工 alpha 真值");
        let out_dir = std::env::var_os("AIAS_AB_OUTPUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"F:\AIAS\ab\instance-constraint"));
        let tag = std::env::var("AIAS_AB_TAG").unwrap_or_else(|_| "instance-constraint".into());
        let clean_radius = ab_env_usize("AIAS_AB_INSTANCE_CLEAN_RADIUS", 48);
        let preserve_radius = ab_env_usize("AIAS_AB_INSTANCE_PRESERVE_RADIUS", 192);
        let primary_component_only = std::env::var("AIAS_AB_INSTANCE_COMPONENT")
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("primary"));
        let radii: Vec<(String, usize)> = std::env::var("AIAS_AB_INSTANCE_RADII")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|part| part.trim().parse::<usize>().ok())
                    .filter(|radius| *radius <= u16::MAX as usize - 1)
                    .map(|radius| (format!("r{radius}"), radius))
                    .collect()
            })
            .filter(|values: &Vec<(String, usize)>| !values.is_empty())
            .unwrap_or_else(|| {
                vec![
                    ("clean".to_string(), clean_radius),
                    ("preserve".to_string(), preserve_radius),
                ]
            });
        let adaptive: Vec<(usize, usize, f32)> = std::env::var("AIAS_AB_INSTANCE_ADAPTIVE")
            .ok()
            .map(|value| {
                value
                    .split(',')
                    .filter_map(|part| {
                        let mut values = part.trim().split(':');
                        let inner = values.next()?.parse::<usize>().ok()?;
                        let outer = values.next()?.parse::<usize>().ok()?;
                        let confidence = values.next()?.parse::<f32>().ok()?;
                        (inner <= outer
                            && outer <= u16::MAX as usize - 1
                            && (0.0..=1.0).contains(&confidence))
                            .then_some((inner, outer, confidence))
                    })
                    .collect()
            })
            .unwrap_or_default();
        fs::create_dir_all(&out_dir).expect("创建实例约束 A/B 输出目录");

        let original = image::open(&input).expect("原图可读").to_rgb8();
        let gt = image::open(&gt_path).expect("人工真值可读").to_rgba8();
        assert_eq!(original.dimensions(), gt.dimensions(), "原图与 GT 尺寸必须一致");
        assert!(is_model_ready(&base, "birefnet-general"), "测试需要 BiRefNet general");
        assert!(is_model_ready(&base, "advanced"), "测试需要动漫精细模型");

        let stem = input.file_stem().and_then(|value| value.to_str()).unwrap_or("image");
        let general_path = out_dir.join(format!("{stem}_instance_general.png"));
        cutout_with_fallback(&base, "birefnet-general", &input, &general_path)
            .expect("通用高质量 alpha 推理");
        let general = image::open(&general_path).expect("通用结果可读").to_rgba8();
        let raw_instance = run_advanced(&base, &original).expect("主角实例推理");
        let (w, h) = original.dimensions();
        let instance = if primary_component_only {
            main_instance_component(&raw_instance, w, h)
        } else {
            raw_instance.clone()
        };
        let mut instance_mask = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
        for (index, pixel) in instance_mask.pixels_mut().enumerate() {
            *pixel = image::Luma([(instance[index] * 255.0).round().clamp(0.0, 255.0) as u8]);
        }
        instance_mask
            .save(out_dir.join(format!("{stem}_instance_mask.png")))
            .expect("实例遮罩写出");

        let alignment = source_gt_alignment(&original, &gt);
        let mut report = format!(
            "AIAS instance-constraint A/B report\ntag={tag}\ninput={}\nground_truth={}\nselector=main-subject\nprimary_component_only={primary_component_only}\nradii={}\nadaptive={}\nalignment_rgb_mae={:.4}\nalignment_changed_ratio={:.5}\n",
            input.display(),
            gt_path.display(),
            radii.iter().map(|(_, radius)| radius.to_string()).collect::<Vec<_>>().join(","),
            adaptive.iter().map(|(inner, outer, threshold)| format!("{inner}:{outer}:{threshold:.2}")).collect::<Vec<_>>().join(","),
            alignment.rgb_mae,
            alignment.rgb_changed_ratio,
        );
        append_instance_metrics(&mut report, "general", &gt, &general);
        for (label, radius) in &radii {
            let gate = dilated_instance_gate(&instance, w, h, *radius);
            let mut gate_image = ImageBuffer::<Luma<u8>, Vec<u8>>::new(w, h);
            for (pixel, keep) in gate_image.pixels_mut().zip(gate.iter()) {
                *pixel = image::Luma([if *keep { 255 } else { 0 }]);
            }
            gate_image
                .save(out_dir.join(format!("{stem}_instance_gate_{label}.png")))
                .expect("实例约束写出");
            let constrained = apply_instance_gate(&general, &gate);
            let result_path = out_dir.join(format!("{stem}_instance_{label}.png"));
            constrained.save(&result_path).expect("实例约束结果写出");
            append_instance_metrics(&mut report, label, &gt, &constrained);
            ab_gt_preview(
                &input,
                &gt,
                &constrained,
                &out_dir.join(format!("{stem}_instance_{label}_preview.jpg")),
            );
            alpha_error_heatmap(
                &original,
                &gt,
                &constrained,
                &out_dir.join(format!("{stem}_instance_{label}_alpha_error.png")),
            );
        }
        if let Some(max_outer) = adaptive.iter().map(|(_, outer, _)| *outer).max() {
            let distance = instance_distance(&instance, w, h, max_outer);
            for (inner, outer, threshold) in adaptive {
                let label = format!(
                    "adaptive_i{inner}_o{outer}_t{:02}",
                    (threshold * 100.0).round() as u8
                );
                let constrained = apply_adaptive_instance_gate(
                    &general,
                    &distance,
                    inner,
                    outer,
                    threshold,
                );
                constrained
                    .save(out_dir.join(format!("{stem}_instance_{label}.png")))
                    .expect("自适应实例约束结果写出");
                append_instance_metrics(&mut report, &label, &gt, &constrained);
                ab_gt_preview(
                    &input,
                    &gt,
                    &constrained,
                    &out_dir.join(format!("{stem}_instance_{label}_preview.jpg")),
                );
                alpha_error_heatmap(
                    &original,
                    &gt,
                    &constrained,
                    &out_dir.join(format!("{stem}_instance_{label}_alpha_error.png")),
                );
            }
        }
        fs::write(out_dir.join(format!("{tag}_metrics.txt")), report)
            .expect("实例约束指标报告写出");
    }

    fn ab_env_usize(name: &str, default: usize) -> usize {
        std::env::var(name)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0 && *value <= u16::MAX as usize - 1)
            .unwrap_or(default)
    }

    /// 选中实例的 Chebyshev 膨胀：`true` 是允许 BiRefNet alpha 保留的区域。
    fn dilated_instance_gate(matte: &[f32], width: u32, height: u32, radius: usize) -> Vec<bool> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h);
        let limit = (radius + 1) as u16;
        let mut distance: Vec<u16> = matte
            .iter()
            .map(|alpha| if *alpha > 0.5 { 0 } else { limit })
            .collect();
        let step = |value: u16| value.saturating_add(1).min(limit);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance.into_iter().map(|value| value <= radius as u16).collect()
    }

    fn apply_instance_gate(general: &RgbaImage, gate: &[bool]) -> RgbaImage {
        assert_eq!(general.width() as usize * general.height() as usize, gate.len());
        let mut constrained = general.clone();
        for (pixel, keep) in constrained.pixels_mut().zip(gate) {
            if !*keep {
                pixel[3] = 0;
            }
        }
        constrained
    }

    /// 计算像素到实例实心区域的 Chebyshev 距离；超过 `limit` 的值饱和，便于
    /// 同一张距离图派生多种门控半径。与 `dilated_instance_gate` 的距离定义一致。
    fn instance_distance(matte: &[f32], width: u32, height: u32, limit: usize) -> Vec<u16> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h);
        let limit = (limit + 1) as u16;
        let mut distance: Vec<u16> = matte
            .iter()
            .map(|alpha| if *alpha > 0.5 { 0 } else { limit })
            .collect();
        let step = |value: u16| value.saturating_add(1).min(limit);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance
    }

    /// 近区完全信任主实例门控；远区只允许高置信度 General alpha 通过。它是
    /// 为长发/衣摆等离实例粗遮罩较远的细结构预留的受控通道，绝不会无边界地
    /// 放回整个背景。
    fn apply_adaptive_instance_gate(
        general: &RgbaImage,
        distance: &[u16],
        inner: usize,
        outer: usize,
        alpha_threshold: f32,
    ) -> RgbaImage {
        assert_eq!(general.width() as usize * general.height() as usize, distance.len());
        let mut constrained = general.clone();
        for (pixel, distance) in constrained.pixels_mut().zip(distance) {
            let keep = *distance as usize <= inner
                || (*distance as usize <= outer
                    && pixel[3] as f32 / 255.0 >= alpha_threshold);
            if !keep {
                pixel[3] = 0;
            }
        }
        constrained
    }

    /// 只留下实例遮罩中面积最大、也最靠近画面中心的连通块。RTMDet 已经选中
    /// 中心角色，但其 refinement mask 偶尔仍会在远处花瓶或装饰处溢出独立小块；
    /// 用这一步作为“门控的门控”，不会直接写入最终 alpha，细发丝仍由后续
    /// 膨胀范围内的 BiRefNet alpha 决定。
    fn main_instance_component(matte: &[f32], width: u32, height: u32) -> Vec<f32> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(matte.len(), w * h, "实例遮罩尺寸无效");
        let mut labels = vec![u32::MAX; matte.len()];
        let mut components: Vec<(usize, usize, usize, usize, usize)> = Vec::new();
        let mut stack = Vec::new();
        for start in 0..matte.len() {
            if matte[start] <= 0.5 || labels[start] != u32::MAX {
                continue;
            }
            let component = components.len() as u32;
            labels[start] = component;
            stack.push(start);
            let (mut count, mut min_x, mut min_y, mut max_x, mut max_y) = (0usize, w, h, 0usize, 0usize);
            while let Some(index) = stack.pop() {
                let (x, y) = (index % w, index / w);
                count += 1;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
                for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                        let next = yy * w + xx;
                        if matte[next] > 0.5 && labels[next] == u32::MAX {
                            labels[next] = component;
                            stack.push(next);
                        }
                    }
                }
            }
            components.push((count, min_x, min_y, max_x, max_y));
        }
        let Some((best, _)) = components.iter().enumerate().max_by(|(_, left), (_, right)| {
            let score = |component: &(usize, usize, usize, usize, usize)| {
                let (count, min_x, min_y, max_x, max_y) = *component;
                let area = count as f32 / (w * h).max(1) as f32;
                let cx = (min_x + max_x) as f32 * 0.5 / w.max(1) as f32;
                let cy = (min_y + max_y) as f32 * 0.5 / h.max(1) as f32;
                let center = 1.0 - ((cx - 0.5).powi(2) + (cy - 0.5).powi(2)).sqrt() / 0.707_106_77;
                area.sqrt() * 0.75 + center.clamp(0.0, 1.0) * 0.25
            };
            score(left).total_cmp(&score(right))
        }) else {
            return vec![0.0; matte.len()];
        };
        matte
            .iter()
            .zip(labels.iter())
            .map(|(alpha, label)| if *label == best as u32 { *alpha } else { 0.0 })
            .collect()
    }

    fn append_instance_metrics(report: &mut String, label: &str, gt: &RgbaImage, result: &RgbaImage) {
        let metrics = alpha_metrics(gt, result);
        report.push_str(&format!(
            "{label}\tmae={:.5}\tiou={:.5}\tinterior_miss_mean={:.5}\tboundary_mae={:.5}\texterior_leak_mean={:.5}\n",
            metrics.mae,
            metrics.iou,
            metrics.interior_miss_mean,
            metrics.boundary_mae,
            metrics.exterior_leak_mean,
        ));
    }

    /// 与人工 alpha 真值的可解释差异：背景残留、主体漏抠、半透明边缘和硬遮罩 IoU。
    #[derive(Debug, Clone, Copy)]
    struct AlphaMetrics {
        mae: f32,
        background_leak_mean: f32,
        background_leak_ratio: f32,
        foreground_miss_mean: f32,
        foreground_miss_ratio: f32,
        edge_mae: f32,
        iou: f32,
        interior_miss_mean: f32,
        boundary_mae: f32,
        exterior_leak_mean: f32,
    }

    #[derive(Debug, Clone, Copy)]
    struct AlignmentMetrics {
        solid_pixels: usize,
        rgb_mae: f32,
        rgb_changed_ratio: f32,
    }

    /// 验证人工版是否仍与原图同一像素网格。只读取 alpha ≥ 0.98 的区域，
    /// 避免透明背景色和边缘去污染影响判断；这里报告而非强制断言，因为
    /// 人工版有可能有正当的前景修色。
    fn source_gt_alignment(source: &RgbImage, gt: &RgbaImage) -> AlignmentMetrics {
        assert_eq!(source.dimensions(), gt.dimensions());
        let mut total = 0u64;
        let mut changed = 0usize;
        let mut pixels = 0usize;
        for (source, truth) in source.pixels().zip(gt.pixels()) {
            if truth[3] < 250 {
                continue;
            }
            pixels += 1;
            let mut delta = 0u32;
            for channel in 0..3 {
                delta += (source[channel] as i16 - truth[channel] as i16).unsigned_abs() as u32;
            }
            total += delta as u64;
            if delta > 12 {
                changed += 1;
            }
        }
        AlignmentMetrics {
            solid_pixels: pixels,
            rgb_mae: total as f32 / (pixels.max(1) * 3) as f32,
            rgb_changed_ratio: changed as f32 / pixels.max(1) as f32,
        }
    }

    fn alpha_metrics(gt: &RgbaImage, result: &RgbaImage) -> AlphaMetrics {
        assert_eq!(gt.dimensions(), result.dimensions());
        let alpha: Vec<u8> = result.pixels().map(|pixel| pixel[3]).collect();
        alpha_metrics_bytes(gt, &alpha)
    }

    fn alpha_metrics_luma(gt: &RgbaImage, result: &image::GrayImage) -> AlphaMetrics {
        assert_eq!(gt.dimensions(), result.dimensions());
        alpha_metrics_bytes(gt, result.as_raw())
    }

    fn alpha_metrics_bytes(gt: &RgbaImage, predicted_alpha: &[u8]) -> AlphaMetrics {
        assert_eq!(gt.width() as usize * gt.height() as usize, predicted_alpha.len());
        let (width, height) = gt.dimensions();
        let solid: Vec<bool> = gt.pixels().map(|pixel| pixel[3] > 127).collect();
        let dist_to_foreground = chebyshev_distance_clamped(&solid, width, height, 4);
        let background: Vec<bool> = solid.iter().map(|value| !value).collect();
        let dist_to_background = chebyshev_distance_clamped(&background, width, height, 4);
        let mut total_error = 0.0f64;
        let mut bg_alpha = 0.0f64;
        let mut bg_count = 0usize;
        let mut bg_leak_count = 0usize;
        let mut fg_loss = 0.0f64;
        let mut fg_count = 0usize;
        let mut fg_miss_count = 0usize;
        let mut edge_error = 0.0f64;
        let mut edge_count = 0usize;
        let mut intersection = 0usize;
        let mut union = 0usize;
        let mut interior_loss = 0.0f64;
        let mut interior_count = 0usize;
        let mut boundary_error = 0.0f64;
        let mut boundary_count = 0usize;
        let mut exterior_alpha = 0.0f64;
        let mut exterior_count = 0usize;

        for (index, (truth, predicted)) in gt.pixels().zip(predicted_alpha).enumerate() {
            let (truth, predicted) = (truth[3] as f32 / 255.0, *predicted as f32 / 255.0);
            total_error += f64::from((truth - predicted).abs());
            if truth <= 0.02 {
                bg_count += 1;
                bg_alpha += f64::from(predicted);
                if predicted > 0.10 {
                    bg_leak_count += 1;
                }
            }
            if truth >= 0.98 {
                fg_count += 1;
                fg_loss += f64::from(1.0 - predicted);
                if predicted < 0.50 {
                    fg_miss_count += 1;
                }
            }
            if (0.02..0.98).contains(&truth) {
                edge_count += 1;
                edge_error += f64::from((truth - predicted).abs());
            }
            let truth_solid = truth > 0.5;
            let predicted_solid = predicted > 0.5;
            if truth_solid || predicted_solid {
                union += 1;
            }
            if truth_solid && predicted_solid {
                intersection += 1;
            }
            // 空间区域不取决于 GT 是否有抗锯齿 alpha：轮廓带是二值轮廓的两侧各 3px。
            if truth >= 0.98 && dist_to_background[index] > 3 {
                interior_count += 1;
                interior_loss += f64::from(1.0 - predicted);
            }
            if dist_to_foreground[index] <= 3 && dist_to_background[index] <= 3 {
                boundary_count += 1;
                boundary_error += f64::from((truth - predicted).abs());
            }
            if truth <= 0.02 && dist_to_foreground[index] > 3 {
                exterior_count += 1;
                exterior_alpha += f64::from(predicted);
            }
        }
        let n = (gt.width() as usize * gt.height() as usize).max(1) as f64;
        AlphaMetrics {
            mae: (total_error / n) as f32,
            background_leak_mean: (bg_alpha / bg_count.max(1) as f64) as f32,
            background_leak_ratio: bg_leak_count as f32 / bg_count.max(1) as f32,
            foreground_miss_mean: (fg_loss / fg_count.max(1) as f64) as f32,
            foreground_miss_ratio: fg_miss_count as f32 / fg_count.max(1) as f32,
            edge_mae: (edge_error / edge_count.max(1) as f64) as f32,
            iou: intersection as f32 / union.max(1) as f32,
            interior_miss_mean: (interior_loss / interior_count.max(1) as f64) as f32,
            boundary_mae: (boundary_error / boundary_count.max(1) as f64) as f32,
            exterior_leak_mean: (exterior_alpha / exterior_count.max(1) as f64) as f32,
        }
    }

    /// 到最近 seed 的 8 邻域（Chebyshev）距离，最大只需区分到 4px 以外。
    fn chebyshev_distance_clamped(seeds: &[bool], width: u32, height: u32, max: u8) -> Vec<u8> {
        let (w, h) = (width as usize, height as usize);
        assert_eq!(seeds.len(), w * h);
        let mut distance: Vec<u8> = seeds
            .iter()
            .map(|seed| if *seed { 0 } else { max })
            .collect();
        let step = |value: u8| value.saturating_add(1).min(max);
        for y in 0..h {
            for x in 0..w {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x > 0 {
                    best = best.min(step(distance[index - 1]));
                }
                if y > 0 {
                    best = best.min(step(distance[index - w]));
                    if x > 0 {
                        best = best.min(step(distance[index - w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index - w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        for y in (0..h).rev() {
            for x in (0..w).rev() {
                let index = y * w + x;
                if distance[index] == 0 {
                    continue;
                }
                let mut best = distance[index];
                if x + 1 < w {
                    best = best.min(step(distance[index + 1]));
                }
                if y + 1 < h {
                    best = best.min(step(distance[index + w]));
                    if x > 0 {
                        best = best.min(step(distance[index + w - 1]));
                    }
                    if x + 1 < w {
                        best = best.min(step(distance[index + w + 1]));
                    }
                }
                distance[index] = best;
            }
        }
        distance
    }

    /// 误差热图：红色表示 AI 多保留（背景残留），蓝色表示 AI 漏抠（主体缺失）。
    /// 原图降亮度作底，误差强度与 alpha 差成比例，可直接定位需要优化的空间区域。
    fn alpha_error_heatmap(source: &RgbImage, gt: &RgbaImage, result: &RgbaImage, out: &Path) {
        assert_eq!(source.dimensions(), gt.dimensions());
        assert_eq!(gt.dimensions(), result.dimensions());
        let mut heatmap = RgbImage::new(source.width(), source.height());
        for (((source, truth), predicted), target) in source
            .pixels()
            .zip(gt.pixels())
            .zip(result.pixels())
            .zip(heatmap.pixels_mut())
        {
            let delta = predicted[3] as f32 / 255.0 - truth[3] as f32 / 255.0;
            let base = [
                (source[0] as f32 * 0.22).round(),
                (source[1] as f32 * 0.22).round(),
                (source[2] as f32 * 0.22).round(),
            ];
            let intensity = ((delta.abs() - 0.05) / 0.95).clamp(0.0, 1.0);
            let tint = if delta >= 0.0 { [255.0, 40.0, 40.0] } else { [45.0, 130.0, 255.0] };
            *target = image::Rgb([
                (base[0] * (1.0 - intensity) + tint[0] * intensity).round() as u8,
                (base[1] * (1.0 - intensity) + tint[1] * intensity).round() as u8,
                (base[2] * (1.0 - intensity) + tint[2] * intensity).round() as u8,
            ]);
        }
        heatmap.save(out).expect("alpha 误差热图写出");
    }

    /// 横向拼三联图：原图 | 抠图白底合成 | 抠图深底合成，高度压到 1200 内。
    fn ab_preview(input: &Path, result: &RgbaImage, out: &Path) {
        let (w, h) = result.dimensions();
        let scale = (1200.0 / h as f32).min(1.0);
        let (tw, th) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let original = image::open(input)
            .expect("原图可读")
            .to_rgb8();
        let o_small = image::imageops::resize(&original, tw, th, FilterType::Triangle);
        let r_small = image::imageops::resize(result, tw, th, FilterType::Triangle);
        let over_white = ab_composite(&r_small, [255.0, 255.0, 255.0]);
        let over_dark = ab_composite(&r_small, [38.0, 42.0, 50.0]);
        let gap = 8;
        let mut canvas = RgbImage::new(tw * 3 + gap * 2, th);
        image::imageops::overlay(&mut canvas, &o_small, 0, 0);
        image::imageops::overlay(&mut canvas, &over_white, (tw + gap) as i64, 0);
        image::imageops::overlay(&mut canvas, &over_dark, ((tw + gap) * 2) as i64, 0);
        canvas.save(out).expect("预览图写出");
    }

    /// GT 五联预览：原图 | 人工抠图（白/深） | 当前结果（白/深）。
    fn ab_gt_preview(input: &Path, gt: &RgbaImage, result: &RgbaImage, out: &Path) {
        let (w, h) = result.dimensions();
        let scale = (1200.0 / h as f32).min(1.0);
        let (tw, th) = (
            ((w as f32 * scale).round() as u32).max(1),
            ((h as f32 * scale).round() as u32).max(1),
        );
        let original = image::open(input).expect("原图可读").to_rgb8();
        let o_small = image::imageops::resize(&original, tw, th, FilterType::Triangle);
        let gt_small = image::imageops::resize(gt, tw, th, FilterType::Triangle);
        let result_small = image::imageops::resize(result, tw, th, FilterType::Triangle);
        let gt_white = ab_composite(&gt_small, [255.0, 255.0, 255.0]);
        let gt_dark = ab_composite(&gt_small, [38.0, 42.0, 50.0]);
        let result_white = ab_composite(&result_small, [255.0, 255.0, 255.0]);
        let result_dark = ab_composite(&result_small, [38.0, 42.0, 50.0]);
        let gap = 8;
        let mut canvas = RgbImage::new(tw * 5 + gap * 4, th);
        for (index, panel) in [o_small, gt_white, gt_dark, result_white, result_dark]
            .into_iter()
            .enumerate()
        {
            image::imageops::overlay(&mut canvas, &panel, (index as u32 * (tw + gap)) as i64, 0);
        }
        canvas.save(out).expect("GT 预览图写出");
    }

    fn ab_composite(result: &RgbaImage, bg: [f32; 3]) -> RgbImage {
        let (tw, th) = result.dimensions();
        let mut composite = RgbImage::new(tw, th);
        for y in 0..th {
            for x in 0..tw {
                let pixel = result.get_pixel(x, y);
                let a = pixel[3] as f32 / 255.0;
                composite.put_pixel(
                    x,
                    y,
                    image::Rgb([
                        (pixel[0] as f32 * a + bg[0] * (1.0 - a)).round() as u8,
                        (pixel[1] as f32 * a + bg[1] * (1.0 - a)).round() as u8,
                        (pixel[2] as f32 * a + bg[2] * (1.0 - a)).round() as u8,
                    ]),
                );
            }
        }
        composite
    }

    /// 手动下载并安装 GPU 版 onnxruntime：
    /// `cargo test gpu_ort_install_manual -- --ignored --nocapture`
    /// 完成后运行 `cuda_ep_diagnostic` 验证 CUDA 是否真正生效。
    #[test]
    #[ignore = "手动执行：下载约 233MB 运行库"]
    fn gpu_ort_install_manual() {
        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        install_gpu_ort(None, &base).unwrap();
        assert!(gpu_ort_ready(&base));
        println!("installed at {}", gpu_ort_capi_dir(&base).display());
    }

    /// 诊断 CUDA EP 是否真正生效（会话日志 + 推理耗时 + 进程模块核对）：
    /// `cargo test cuda_ep_diagnostic -- --ignored --nocapture`
    /// 运行期间可用以下命令核对本进程加载的 CUDA 模块：
    /// `powershell "Get-Process -Id <pid> -Module | ? { $_.ModuleName -match 'cuda|cudnn|onnxruntime' } | select ModuleName,FileName"`
    #[test]
    #[ignore = "手动执行：需要本机已装抠图模型"]
    fn cuda_ep_diagnostic() {
        use std::sync::Arc;
        ort::init()
            .with_name("aias-cuda-diag")
            .with_logger(Arc::new(|level, category, _id, code_location, message| {
                println!("[ORT {:?}] {} {} {}", level, category, code_location, message);
            }))
            .commit();

        let base = dirs::home_dir()
            .expect("无法定位用户目录")
            .join(r"AppData\Roaming\studio.avroracl.aias");
        ensure_ort_runtime(&base).unwrap();
        let model = models_dir(&base).join("isnetis.onnx");
        assert!(model.exists(), "测试需要已安装 isnetis.onnx");

        println!("pid={}", std::process::id());
        let compiled_with_cuda = ort::ep::CUDA::default().is_available();
        println!("ORT 编译时包含 CUDA EP：{compiled_with_cuda:?}");
        let started = std::time::Instant::now();
        let session = build_session(&model, true).unwrap();
        println!("session built in {:?}", started.elapsed());
        drop(session);

        let mut img = RgbImage::new(1024, 1024);
        for y in 0..1024 {
            for x in 0..1024 {
                let inside = (128..896).contains(&x) && (128..896).contains(&y);
                img.put_pixel(x, y, if inside { image::Rgb([244, 150, 58]) } else { image::Rgb([28, 34, 58]) });
            }
        }
        let input = base.join("cuda-diag-in.png");
        let output = base.join("cuda-diag-out.png");
        img.save(&input).unwrap();

        cutout(&base, "simple", &input, &output).unwrap(); // 预热（含 cuDNN 选核）
        use ort::ep::ExecutionProvider;
        println!(
            "预热后 CUDA EP 编译状态：{:?}（true 表示当前加载的是 GPU 版运行库）",
            ort::ep::CUDA::default().is_available()
        );
        for run in 1..=3 {
            let started = std::time::Instant::now();
            cutout(&base, "simple", &input, &output).unwrap();
            println!("run {run}: {:?}", started.elapsed());
        }

        println!("===== 15 秒内核对进程模块（命令见测试注释）=====");
        std::thread::sleep(std::time::Duration::from_secs(15));
    }
}

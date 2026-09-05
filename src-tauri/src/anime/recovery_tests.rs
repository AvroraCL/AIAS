//! Development-only recovery probes and regression checks for the optional detail recovery.
use super::*;
use image::{RgbImage, Rgba, RgbaImage};

#[test]
fn recovery_requires_agreement_and_never_erases_existing_alpha() {
    assert!(0.8f32.min(0.1) < 0.60);
    assert_eq!(recovery_alpha(0.9, 0.8, 0.7, 1.0), 0.9);
    assert_eq!(recovery_alpha(0.0, 0.8, 0.7, 0.0), 0.0);
    assert!((recovery_alpha(0.1, 0.8, 0.7, 0.5) - 0.4).abs() < 1e-6);
    assert_eq!(recovery_upper_roi(&[0.0; 100], 10, 10, 55), (0, 0, 10, 10));
}

fn composite(img: &RgbaImage, bg: u8) -> RgbImage {
    RgbImage::from_fn(img.width(), img.height(), |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as f32 / 255.0;
        image::Rgb(std::array::from_fn(|c| {
            (p[c] as f32 * a + bg as f32 * (1.0 - a)).round() as u8
        }))
    })
}

#[test]
#[ignore = "Manual P107: locate ground-truth improvements without feeding GT into inference"]
fn ab_recovery_reference_diagnostics() {
    let root = PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅");
    let out = root.join("AB测试结果\\P107_focused_context_recovery");
    let source = image::open(root.join("测试\\原图.png")).unwrap().to_rgb8();
    let gt = image::open(root.join("测试\\人工抠图版.png"))
        .unwrap()
        .to_rgba8();
    let baseline = image::open(out.join("原图_A_baseline.png"))
        .unwrap()
        .to_rgba8();
    let candidate = image::open(out.join("原图_B_recovery_original_rgb.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(source.dimensions(), gt.dimensions());
    assert_eq!(gt.dimensions(), baseline.dimensions());
    assert_eq!(baseline.dimensions(), candidate.dimensions());
    let (w, h) = source.dimensions();
    let mut overlay = RgbImage::new(w, h);
    let mut truth_recovered = RgbImage::new(w, h);
    let mut good = 0u64;
    let mut bad = 0u64;
    let mut unchanged = 0u64;
    let mut good_mass = 0u64;
    let mut bad_mass = 0u64;
    let mut box_good = (w, h, 0, 0);
    let mut box_bad = (w, h, 0, 0);
    let mut cells = [(0u64, 0u64); 16];
    for y in 0..h {
        for x in 0..w {
            let src = source.get_pixel(x, y);
            let truth = gt.get_pixel(x, y);
            let old = baseline.get_pixel(x, y);
            let new = candidate.get_pixel(x, y);
            let delta = new[3].saturating_sub(old[3]) as u64;
            assert!(new[3] >= old[3], "add-only recovery must not erase alpha");
            let dim = image::Rgb([src[0] / 3, src[1] / 3, src[2] / 3]);
            if delta == 0 {
                overlay.put_pixel(x, y, dim);
                truth_recovered.put_pixel(x, y, image::Rgb([0, 0, 0]));
                continue;
            }
            let before = u8::abs_diff(old[3], truth[3]) as u64;
            let after = u8::abs_diff(new[3], truth[3]) as u64;
            let cell = ((y * 4 / h) * 4 + x * 4 / w) as usize;
            if after < before {
                good += 1;
                good_mass += before - after;
                cells[cell].0 += 1;
                box_good.0 = box_good.0.min(x);
                box_good.1 = box_good.1.min(y);
                box_good.2 = box_good.2.max(x + 1);
                box_good.3 = box_good.3.max(y + 1);
                overlay.put_pixel(x, y, image::Rgb([0, 255, 80]));
                truth_recovered.put_pixel(x, y, image::Rgb([truth[3], truth[3], truth[3]]));
            } else if after > before {
                bad += 1;
                bad_mass += after - before;
                cells[cell].1 += 1;
                box_bad.0 = box_bad.0.min(x);
                box_bad.1 = box_bad.1.min(y);
                box_bad.2 = box_bad.2.max(x + 1);
                box_bad.3 = box_bad.3.max(y + 1);
                overlay.put_pixel(x, y, image::Rgb([255, 45, 40]));
                truth_recovered.put_pixel(x, y, image::Rgb([0, 0, 0]));
            } else {
                unchanged += 1;
                overlay.put_pixel(x, y, image::Rgb([255, 220, 0]));
                truth_recovered.put_pixel(x, y, image::Rgb([0, 0, 0]));
            }
        }
    }
    overlay
        .save(out.join("原图_change_truth_overlay_green_improve_red_regress.png"))
        .unwrap();
    truth_recovered
        .save(out.join("原图_change_pixels_ground_truth_alpha.png"))
        .unwrap();
    let mut report=format!("P107 diagnostic; GT used only after inference.\nchanged_improve_pixels={good}\nchanged_regress_pixels={bad}\nchanged_equal_error_pixels={unchanged}\nerror_reduction_u8_sum={good_mass}\nerror_increase_u8_sum={bad_mass}\nimprove_bounds={box_good:?}\nregress_bounds={box_bad:?}\n4x4_grid: each entry=(improve_pixels,regress_pixels), rows top-to-bottom\n");
    for row in cells.chunks(4) {
        report.push_str(&format!("{row:?}\n"));
    }
    fs::write(out.join("reference_change_diagnostic.txt"), report).unwrap();
}

#[test]
#[ignore = "Manual P109: threshold-only recovery ablation using saved P107 masks"]
fn ab_recovery_agreement_threshold() {
    let root = PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅");
    let p107 = root.join("AB测试结果\\P107_focused_context_recovery");
    let out = root.join("AB测试结果\\P109_recovery_threshold_ablation");
    fs::create_dir_all(&out).unwrap();
    let rgb = image::open(root.join("测试\\原图.png")).unwrap().to_rgb8();
    let gt = image::open(root.join("测试\\人工抠图版.png"))
        .unwrap()
        .to_rgba8();
    let baseline = image::open(p107.join("原图_A_baseline.png"))
        .unwrap()
        .to_rgba8();
    let (w, h) = rgb.dimensions();
    let alpha: Vec<f32> = baseline.pixels().map(|p| p[3] as f32 / 255.0).collect();
    let roi1 = recovery_upper_roi(&alpha, w, h, 55);
    let roi2 = recovery_upper_roi(&alpha, w, h, 75);
    let local1 = image::open(p107.join("原图_crop55_mask.png"))
        .unwrap()
        .to_luma8();
    let local2 = image::open(p107.join("原图_crop75_mask.png"))
        .unwrap()
        .to_luma8();
    assert_eq!(local1.dimensions(), (roi1.2, roi1.3));
    assert_eq!(local2.dimensions(), (roi2.2, roi2.3));
    let distance = distance_to_foreground(&alpha, w, h, 97);
    let mut report=format!("P109: exactly P107 focused crops/masks; only min(local55,local75) agreement threshold varies. GT is evaluation-only.\nroi55={roi1:?}\nroi75={roi2:?}\n");
    super::tests::toonout_tests::append_instance_metrics(&mut report, "baseline", &gt, &baseline);
    for threshold in [0.50f32, 0.60, 0.70, 0.80, 0.90] {
        let mut candidate = alpha.clone();
        let mut changed = 0usize;
        for y in roi1.1..roi1.1 + roi1.3 {
            for x in roi1.0..roi1.0 + roi1.2 {
                let i = (y * w + x) as usize;
                let d = distance[i] as f32;
                if d >= 96.0 || alpha[i] >= 0.98 {
                    continue;
                }
                let first = local1.get_pixel(x - roi1.0, y - roi1.1)[0] as f32 / 255.0;
                let second = local2.get_pixel(x - roi2.0, y - roi2.1)[0] as f32 / 255.0;
                if first.min(second) < threshold {
                    continue;
                }
                let edge = (x - roi1.0)
                    .min(roi1.0 + roi1.2 - 1 - x)
                    .min(y - roi1.1)
                    .min(roi1.1 + roi1.3 - 1 - y);
                let weight =
                    ((96.0 - d) / 32.0).clamp(0.0, 1.0) * (edge as f32 / 64.0).clamp(0.0, 1.0);
                let next = recovery_alpha(alpha[i], first, second, weight);
                changed += usize::from((next * 255.0).round() as u8 != baseline.get_pixel(x, y)[3]);
                candidate[i] = next;
            }
        }
        let result = RgbaImage::from_fn(w, h, |x, y| {
            let p = rgb.get_pixel(x, y);
            Rgba([
                p[0],
                p[1],
                p[2],
                (candidate[(y * w + x) as usize] * 255.0).round() as u8,
            ])
        });
        let label = format!("threshold_{:.0}", threshold * 100.0);
        report.push_str(&format!("{label}\tchanged_alpha_pixels={changed}\t"));
        super::tests::toonout_tests::append_instance_metrics(&mut report, &label, &gt, &result);
        result
            .save(out.join(format!("原图_{label}_original_rgb.png")))
            .unwrap();
    }
    fs::write(out.join("report.txt"), report).unwrap();
}

#[test]
#[ignore = "Manual P111: run the exact optional production detail-recovery path against the maintained reference"]
fn ab_production_detail_recovery_path() {
    let root = PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅");
    let inputs = root.join("测试");
    let results = root.join("AB测试结果");
    let out = results.join("P111_production_detail_recovery");
    fs::create_dir_all(&out).unwrap();

    let base = dirs::data_dir().unwrap().join("studio.avroracl.aias");
    let rgb = image::open(inputs.join("原图.png")).unwrap().to_rgb8();
    let baseline =
        image::open(results.join("P77_production_animeseg_closed_form\\原图_anime-specialist.png"))
            .unwrap()
            .to_rgba8();
    assert_eq!(rgb.dimensions(), baseline.dimensions());

    let started = std::time::Instant::now();
    let candidate = recover_anime_specialist_details_rgba(&base, &rgb, baseline.clone()).unwrap();
    let elapsed_ms = started.elapsed().as_millis();
    let mut changed = 0usize;
    for (before, after) in baseline.pixels().zip(candidate.pixels()) {
        assert!(
            after[3] >= before[3],
            "production recovery must be add-only"
        );
        changed += usize::from(after[3] != before[3]);
    }
    assert!(
        changed > 0,
        "reference fixture should exercise the recovery path"
    );
    let truth = image::open(inputs.join("人工抠图版.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(truth.dimensions(), baseline.dimensions());

    baseline.save(out.join("原图_A_baseline.png")).unwrap();
    candidate
        .save(out.join("原图_B_production_detail_recovery.png"))
        .unwrap();
    let preview_height = 900;
    let preview_width =
        (rgb.width() as f64 / rgb.height() as f64 * preview_height as f64).round() as u32;
    let left = image::imageops::resize(
        &composite(&baseline, 235),
        preview_width,
        preview_height,
        image::imageops::FilterType::Lanczos3,
    );
    let right = image::imageops::resize(
        &composite(&candidate, 235),
        preview_width,
        preview_height,
        image::imageops::FilterType::Lanczos3,
    );
    let mut preview = RgbImage::from_pixel(
        preview_width * 2 + 8,
        preview_height,
        image::Rgb([48, 48, 52]),
    );
    image::imageops::overlay(&mut preview, &left, 0, 0);
    image::imageops::overlay(&mut preview, &right, (preview_width + 8) as i64, 0);
    preview
        .save(out.join("原图_A基线_vs_B细节补全_灰底对比.png"))
        .unwrap();
    let mut report = format!(
        "P111: exact shipped optional detail-recovery function; no ground truth is read until inference is complete.\nlocal_inference_and_composite_ms={elapsed_ms}\nchanged_alpha_pixels={changed}\n"
    );
    super::tests::toonout_tests::append_instance_metrics(
        &mut report,
        "baseline",
        &truth,
        &baseline,
    );
    super::tests::toonout_tests::append_instance_metrics(
        &mut report,
        "production_detail_recovery",
        &truth,
        &candidate,
    );
    fs::write(out.join("report.txt"), report).unwrap();
    fs::write(
        out.join("说明.txt"),
        "A 是当前正式 AnimeSeg 输出；B 是同一正式输出启用“高分辨率细节补全”后的结果。\n\
两者仅在主体上半部、距离既有前景不超过 96 像素的边缘附近可能不同；并且两个不同上下文的局部模型都需给出至少 0.60 的前景置信度，才允许增加 alpha。\n\
人工抠图版不参与裁切、推理或合成，只在生成 B 后用于 report.txt 中的客观评分。\n\
灰底对比图左 A、右 B；它方便观察轮廓，但不代表 B 必然适用于每张图，因此软件中该开关默认关闭。\n",
    )
    .unwrap();
    release_birefnet_session("anime-specialist");
}

#[test]
#[ignore = "Manual P112: test whether a General BiRefNet agreement gate reduces detail-recovery regressions"]
fn ab_detail_recovery_model_disagreement_gate() {
    let root = PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅");
    let inputs = root.join("测试");
    let results = root.join("AB测试结果");
    let out = results.join("P112_model_disagreement_gate");
    fs::create_dir_all(&out).unwrap();

    let base = dirs::data_dir().unwrap().join("studio.avroracl.aias");
    assert!(
        is_model_ready(&base, "birefnet-general"),
        "P112 requires the separately installed General BiRefNet model"
    );
    let rgb = image::open(inputs.join("原图.png")).unwrap().to_rgb8();
    let baseline =
        image::open(results.join("P77_production_animeseg_closed_form\\原图_anime-specialist.png"))
            .unwrap()
            .to_rgba8();
    let (width, height) = rgb.dimensions();
    assert_eq!(baseline.dimensions(), (width, height));
    let base_alpha: Vec<f32> = baseline
        .pixels()
        .map(|pixel| pixel[3] as f32 / 255.0)
        .collect();
    let inner = recovery_upper_roi(&base_alpha, width, height, 55);
    let outer = recovery_upper_roi(&base_alpha, width, height, 75);

    // All predictions and proposal gates are computed before the manual alpha
    // is opened. The second model merely vetoes an AnimeSeg proposal; it never
    // supplies alpha or selects a crop.
    let started = std::time::Instant::now();
    let inner_rgb = image::imageops::crop_imm(&rgb, inner.0, inner.1, inner.2, inner.3).to_image();
    let anime_inner = run_birefnet(&base, "anime-specialist", false, &inner_rgb).unwrap();
    let outer_rgb = image::imageops::crop_imm(&rgb, outer.0, outer.1, outer.2, outer.3).to_image();
    let anime_outer = run_birefnet(&base, "anime-specialist", false, &outer_rgb).unwrap();
    release_birefnet_session("anime-specialist");
    let general_inner = run_birefnet(&base, "birefnet-general", false, &inner_rgb).unwrap();
    release_birefnet_session("birefnet-general");
    let inference_ms = started.elapsed().as_millis();

    let truth = image::open(inputs.join("人工抠图版.png"))
        .unwrap()
        .to_rgba8();
    assert_eq!(truth.dimensions(), (width, height));
    let distance = distance_to_foreground(&base_alpha, width, height, 97);
    let mut report = format!(
        "P112: no GT in crop selection, inference, or proposal gating. AnimeSeg runs at two upper-subject contexts; General BiRefNet may only veto an otherwise agreed AnimeSeg addition.\ninner={inner:?}\nouter={outer:?}\nlocal_inference_ms={inference_ms}\n"
    );
    super::tests::toonout_tests::append_instance_metrics(
        &mut report,
        "baseline",
        &truth,
        &baseline,
    );
    baseline.save(out.join("原图_A_baseline.png")).unwrap();

    for (label, general_threshold) in [
        ("anime_two_pass", None),
        ("general_gate_050", Some(0.50)),
        ("general_gate_060", Some(0.60)),
        ("general_gate_070", Some(0.70)),
        ("general_gate_080", Some(0.80)),
        ("general_gate_090", Some(0.90)),
    ] {
        let mut alpha = base_alpha.clone();
        let mut changed = 0usize;
        for y in inner.1..inner.1 + inner.3 {
            for x in inner.0..inner.0 + inner.2 {
                let index = (y * width + x) as usize;
                let d = distance[index];
                if d >= 96 || base_alpha[index] >= 0.98 {
                    continue;
                }
                let first = anime_inner[((y - inner.1) * inner.2 + x - inner.0) as usize];
                let second = anime_outer[((y - outer.1) * outer.2 + x - outer.0) as usize];
                if first.min(second) < 0.60 {
                    continue;
                }
                let general = general_inner[((y - inner.1) * inner.2 + x - inner.0) as usize];
                if general_threshold.is_some_and(|threshold| general < threshold) {
                    continue;
                }
                let edge = (x - inner.0)
                    .min(inner.0 + inner.2 - 1 - x)
                    .min(y - inner.1)
                    .min(inner.1 + inner.3 - 1 - y);
                let weight = ((96.0 - d as f32) / 32.0).clamp(0.0, 1.0)
                    * (edge as f32 / 64.0).clamp(0.0, 1.0);
                let next = recovery_alpha(base_alpha[index], first, second, weight);
                changed += usize::from(
                    (next * 255.0).round() as u8 != (base_alpha[index] * 255.0).round() as u8,
                );
                alpha[index] = next;
            }
        }
        let result = RgbaImage::from_fn(width, height, |x, y| {
            let source = rgb.get_pixel(x, y);
            Rgba([
                source[0],
                source[1],
                source[2],
                (alpha[(y * width + x) as usize] * 255.0).round() as u8,
            ])
        });
        let mut error_reduction = 0u64;
        let mut error_increase = 0u64;
        for ((old, new), reference) in baseline.pixels().zip(result.pixels()).zip(truth.pixels()) {
            let before = u8::abs_diff(old[3], reference[3]) as u64;
            let after = u8::abs_diff(new[3], reference[3]) as u64;
            error_reduction += before.saturating_sub(after);
            error_increase += after.saturating_sub(before);
        }
        report.push_str(&format!(
            "{label}\tchanged_alpha_pixels={changed}\terror_reduction_u8_sum={error_reduction}\terror_increase_u8_sum={error_increase}\t"
        ));
        super::tests::toonout_tests::append_instance_metrics(&mut report, label, &truth, &result);
        result
            .save(out.join(format!("原图_B_{label}.png")))
            .unwrap();
    }
    fs::write(out.join("report.txt"), report).unwrap();
}

/// Same two-pass Chebyshev distance as the metric helper, without first
/// allocating an extra full-image `Vec<bool>` beside a large RGBA image.
#[test]
#[ignore = "Manual P106: context-preserving local recovery and independent RGB ablation"]
fn ab_context_recovery_and_color() {
    let base = dirs::data_dir().unwrap().join("studio.avroracl.aias");
    let inputs = PathBuf::from(r"F:\战争雷霆涂装\贴图素材\F15E 塞雷娅\测试");
    let results = inputs.parent().unwrap().join("AB测试结果");
    let focus_upper = std::env::var_os("AIAS_RECOVERY_FOCUS_UPPER").is_some();
    let alpha_only = std::env::var_os("AIAS_RECOVERY_ALPHA_ONLY").is_some();
    let only_file = std::env::var("AIAS_RECOVERY_ONLY").ok();
    let out = results.join(if focus_upper {
        "P107_focused_context_recovery"
    } else {
        "P106_context_recovery_color_ablation"
    });
    fs::create_dir_all(&out).unwrap();
    let mut report=String::from("P106: development-only. Original RGB -> same AnimeSeg at two upper-body context crops. No GT in inference or ROI selection.\nA=baseline; B=add-only alpha, original RGB; C=same B alpha, recomputed RGB; D=baseline alpha, original RGB.\nA vs D isolates baseline color; B vs C isolates recovered color; B vs D isolates alpha.\n");
    for file in [
        "原图.png",
        "129085040_p0.jpg",
        "130169544_p0.png",
        "136565655_p0.jpg",
        "142101839_p0.jpg",
        "73307539_p0.jpg",
    ] {
        if only_file.as_deref().is_some_and(|name| name != file) {
            continue;
        }
        let input = inputs.join(file);
        let stem = input.file_stem().unwrap().to_str().unwrap();
        let rgb = image::open(&input).unwrap().to_rgb8();
        let (w, h) = rgb.dimensions();
        let baseline_dir = if stem == "原图" {
            "P77_production_animeseg_closed_form"
        } else {
            "P79_new_images_no_gt"
        };
        let current = image::open(
            results
                .join(baseline_dir)
                .join(format!("{stem}_anime-specialist.png")),
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(current.dimensions(), (w, h));
        let alpha: Vec<f32> = current.pixels().map(|p| p[3] as f32 / 255.0).collect();
        let roi1 = recovery_upper_roi(&alpha, w, h, 55);
        let roi2 = recovery_upper_roi(&alpha, w, h, 75);
        println!("P106 start {file} roi1={roi1:?} roi2={roi2:?}");
        let started = std::time::Instant::now();
        let local1 = run_birefnet_single_for_test(
            &base,
            "anime-specialist",
            false,
            &image::imageops::crop_imm(&rgb, roi1.0, roi1.1, roi1.2, roi1.3).to_image(),
        )
        .unwrap();
        let local2 = run_birefnet_single_for_test(
            &base,
            "anime-specialist",
            false,
            &image::imageops::crop_imm(&rgb, roi2.0, roi2.1, roi2.2, roi2.3).to_image(),
        )
        .unwrap();
        let inference_ms = started.elapsed().as_millis();
        // Limit proposals to a wider neighbourhood, but feather the border instead
        // of cutting it at a hard 8px gate. This is NOT a hair detector.
        let distance = distance_to_foreground(&alpha, w, h, 97);
        let mut recovered = alpha.clone();
        let mut changed_count = 0usize;
        let mut changed = if alpha_only {
            None
        } else {
            Some(vec![false; alpha.len()])
        };
        for y in roi1.1..roi1.1 + roi1.3 {
            for x in roi1.0..roi1.0 + roi1.2 {
                let i = (y * w + x) as usize;
                let d = distance[i] as f32;
                if d >= 96.0 || alpha[i] >= 0.98 {
                    continue;
                }
                let edge = (x - roi1.0)
                    .min(roi1.0 + roi1.2 - 1 - x)
                    .min(y - roi1.1)
                    .min(roi1.1 + roi1.3 - 1 - y);
                let weight =
                    ((96.0 - d) / 32.0).clamp(0.0, 1.0) * (edge as f32 / 64.0).clamp(0.0, 1.0);
                let first = local1[((y - roi1.1) * roi1.2 + x - roi1.0) as usize];
                let second = local2[((y - roi2.1) * roi2.2 + x - roi2.0) as usize];
                if first.min(second) < 0.60 {
                    continue;
                }
                recovered[i] = recovery_alpha(alpha[i], first, second, weight);
                let changed_here =
                    (recovered[i] * 255.0).round() as u8 != current.get_pixel(x, y)[3];
                changed_count += usize::from(changed_here);
                if let Some(changed) = changed.as_mut() {
                    changed[i] = changed_here;
                }
            }
        }
        let original_color = |mask: &[f32]| {
            RgbaImage::from_fn(w, h, |x, y| {
                let p = rgb.get_pixel(x, y);
                Rgba([
                    p[0],
                    p[1],
                    p[2],
                    (mask[(y * w + x) as usize] * 255.0).round() as u8,
                ])
            })
        };
        let b = original_color(&recovered);
        let d = original_color(&alpha);
        let c = if let Some(changed) = changed.as_deref() {
            let mut c = current.clone();
            apply_refined_boundary(&rgb, &mut c, &recovered, changed);
            for ((a, b), c) in current.pixels().zip(b.pixels()).zip(c.pixels()) {
                assert!(b[3] >= a[3]);
                assert_eq!(b[3], c[3]);
            }
            Some(c)
        } else {
            None
        };
        // Save raw ROI masks for inspecting which model mistakes drive proposals.
        for (label, roi, mask) in [("crop55", roi1, &local1), ("crop75", roi2, &local2)] {
            let raw = image::GrayImage::from_fn(roi.2, roi.3, |x, y| {
                image::Luma([
                    (mask[(y * roi.2 + x) as usize].clamp(0.0, 1.0) * 255.0).round() as u8,
                ])
            });
            raw.save(out.join(format!("{stem}_{label}_mask.png")))
                .unwrap();
        }
        report.push_str(&format!("\n{file}\nfocus_upper={focus_upper}\nalpha_only={alpha_only}\nroi55={roi1:?}\nroi75={roi2:?}\nlocal_inference_ms={inference_ms}\nchanged_alpha_pixels={changed_count}\n"));
        if stem == "原图" {
            let gt = image::open(inputs.join("人工抠图版.png"))
                .unwrap()
                .to_rgba8();
            assert_eq!(gt.dimensions(), (w, h));
            for (label, result) in [("baseline", &current), ("recovered", &b)] {
                super::tests::toonout_tests::append_instance_metrics(
                    &mut report,
                    label,
                    &gt,
                    result,
                );
            }
            let mut good = 0f64;
            let mut bad = 0f64;
            for ((old, new), gt) in current.pixels().zip(b.pixels()).zip(gt.pixels()) {
                let before = u8::abs_diff(old[3], gt[3]) as f64;
                let after = u8::abs_diff(new[3], gt[3]) as f64;
                good += (before - after).max(0.0) / 255.0;
                bad += (after - before).max(0.0) / 255.0;
            }
            report.push_str(&format!(
                "alpha_error_reduction_mass={good:.2}\nalpha_error_increase_mass={bad:.2}\n"
            ));
        }
        for (label, result) in [
            ("A_baseline", &current),
            ("B_recovery_original_rgb", &b),
            ("D_baseline_original_rgb", &d),
        ] {
            result
                .save(out.join(format!("{stem}_{label}.png")))
                .unwrap();
        }
        if let Some(c) = &c {
            c.save(out.join(format!("{stem}_C_recovery_refined_rgb.png")))
                .unwrap();
        }
        // Paired previews use the same RGB or alpha, not both changing at once.
        let pairs: Vec<(&str, &RgbaImage, &RgbaImage)> = if let Some(c) = &c {
            vec![
                ("alpha_only_D_vs_B", &d, &b),
                ("color_only_A_vs_D", &current, &d),
                ("candidate_color_B_vs_C", &b, c),
            ]
        } else {
            vec![
                ("alpha_only_D_vs_B", &d, &b),
                ("color_only_A_vs_D", &current, &d),
            ]
        };
        for (label, left, right) in pairs {
            for bg in [0, 96, 255] {
                let left = composite(left, bg);
                let right = composite(right, bg);
                let mut preview = RgbImage::new(1208, (h as f64 / w as f64 * 600.0).round() as u32);
                let ph = preview.height();
                image::imageops::overlay(
                    &mut preview,
                    &image::imageops::resize(&left, 600, ph, image::imageops::FilterType::Lanczos3),
                    0,
                    0,
                );
                image::imageops::overlay(
                    &mut preview,
                    &image::imageops::resize(
                        &right,
                        600,
                        ph,
                        image::imageops::FilterType::Lanczos3,
                    ),
                    608,
                    0,
                );
                preview
                    .save(out.join(format!("{stem}_{label}_bg{bg}_overview.jpg")))
                    .unwrap();
                // Two automatic upper-subject boundary crops, selected before seeing results.
                let side = 768.min(w).min(h);
                for (region, x, y) in [
                    (
                        "upper_left",
                        roi1.0.min(w - side),
                        (roi1.1 + roi1.3 / 4).min(h - side),
                    ),
                    (
                        "upper_right",
                        (roi1.0 + roi1.2).saturating_sub(side).min(w - side),
                        (roi1.1 + roi1.3 / 4).min(h - side),
                    ),
                ] {
                    let mut crop = RgbImage::new(side * 2 + 8, side);
                    image::imageops::overlay(
                        &mut crop,
                        &image::imageops::crop_imm(&left, x, y, side, side).to_image(),
                        0,
                        0,
                    );
                    image::imageops::overlay(
                        &mut crop,
                        &image::imageops::crop_imm(&right, x, y, side, side).to_image(),
                        (side + 8) as i64,
                        0,
                    );
                    crop.save(out.join(format!("{stem}_{label}_bg{bg}_{region}_1to1.png")))
                        .unwrap();
                }
            }
        }
        fs::write(out.join("report.txt"), &report).unwrap();
        println!("P106 finished {file}");
    }
    release_birefnet_session("anime-specialist");
}

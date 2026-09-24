use crate::{
    bake,
    denoise::Oidn,
    gpu::{Gpu, Surface},
    model::{self, Model, Named, Triangle},
};
use glam::Vec3;
use std::collections::BTreeMap;
use std::path::Path;

#[test]
fn output_estimate_counts_uv_and_doubles_precision_maps_at_16_bit() {
    let mut options = bake::Options {
        bits: 8,
        materials: vec![0, 1],
        uv: true,
        ..Default::default()
    };
    let pixels = 1000;
    let uv_only = bake::estimated_output_bytes(&options, pixels);
    assert!(uv_only > 0, "UV-only output must reserve disk space");

    options.uv = false;
    options.world_normal = true;
    let precision_8 = bake::estimated_output_bytes(&options, pixels);
    options.bits = 16;
    let precision_16 = bake::estimated_output_bytes(&options, pixels);
    assert_eq!(precision_16 - precision_8, pixels * 4 * 3 / 5 * 2);
}

#[test]
fn structured_bake_progress_is_monotonic_across_maps_and_materials() {
    let mut events = Vec::new();
    let mut capture = |event| events.push(event);
    for (material_position, map_index, within) in [
        (0, 0, 0.0),
        (0, 5, 0.84),
        (0, 6, 0.10),
        (0, 6, 1.0),
        (1, 0, 0.0),
        (1, 6, 1.0),
    ] {
        bake::emit_bake_progress(
            &mut capture,
            "测试阶段".into(),
            "mesh_map",
            "curvature",
            material_position,
            material_position,
            2,
            map_index,
            7,
            within,
            None,
        );
    }
    let values: Vec<_> = events
        .iter()
        .map(|event| event["progress"].as_f64().unwrap())
        .collect();
    assert!(values.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(events[0]["stage"], "mesh_map");
    assert_eq!(events[0]["map"], "curvature");
    assert_eq!(events[0]["materialPosition"], 1);
    assert_eq!(events.last().unwrap()["materialPosition"], 2);
    assert_eq!(events.last().unwrap()["progress"], 0.96);
}

fn triangle(uv: [[f32; 2]; 3], object: usize, material: usize) -> Triangle {
    Triangle {
        positions: [[0., 0., 0.], [1., 0., 1.], [1., 0., 0.]],
        normals: [[0., 1., 0.]; 3],
        uvs: BTreeMap::from([(0, uv)]),
        object,
        material,
        source_face: object,
    }
}
fn model(triangles: Vec<Triangle>) -> Model {
    Model {
        name: "测试".into(),
        objects: vec![
            Named {
                id: 0,
                name: "地面".into(),
            },
            Named {
                id: 1,
                name: "墙".into(),
            },
        ],
        materials: vec![
            Named {
                id: 0,
                name: "同名".into(),
            },
            Named {
                id: 1,
                name: "同名".into(),
            },
        ],
        triangles,
        bounds: [[0., 0., 0.], [1., 1., 1.]],
        units: "模型单位".into(),
        degenerate_faces: 0,
        degenerate_examples: vec![],
        warnings: vec![],
        source_format: "obj".into(),
        generated_channels: BTreeMap::new(),
    }
}

#[test]
#[ignore = "requires the packaged OIDN runtime; set AIAS_OIDN_DIR"]
fn packaged_oidn_runtime_denoises_pixels() {
    let directory = std::env::var_os("AIAS_OIDN_DIR").expect("AIAS_OIDN_DIR is required");
    let runtime = Oidn::load(std::path::Path::new(&directory)).unwrap();
    let mut pixels = vec![0.0f32; 16 * 16];
    pixels[8 * 16 + 8] = 1.0;
    runtime.denoise_gray(&mut pixels, 16, 16).unwrap();
    assert!(pixels.iter().all(|value| value.is_finite()));
}

#[test]
fn missing_mtl_keeps_usemtl_slots() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.obj");
    std::fs::write(&path, "mtllib absent.mtl\no hull\nusemtl Armor\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n").unwrap();
    let loaded = model::load(&path).unwrap();
    assert_eq!(loaded.materials[loaded.triangles[0].material].name, "Armor");
    assert!(loaded
        .warnings
        .iter()
        .any(|warning| warning.contains("absent.mtl")));
}

#[test]
fn smart_uv_preserves_tiled_source_and_generates_missing_materials() {
    let valid = [[0., 0.], [0., 1.], [1., 0.]];
    let tiled = [[2., -1.], [2., 0.], [3., -1.]];
    let mut missing = triangle(valid, 2, 2);
    missing.uvs.clear();
    let mut value = model(vec![triangle(valid, 0, 0), triangle(tiled, 1, 1), missing]);
    value.objects.push(Named {
        id: 2,
        name: "缺失 UV".into(),
    });
    value.materials.push(Named {
        id: 2,
        name: "缺失 UV".into(),
    });
    let selected = model::prepare_uvs(&mut value, "preserveValid").unwrap();
    assert_eq!(selected[&0], 0);
    assert_eq!(
        selected[&1], 0,
        "finite tiled UV must retain the source channel"
    );
    assert_eq!(value.generated_channels.get(&2), Some(&selected[&2]));
    assert_eq!(value.triangles[0].uvs[&0], valid);
    assert_eq!(value.triangles[1].uvs[&0], tiled);
    let all_objects = vec![0, 1, 2];
    assert!(model::inspect(&value, 1, selected[&1], &all_objects).valid);
    assert!(model::inspect(&value, 2, selected[&2], &all_objects).valid);
}

#[test]
fn generated_uv_welds_connected_triangle_corners() {
    let mut first = triangle([[f32::NAN; 2]; 3], 0, 0);
    first.positions = [[0., 0., 0.], [1., 0., 0.], [1., 1., 0.]];
    let mut second = triangle([[f32::NAN; 2]; 3], 0, 0);
    second.positions = [[0., 0., 0.], [1., 1., 0.], [0., 1., 0.]];
    let mut value = model(vec![first, second]);
    let selected = model::prepare_uvs(&mut value, "regenerateAll").unwrap();
    let channel = selected[&0];
    let a = value.triangles[0].uvs[&channel];
    let b = value.triangles[1].uvs[&channel];
    assert_eq!(a[0], b[0], "the shared origin must stay in one chart");
    assert_eq!(
        a[2], b[1],
        "the shared opposite corner must stay in one chart"
    );
}

#[test]
fn compact_preview_round_trips_offsets() {
    let value = model(vec![triangle([[0., 0.], [0., 1.], [1., 0.]], 0, 0)]);
    let dir = tempfile::tempdir().unwrap();
    let preview = model::write_preview(&value, dir.path()).unwrap();
    let bytes = std::fs::read(preview["bufferPath"].as_str().unwrap()).unwrap();
    assert_eq!(bytes.len() as u64, preview["byteLength"].as_u64().unwrap());
    let batch = &preview["batches"][0];
    let offset = batch["positionOffset"].as_u64().unwrap() as usize;
    assert_eq!(
        f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap()),
        0.0
    );
    assert_eq!(batch["vertexCount"], 3);
}

#[test]
fn uv_shared_edges_at_fractional_offsets_do_not_overlap() {
    for i in 1..2000 {
        let x = (i * 37 % 997) as f32 / 1103.;
        let y = (i * 71 % 991) as f32 / 1109.;
        let p = [x, y];
        let q = [x + 0.0073, y + 0.0268];
        let r = [x + 0.042, y - 0.008];
        let s = [x - 0.011, y + 0.025];
        assert!(!model::overlap([p, q, r], [p, s, q]), "shared edge at {i}");
        assert!(model::overlap([p, q, r], [p, q, r]), "real overlap at {i}");
    }
}
#[test]
fn degenerate_faces_are_skipped_and_counted() {
    // 第二个面三点共线（z 轴上等距），导入必须跳过并计数，而不是拒收。
    let dir = std::env::temp_dir().join(format!("aias-bake-degenerate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("degenerate.obj");
    std::fs::write(
        &path,
        "o 0\nv 0 0 0\nv 1 0 0\nv 0 1 0\nv 2 0 0\nv 3 0 0\nv 4 0 0\nf 1 2 3\nf 4 5 6\n",
    )
    .unwrap();
    let loaded = model::load(&path).unwrap();
    assert_eq!(loaded.triangles.len(), 1, "only the valid face survives");
    assert_eq!(loaded.degenerate_faces, 1);
    assert_eq!(loaded.degenerate_examples, vec!["对象 0 面 1".to_string()]);
    std::fs::remove_dir_all(&dir).ok();
}
#[test]
fn uv_shared_edge_allowed_and_overlap_located() {
    let a = [[0., 0.], [1., 1.], [1., 0.]];
    let b = [[0., 0.], [0., 1.], [1., 1.]];
    assert!(!model::overlap(a, b));
    assert!(model::overlap(a, a));
    let m = model(vec![triangle(a, 0, 0), triangle(b, 1, 0)]);
    assert!(model::inspect(&m, 0, 0, &[0, 1]).valid);
    let m = model(vec![triangle(a, 0, 0), triangle(a, 1, 0)]);
    let r = model::inspect(&m, 0, 0, &[0, 1]);
    // 完全堆叠计入明细但不算缺陷：镜像/分层 UV 是设计，不再判不合格。
    assert!(r.valid);
    assert_eq!(r.defect_count, 0);
    assert_eq!(r.issue_count, 1);
    assert_eq!(r.issues[0].other_triangle, Some(1));
    assert!(model::inspect(&m, 0, 0, &[0]).valid);
    let m = model(vec![triangle(a, 0, 0), triangle(a, 1, 1)]);
    assert!(model::inspect(&m, 0, 0, &[0, 1]).valid);
}
#[test]
fn uv_missing_degenerate_and_outside_are_explicit() {
    let mut t = triangle([[0., 0.], [0., 0.], [0., 0.]], 0, 0);
    let degenerate = model::inspect(&model(vec![t.clone()]), 0, 0, &[0]);
    assert_eq!(degenerate.issues[0].kind, "退化 UV");
    // 零面积退化面覆盖不到像素，无害：记入明细但不判缺陷。
    assert!(degenerate.valid);
    assert_eq!(degenerate.defect_count, 0);
    t.uvs.clear();
    let missing = model::inspect(&model(vec![t.clone()]), 0, 0, &[0]);
    assert_eq!(missing.issues[0].kind, "缺失 UV");
    assert!(!missing.valid);
    t.uvs.insert(0, [[0., 0.], [1.01, 0.], [0., 1.]]);
    let outside = model::inspect(&model(vec![t]), 0, 0, &[0]);
    assert!(outside.valid);
    assert_eq!(outside.defect_count, 0);
    assert_eq!(outside.tiled_count, 1);
    assert_eq!(outside.issue_count, 0);
}

#[test]
fn raster_repeats_native_uv_without_moving_source_vertices() {
    let original = [[0., 0.], [0., 1.], [1., 0.]];
    let tiled = original.map(|p| [p[0] - 8., p[1] - 7.]);
    let reference = model(vec![triangle(original, 0, 0)]);
    let source = model(vec![triangle(tiled, 0, 0)]);
    let cancel = Path::new("");
    let (expected, expected_covered, _) =
        bake::raster(&reference, 0, 0, &[0], 16, false, cancel, || {}).unwrap();
    let (actual, actual_covered, _) =
        bake::raster(&source, 0, 0, &[0], 16, false, cancel, || {}).unwrap();
    assert_eq!(actual_covered, expected_covered);
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected.iter()) {
        assert_eq!(a.pixel, e.pixel);
        assert_eq!(a.object, e.object);
        for axis in 0..3 {
            assert!((a.position[axis] - e.position[axis]).abs() < 1e-5);
            assert!((a.normal[axis] - e.normal[axis]).abs() < 1e-5);
        }
    }
    assert_eq!(source.triangles[0].uvs[&0], tiled);
}

#[test]
fn raster_splits_triangle_across_repeat_seam() {
    let source = model(vec![triangle(
        [[0.75, 0.25], [1.25, 0.25], [1.0, 0.75]],
        0,
        0,
    )]);
    let (_, covered, wire) =
        bake::raster(&source, 0, 0, &[0], 16, true, Path::new(""), || {}).unwrap();
    assert!(covered[9 * 16], "triangle must wrap onto the left edge");
    assert!(covered[9 * 16 + 15], "triangle must cover the right edge");
    assert!(
        !covered[9 * 16 + 8],
        "repeat must not stretch across the map"
    );
    assert_eq!(wire.len(), 16 * 16 * 4);
}

#[test]
fn raster_reports_shared_uv_pixels_instead_of_silent_first_wins() {
    let uv = [[0., 0.], [0., 1.], [1., 0.]];
    let first = triangle(uv, 0, 0);
    let mut second = triangle(uv, 0, 0);
    for point in &mut second.positions {
        point[2] += 10.;
    }
    let source = model(vec![first, second]);
    let (surfaces, covered, _, coverage) =
        bake::raster_with_stats(&source, 0, 0, &[0], 16, false, Path::new(""), || {}).unwrap();
    assert!(!surfaces.is_empty());
    assert_eq!(coverage.shared_pixels, surfaces.len());
    assert_eq!(coverage.shared_samples, surfaces.len());
    assert!(coverage
        .unique_mask(&covered)
        .iter()
        .all(|value| *value == 0));

    let (single, single_covered, _, single_coverage) = bake::raster_with_stats(
        &model(vec![triangle(uv, 0, 0)]),
        0,
        0,
        &[0],
        16,
        false,
        Path::new(""),
        || {},
    )
    .unwrap();
    assert_eq!(single_coverage.shared_pixels, 0);
    assert_eq!(
        single_coverage
            .unique_mask(&single_covered)
            .iter()
            .filter(|value| **value == 255)
            .count(),
        single.len()
    );
}

#[test]
fn saturated_uv_reuse_stops_without_changing_baked_surface_or_mask() {
    let a = triangle([[0., 0.], [0., 1.], [1., 0.]], 0, 0);
    let b = triangle([[1., 1.], [1., 0.], [0., 1.]], 0, 0);
    let source_before_extra = model(vec![a.clone(), b.clone(), a.clone(), b.clone()]);
    let extra = triangle([[0.2, 0.2], [0.8, 0.2], [0.5, 0.8]], 0, 0);
    let source = model(vec![a.clone(), b.clone(), a, b, extra]);
    let (fast_surfaces, fast_covered, _, fast_stats) =
        bake::raster_with_stats(&source, 0, 0, &[0], 16, false, Path::new(""), || {}).unwrap();
    let (full_surfaces, full_covered, wire, full_stats) =
        bake::raster_with_stats(&source, 0, 0, &[0], 16, true, Path::new(""), || {}).unwrap();
    let (_, _, prior_wire, _) = bake::raster_with_stats(
        &source_before_extra,
        0,
        0,
        &[0],
        16,
        true,
        Path::new(""),
        || {},
    )
    .unwrap();
    assert_eq!(fast_surfaces.len(), 16 * 16);
    assert_eq!(fast_covered, full_covered);
    assert_eq!(fast_stats.shared_pixels, 16 * 16);
    assert_eq!(fast_stats.shared_pixels, full_stats.shared_pixels);
    assert_eq!(
        fast_stats.unique_mask(&fast_covered),
        full_stats.unique_mask(&full_covered)
    );
    assert!(fast_stats.shared_samples_truncated);
    assert!(full_stats.shared_samples_truncated);
    assert_ne!(
        wire, prior_wire,
        "UV wire must include faces after saturation"
    );
    for (fast, full) in fast_surfaces.iter().zip(full_surfaces.iter()) {
        assert_eq!(fast.pixel, full.pixel);
        assert_eq!(fast.position, full.position);
        assert_eq!(fast.normal, full.normal);
    }
}

#[test]
fn unique_mask_excludes_order_dependent_surface_data() {
    let shared_uv = [[0., 0.], [0., 1.], [0.5, 0.]];
    let first = triangle(shared_uv, 0, 0);
    let mut second = first.clone();
    for point in &mut second.positions {
        point[2] += 10.;
    }
    let unique = triangle([[0.5, 0.], [1., 0.], [1., 1.]], 0, 0);
    let (a, covered_a, _, stats_a) = bake::raster_with_stats(
        &model(vec![first.clone(), second.clone(), unique.clone()]),
        0,
        0,
        &[0],
        16,
        false,
        Path::new(""),
        || {},
    )
    .unwrap();
    let (b, covered_b, _, stats_b) = bake::raster_with_stats(
        &model(vec![second, first, unique]),
        0,
        0,
        &[0],
        16,
        false,
        Path::new(""),
        || {},
    )
    .unwrap();
    let mask = stats_a.unique_mask(&covered_a);
    assert_eq!(mask, stats_b.unique_mask(&covered_b));
    let a: BTreeMap<_, _> = a.into_iter().map(|s| (s.pixel, s)).collect();
    let b: BTreeMap<_, _> = b.into_iter().map(|s| (s.pixel, s)).collect();
    let mut raw_a = vec![1.0f32; mask.len()];
    let mut raw_b = vec![1.0f32; mask.len()];
    for (pixel, surface) in &a {
        raw_a[*pixel as usize] = 0.2 + surface.position[2] * 0.05;
    }
    for (pixel, surface) in &b {
        raw_b[*pixel as usize] = 0.2 + surface.position[2] * 0.05;
    }
    assert_ne!(raw_a, raw_b);
    let safe_a = bake::unique_scalar_values(&raw_a, &mask, 1.0);
    let safe_b = bake::unique_scalar_values(&raw_b, &mask, 1.0);
    assert_eq!(safe_a, safe_b, "可靠区域 AO 不应依赖三角面顺序");
    let thickness_a = bake::unique_scalar_values(&raw_a, &mask, 0.0);
    let thickness_b = bake::unique_scalar_values(&raw_b, &mask, 0.0);
    assert_eq!(thickness_a, thickness_b, "可靠区域厚度不应依赖三角面顺序");
    let mut unstable = 0;
    let mut reliable = 0;
    for (pixel, left) in &a {
        let right = &b[pixel];
        if mask[*pixel as usize] == 255 {
            reliable += 1;
            assert_eq!(left.position, right.position);
        } else if left.position != right.position {
            unstable += 1;
        }
    }
    assert!(reliable > 0);
    assert!(
        unstable > 0,
        "the fixture must reproduce first-face dependence"
    );
}

#[test]
fn reliable_scalar_neutral_stays_exact_under_8_bit_dither() {
    for pixel in 0..4096 {
        assert_eq!(bake::scalar_byte(1.0, 1.0, pixel), 255);
        assert_eq!(bake::scalar_byte(0.0, 0.0, pixel), 0);
    }
    assert!(bake::scalar_byte(0.5, 1.0, 0) < 255);
}

#[test]
fn saturated_raster_still_checks_later_uv_coordinates() {
    let a = triangle([[0., 0.], [0., 1.], [1., 0.]], 0, 0);
    let b = triangle([[1., 1.], [1., 0.], [0., 1.]], 0, 0);
    let unsafe_uv = triangle([[1_000_001., 0.], [1_000_001., 1.], [1_000_002., 0.]], 0, 0);
    let source = model(vec![a.clone(), b.clone(), a, b, unsafe_uv]);
    let error = bake::raster_with_stats(&source, 0, 0, &[0], 16, false, Path::new(""), || {})
        .err()
        .expect("late unsafe UV must not be hidden by full coverage");
    assert!(error.contains("坐标过大"), "{error}");
}

#[test]
fn unsafe_uv_extent_falls_back_without_mutating_source_coordinates() {
    let source_uv = [[1_000_001., 0.], [1_000_001., 1.], [1_000_002., 0.]];
    let mut value = model(vec![triangle(source_uv, 0, 0)]);
    let report = model::inspect(&value, 0, 0, &[0]);
    assert!(!report.valid);
    assert_eq!(report.issues[0].kind, "UV 平铺范围过大");
    let selected = model::prepare_uvs(&mut value, "preserveValid").unwrap();
    assert_eq!(value.triangles[0].uvs[&0], source_uv);
    assert_ne!(selected[&0], 0);
    assert!(model::inspect(&value, 0, selected[&0], &[0]).valid);
}
#[test]
fn raster_seams_and_dilation_preserve_coverage_and_pure_id() {
    let m = model(vec![
        triangle([[0., 0.], [1., 1.], [1., 0.]], 0, 0),
        triangle([[0., 0.], [0., 1.], [1., 1.]], 0, 0),
    ]);
    let (s, c, w) = bake::raster(&m, 0, 0, &[0], 16, true, Path::new(""), || {}).unwrap();
    assert_eq!(s.len(), 256);
    assert!(c.iter().all(|x| *x));
    assert_eq!(w.len(), 1024);
    let c = [false, false, false, false, true, false, false, false, false];
    let d = bake::dilate(&c, 3, 1);
    assert!(d.iter().all(|s| *s == 4));
    // 欧氏最近源：对角平局不再有方向偏差。(4,0) 到 (4,4) 距离 4，到 (0,0)
    // 距离 √32≈5.66，最近源必须是 (4,4)（旧切比雪夫 BFS 会按扩展顺序取到 (0,0)）。
    let size = 5;
    let mut c = vec![false; size * size];
    c[0] = true;
    c[size * size - 1] = true;
    let d = bake::dilate(&c, size, 4);
    assert_eq!(d[4], 0, "顶行右侧最近源是 (0,0)（距离 4 < √32）");
    assert_eq!(
        d[14],
        (size * size - 1) as u32,
        "(4,2) 到 (4,4) 距离 2，最近源是 (4,4)"
    );
    assert_ne!(bake::color(0), bake::color(1));
    assert_eq!(bake::safe_name("中文:/材质"), "中文__材质");
}

#[test]
fn encode_surface_map_16_places_values_at_uv_pixel() {
    // surfaces 按扫描顺序 push，pixel 是 UV 像素索引，两者几乎不重合：
    // 按序号落位会把整张 16 位图写乱（与 8 位版语义不一致）。
    let surfaces = vec![
        Surface {
            position: [0.; 3],
            object: 0,
            normal: [0.; 3],
            pixel: 5,
        },
        Surface {
            position: [0.; 3],
            object: 1,
            normal: [0.; 3],
            pixel: 0,
        },
        Surface {
            position: [0.; 3],
            object: 2,
            normal: [0.; 3],
            pixel: 3,
        },
    ];
    let nearest = vec![u32::MAX; 8];
    let pixels = bake::encode_surface_map_16(&surfaces, &nearest, |s| [s.object as u16 * 100; 4]);
    assert_eq!(pixels.len(), 32);
    assert_eq!(&pixels[0..4], &[100, 100, 100, 100]);
    assert_eq!(&pixels[12..16], &[200, 200, 200, 200]);
    assert_eq!(&pixels[20..24], &[0, 0, 0, 0]);
    // 未覆盖像素保持 0
    assert!(pixels[4..12].iter().all(|v| *v == 0));
    assert!(pixels[24..].iter().all(|v| *v == 0));
}

#[test]
fn raster_honours_cancellation() {
    let m = model(
        (0..1100)
            .map(|_| triangle([[0., 0.], [1., 1.], [1., 0.]], 0, 0))
            .collect(),
    );
    let cancel =
        std::env::temp_dir().join(format!("aias-raster-cancel-{}.flag", std::process::id()));
    std::fs::write(&cancel, b"").unwrap();
    let result = bake::raster(&m, 0, 0, &[0], 16, false, &cancel, || {});
    std::fs::remove_file(&cancel).ok();
    assert!(result.err().is_some_and(|e| e.contains("取消")));
    // 心跳闭包也应被调用过（喂看门狗）：无取消文件时不提前返回
    let mut beats = 0;
    let nocancel =
        std::env::temp_dir().join(format!("aias-raster-nocancel-{}.flag", std::process::id()));
    std::fs::remove_file(&nocancel).ok();
    let _ = bake::raster(&m, 0, 0, &[0], 16, false, &nocancel, || beats += 1);
    assert!(beats > 0);
}

#[test]
fn curvature_map_is_neutral_on_flats_and_marks_convex_bends() {
    let flat = (0..3)
        .map(|column| Surface {
            position: [column as f32, 0., 0.],
            normal: [0., 1., 0.],
            object: 0,
            pixel: column + 3,
        })
        .collect::<Vec<_>>();
    let nearest = [
        u32::MAX,
        u32::MAX,
        u32::MAX,
        3,
        4,
        5,
        u32::MAX,
        u32::MAX,
        u32::MAX,
    ];
    let flat_pixels = bake::curvature_map(&flat, &nearest, 3);
    assert_eq!(flat_pixels[4 * 4], 128);
    let bent = vec![
        Surface {
            normal: [-0.2, 0.98, 0.],
            ..flat[0]
        },
        flat[1],
        Surface {
            normal: [0.2, 0.98, 0.],
            ..flat[2]
        },
    ];
    let bent_pixels = bake::curvature_map(&bent, &nearest, 3);
    assert!(bent_pixels[4 * 4] > flat_pixels[4 * 4]);
}

#[test]
fn reliable_curvature_does_not_read_shared_uv_neighbors() {
    let nearest = [
        u32::MAX,
        u32::MAX,
        u32::MAX,
        3,
        4,
        5,
        u32::MAX,
        u32::MAX,
        u32::MAX,
    ];
    let mut unique_mask = [0u8; 9];
    unique_mask[4] = 255;
    unique_mask[5] = 255;
    let surfaces = |shared_normal_x: f32| {
        vec![
            Surface {
                position: [-1., 0., 0.],
                normal: [shared_normal_x, 1., 0.],
                object: 0,
                pixel: 3,
            },
            Surface {
                position: [0., 0., 0.],
                normal: [0., 1., 0.],
                object: 0,
                pixel: 4,
            },
            Surface {
                position: [1., 0., 0.],
                normal: [0.02, 1., 0.],
                object: 0,
                pixel: 5,
            },
        ]
    };
    let first = surfaces(-0.03);
    let second = surfaces(0.03);
    let raw_first = bake::curvature_map(&first, &nearest, 3);
    let raw_second = bake::curvature_map(&second, &nearest, 3);
    assert_ne!(raw_first[4 * 4], raw_second[4 * 4]);
    let safe_first = bake::curvature_map_unique(&first, &nearest, 3, &unique_mask);
    let safe_second = bake::curvature_map_unique(&second, &nearest, 3, &unique_mask);
    assert_eq!(safe_first[4 * 4], safe_second[4 * 4]);
    assert!(safe_first[4 * 4] > 128);
    assert_eq!(safe_first[3 * 4], 128);
    assert_eq!(safe_second[3 * 4], 128);
    let safe_16_first = bake::curvature_map_16_unique(&first, &nearest, 3, &unique_mask);
    let safe_16_second = bake::curvature_map_16_unique(&second, &nearest, 3, &unique_mask);
    assert_eq!(safe_16_first[4 * 4], safe_16_second[4 * 4]);
    assert_eq!(safe_16_first[3 * 4], 32768);
}

#[test]
fn curvature_does_not_invent_a_bend_between_different_objects() {
    let nearest = [
        u32::MAX,
        u32::MAX,
        u32::MAX,
        3,
        4,
        5,
        u32::MAX,
        u32::MAX,
        u32::MAX,
    ];
    let center = Surface {
        position: [0., 0., 0.],
        normal: [0., 1., 0.],
        object: 0,
        pixel: 4,
    };
    let own_neighbor = Surface {
        position: [1., 0., 0.],
        pixel: 5,
        ..center
    };
    let foreign_neighbor = Surface {
        position: [-1., 0., 0.],
        normal: [-0.2, 0.98, 0.],
        object: 1,
        pixel: 3,
    };
    let flat = bake::curvature_map(&[center, own_neighbor], &nearest, 3);
    let adjacent = bake::curvature_map(&[foreign_neighbor, center, own_neighbor], &nearest, 3);
    assert_eq!(adjacent[4 * 4], flat[4 * 4]);
}

#[test]
fn curvature_uses_mesh_edges_to_reject_adjacent_disconnected_uv_islands() {
    let first = triangle([[0., 0.], [0.5, 0.], [0., 0.5]], 0, 0);
    let mut disconnected = first.clone();
    for vertex in &mut disconnected.positions {
        vertex[0] += 10.;
    }
    let mut connected = first.clone();
    connected.positions = [first.positions[1], first.positions[2], [2., 0., 0.]];
    let components = bake::mesh_components(&model(vec![first, disconnected, connected]));
    assert_eq!(components[0], components[2]);
    assert_ne!(components[0], components[1]);

    let nearest = [
        u32::MAX,
        u32::MAX,
        u32::MAX,
        3,
        4,
        5,
        u32::MAX,
        u32::MAX,
        u32::MAX,
    ];
    let surfaces = [
        Surface {
            position: [-1., 0., 0.],
            normal: [-0.2, 0.98, 0.],
            object: 0,
            pixel: 3,
        },
        Surface {
            position: [0., 0., 0.],
            normal: [0., 1., 0.],
            object: 0,
            pixel: 4,
        },
        Surface {
            position: [1., 0., 0.],
            normal: [0., 1., 0.],
            object: 0,
            pixel: 5,
        },
    ];
    let surface_components = [components[1], components[0], components[2]];
    let previous = bake::curvature_map(&surfaces, &nearest, 3);
    assert_eq!(previous[4 * 4], 255, "用例应复现同对象假折痕");
    let corrected =
        bake::curvature_map_with_mask(&surfaces, &nearest, 3, None, Some(&surface_components));
    assert_eq!(corrected[4 * 4], 128);
    let mut unique_mask = [0u8; 9];
    unique_mask[3..=5].fill(255);
    let safe = bake::curvature_map_16_with_mask(
        &surfaces,
        &nearest,
        3,
        Some(&unique_mask),
        Some(&surface_components),
    );
    assert_eq!(safe[4 * 4], 32768);
}

#[test]
fn material_stems_follow_sp_style_naming() {
    let named = |id: usize, name: &str| Named {
        id,
        name: name.into(),
    };
    let stems = bake::material_stems(&[
        named(0, "Body"),
        named(1, "Hair"),
        named(2, "同名"),
        named(3, "同名"),
        named(4, "  "),
    ]);
    assert_eq!(stems[0], "Body");
    assert_eq!(stems[1], "Hair");
    assert_eq!(stems[2], "同名_2");
    assert_eq!(stems[3], "同名_3");
    assert_eq!(stems[4], "material_4");
    for stem in &stems {
        assert!(
            !stem.contains("_m0"),
            "old id-tagged format must not survive"
        );
    }
    // 同名区分依据全部材质统计，批量清洗过滤不会改变已定名。
    let single = bake::material_stems(&[named(0, "同名")]);
    assert_eq!(single[0], "同名");
    // 消歧结果与另一材质的字面名重合时，必须继续追加后缀保持唯一，
    // 否则两个材质写出同一组贴图文件静默互相覆盖。
    let collision = bake::material_stems(&[named(0, "Gold"), named(1, "Gold"), named(2, "Gold_0")]);
    assert_eq!(collision.len(), 3);
    assert_eq!(
        collision
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3
    );
    // 长名截断按 UTF-8 字节预算（120 字节）而非字符数，CJK 名不会造出超长路径。
    let long_cjk: String = "坦".repeat(90);
    let stems = bake::material_stems(&[named(0, &long_cjk)]);
    assert_eq!(stems[0].chars().count(), 40);
    assert_eq!(stems[0].len(), 120);
}
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn hit(o: Vec3, d: Vec3, v: &[[f32; 3]], distance: f32, bias: f32) -> bool {
    let a = Vec3::from_array(v[0]);
    let e1 = Vec3::from_array(v[1]) - a;
    let e2 = Vec3::from_array(v[2]) - a;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return false;
    }
    let q = o - a;
    let u = q.dot(p) / det;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let r = q.cross(e1);
    let v = d.dot(r) / det;
    if v < 0. || u + v > 1. {
        return false;
    }
    let t = e2.dot(r) / det;
    t >= bias && t <= distance
}
fn reference(
    s: &Surface,
    vertices: &[[f32; 3]],
    objects: &[u32],
    samples: u32,
    distance: f32,
    bias: f32,
    self_only: bool,
) -> u32 {
    let n = Vec3::from_array(s.normal).normalize();
    let t = (if n.z.abs() < 0.999 { Vec3::Z } else { Vec3::Y })
        .cross(n)
        .normalize();
    let b = n.cross(t);
    let origin = Vec3::from_array(s.position);
    let azimuth_phase = (hash(s.pixel) & 0x00ff_ffff) as f32 / 16_777_216.;
    (0..samples)
        .filter(|k| {
            let u = (*k as f32 + 0.5) / samples as f32;
            let v = (azimuth_phase + (*k as f32 + 0.5) / samples as f32).fract();
            let a = std::f32::consts::TAU * v;
            let d = t * (u.sqrt() * a.cos()) + b * (u.sqrt() * a.sin()) + n * (1. - u).sqrt();
            vertices
                .chunks_exact(3)
                .zip(objects)
                .any(|(v, o)| (!self_only || *o == s.object) && hit(origin, d, v, distance, bias))
        })
        .count() as u32
}
#[test]
#[ignore = "Requires physical DXR 1.1 GPU"]
fn gpu_matches_cpu_distance_self_and_repeat() {
    let v = [
        [-2., 0., -2.],
        [2., 0., -2.],
        [0., 0., 2.],
        [-2., 0.5, -2.],
        [0., 0.5, 2.],
        [2., 0.5, -2.],
    ];
    let objects = [0, 1];
    let surfaces: Vec<_> = (0..64)
        .map(|i| Surface {
            position: [(i % 8) as f32 * 0.1 - 0.4, 0., (i / 8) as f32 * 0.1 - 0.4],
            normal: [0., 1., 0.],
            object: 0,
            pixel: i,
        })
        .collect();
    let mut gpu = Gpu::new(0, &v, &objects, false).unwrap();
    let top = [Surface {
        position: [0., 0.5, 0.],
        normal: [0., 1., 0.],
        object: 1,
        pixel: 0,
    }];
    let thickness = gpu
        .trace_thickness(&top, 128, 2., 0.0001, false, || false)
        .unwrap();
    assert!(
        thickness[0] > 0,
        "inward rays must measure the opposite shell"
    );
    for (distance, self_only) in [(2., false), (0.1, false), (2., true)] {
        let actual = gpu
            .trace(&surfaces, 128, distance, 0.0001, self_only, || false)
            .unwrap();
        let expected: Vec<_> = surfaces
            .iter()
            .map(|s| reference(s, &v, &objects, 128, distance, 0.0001, self_only))
            .collect();
        assert_eq!(actual, expected, "GPU/CPU disagreement");
        assert_eq!(
            actual,
            gpu.trace(&surfaces, 128, distance, 0.0001, self_only, || false)
                .unwrap()
        );
        if self_only || distance < 0.5 {
            assert!(actual.iter().all(|h| *h == 0));
        } else {
            assert!(actual.iter().all(|h| *h >= 80));
        }
    }
    assert!(gpu
        .trace(&surfaces, 128, 2., 0.0001, false, || true)
        .unwrap_err()
        .contains("取消"));
    assert!(gpu
        .trace(&surfaces, 32, 2., 0.0001, false, || false)
        .is_ok());
    assert!(Gpu::with_budget(0, &v, &objects, Some(1024), false)
        .err()
        .unwrap()
        .contains("显存"));
    gpu.remove_device();
    assert!(gpu
        .trace(&surfaces, 32, 2., 0.0001, false, || false)
        .is_err());
    drop(gpu);
    let mut retry = Gpu::new(0, &v, &objects, false).unwrap();
    assert!(retry
        .trace(&surfaces, 32, 2., 0.0001, false, || false)
        .is_ok());
}

#[test]
fn dither_lsb_stays_within_one_lsb_and_averages_near_zero() {
    let mut sum = 0f64;
    for i in 0..10_000usize {
        let v = bake::dither_lsb(i);
        assert!((-1.0..=1.0).contains(&v), "抖动越界: {v}");
        sum += v as f64;
    }
    assert!((sum / 10_000.0).abs() < 0.05, "抖动均值应接近 0");
}

#[test]
fn dt_1d_sq_reports_exact_squared_distances_and_sources() {
    let inf = 1u32 << 30;
    let mut f = vec![inf; 8];
    f[0] = 0;
    f[7] = 0;
    let src = [
        0u32,
        u32::MAX,
        u32::MAX,
        u32::MAX,
        u32::MAX,
        u32::MAX,
        u32::MAX,
        7u32,
    ];
    let (d, s) = bake::dt_1d_sq(&f, &src);
    let expect = [0u32, 1, 4, 9, 9, 4, 1, 0];
    for (i, e) in expect.iter().enumerate() {
        assert_eq!(d[i], *e, "位置 {i} 平方距离不符");
    }
    assert_eq!(s[2], 0, "位置 2 最近源是 0");
    assert_eq!(s[5], 7, "位置 5 最近源是 7");
}

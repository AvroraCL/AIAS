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
    assert_eq!(precision_16, precision_8 * 2);
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
fn smart_uv_preserves_valid_and_generates_invalid_materials() {
    let valid = [[0., 0.], [0., 1.], [1., 0.]];
    let mut invalid = triangle([[2., 0.], [2., 1.], [3., 0.]], 1, 1);
    invalid.positions = [[0., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    let mut value = model(vec![triangle(valid, 0, 0), invalid]);
    let selected = model::prepare_uvs(&mut value, "preserveValid").unwrap();
    assert_eq!(selected[&0], 0);
    assert_ne!(selected[&1], 0);
    assert_eq!(value.triangles[0].uvs[&0], valid);
    let all_objects = vec![0, 1];
    assert!(model::inspect(&value, 1, selected[&1], &all_objects).valid);
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
    assert_eq!(outside.issues[0].kind, "UV 超出 0–1");
    assert!(!outside.valid);
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
    assert_eq!(d[14], (size * size - 1) as u32, "(4,2) 到 (4,4) 距离 2，最近源是 (4,4)");
    assert_ne!(bake::color(0), bake::color(1));
    assert_eq!(bake::safe_name("中文:/材质"), "中文__材质");
}

#[test]
fn encode_surface_map_16_places_values_at_uv_pixel() {
    // surfaces 按扫描顺序 push，pixel 是 UV 像素索引，两者几乎不重合：
    // 按序号落位会把整张 16 位图写乱（与 8 位版语义不一致）。
    let surfaces = vec![
        Surface { position: [0.; 3], object: 0, normal: [0.; 3], pixel: 5 },
        Surface { position: [0.; 3], object: 1, normal: [0.; 3], pixel: 0 },
        Surface { position: [0.; 3], object: 2, normal: [0.; 3], pixel: 3 },
    ];
    let nearest = vec![u32::MAX; 8];
    let pixels = bake::encode_surface_map_16(&surfaces, &nearest, |s| {
        [s.object as u16 * 100; 4]
    });
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
    let cancel = std::env::temp_dir().join(format!("aias-raster-cancel-{}.flag", std::process::id()));
    std::fs::write(&cancel, b"").unwrap();
    let result = bake::raster(&m, 0, 0, &[0], 16, false, &cancel, || {});
    std::fs::remove_file(&cancel).ok();
    assert!(result.err().is_some_and(|e| e.contains("取消")));
    // 心跳闭包也应被调用过（喂看门狗）：无取消文件时不提前返回
    let mut beats = 0;
    let nocancel = std::env::temp_dir().join(format!("aias-raster-nocancel-{}.flag", std::process::id()));
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
    let collision = bake::material_stems(&[
        named(0, "Gold"),
        named(1, "Gold"),
        named(2, "Gold_0"),
    ]);
    assert_eq!(collision.len(), 3);
    assert_eq!(collision.iter().collect::<std::collections::HashSet<_>>().len(), 3);
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
fn hit(o: Vec3, d: Vec3, v: &[[f32; 3]], distance: f32) -> bool {
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
    t >= 0. && t <= distance
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
    let origin = Vec3::from_array(s.position) + n * bias;
    (0..samples)
        .filter(|k| {
            let u = (*k as f32 + 0.5) / samples as f32;
            let v = (hash(s.pixel ^ hash(*k + 17)) & 0xffffff) as f32 / 16777216.;
            let a = std::f32::consts::TAU * v;
            let d = t * (u.sqrt() * a.cos()) + b * (u.sqrt() * a.sin()) + n * (1. - u).sqrt();
            vertices
                .chunks_exact(3)
                .zip(objects)
                .any(|(v, o)| (!self_only || *o == s.object) && hit(origin, d, v, distance))
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
            assert!(actual.iter().all(|h| *h > 80));
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
    let src = [0u32, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX, 7u32];
    let (d, s) = bake::dt_1d_sq(&f, &src);
    let expect = [0u32, 1, 4, 9, 9, 4, 1, 0];
    for (i, e) in expect.iter().enumerate() {
        assert_eq!(d[i], *e, "位置 {i} 平方距离不符");
    }
    assert_eq!(s[2], 0, "位置 2 最近源是 0");
    assert_eq!(s[5], 7, "位置 5 最近源是 7");
}

use crate::{
    bake,
    gpu::{Gpu, Surface},
    model::{self, Model, Named, Triangle},
};
use glam::Vec3;
use std::collections::BTreeMap;
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
    }
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
    assert!(!r.valid);
    assert_eq!(r.issues[0].other_triangle, Some(1));
    assert!(model::inspect(&m, 0, 0, &[0]).valid);
    let m = model(vec![triangle(a, 0, 0), triangle(a, 1, 1)]);
    assert!(model::inspect(&m, 0, 0, &[0, 1]).valid);
}
#[test]
fn uv_missing_degenerate_and_outside_are_explicit() {
    let mut t = triangle([[0., 0.], [0., 0.], [0., 0.]], 0, 0);
    assert_eq!(
        model::inspect(&model(vec![t.clone()]), 0, 0, &[0]).issues[0].kind,
        "退化 UV"
    );
    t.uvs.clear();
    assert_eq!(
        model::inspect(&model(vec![t.clone()]), 0, 0, &[0]).issues[0].kind,
        "缺失 UV"
    );
    t.uvs.insert(0, [[0., 0.], [1.01, 0.], [0., 1.]]);
    assert_eq!(
        model::inspect(&model(vec![t]), 0, 0, &[0]).issues[0].kind,
        "UV 超出 0–1"
    );
}
#[test]
fn raster_seams_and_dilation_preserve_coverage_and_pure_id() {
    let m = model(vec![
        triangle([[0., 0.], [1., 1.], [1., 0.]], 0, 0),
        triangle([[0., 0.], [0., 1.], [1., 1.]], 0, 0),
    ]);
    let (s, c, w) = bake::raster(&m, 0, 0, &[0], 16, true).unwrap();
    assert_eq!(s.len(), 256);
    assert!(c.iter().all(|x| *x));
    assert_eq!(w.len(), 1024);
    let c = [false, false, false, false, true, false, false, false, false];
    let d = bake::dilate(&c, 3, 1);
    assert!(d.iter().all(|s| *s == Some(4)));
    assert_ne!(bake::color(0), bake::color(1));
    assert_eq!(bake::safe_name("中文:/材质"), "中文__材质");
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
    let mut gpu = Gpu::new(0, &v, &objects).unwrap();
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
    assert!(Gpu::with_budget(0, &v, &objects, Some(1024))
        .err()
        .unwrap()
        .contains("显存"));
    gpu.remove_device();
    assert!(gpu
        .trace(&surfaces, 32, 2., 0.0001, false, || false)
        .is_err());
    drop(gpu);
    let mut retry = Gpu::new(0, &v, &objects).unwrap();
    assert!(retry
        .trace(&surfaces, 32, 2., 0.0001, false, || false)
        .is_ok());
}

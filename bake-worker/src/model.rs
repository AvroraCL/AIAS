use glam::{Mat3, Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Triangle {
    pub positions: [[f32; 3]; 3],
    pub normals: [[f32; 3]; 3],
    pub uvs: BTreeMap<u32, [[f32; 2]; 3]>,
    pub object: usize,
    pub material: usize,
    pub source_face: usize,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Named {
    pub id: usize,
    pub name: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub name: String,
    pub objects: Vec<Named>,
    pub materials: Vec<Named>,
    pub triangles: Vec<Triangle>,
    pub bounds: [[f32; 3]; 2],
    pub units: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub kind: String,
    pub object: usize,
    pub face: usize,
    pub triangle: usize,
    pub other_triangle: Option<usize>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UvReport {
    pub material: usize,
    pub channel: u32,
    pub valid: bool,
    pub issues: Vec<Issue>,
    pub issue_count: usize,
}
pub fn load(path: &Path) -> Result<Model, String> {
    let bytes = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    import_budget(bytes)?;
    let mut model = Model {
        name: path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        objects: vec![],
        materials: vec![],
        triangles: vec![],
        bounds: [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]],
        units: "模型单位".into(),
    };
    match path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
        .as_str()
    {
        "obj" => obj(path, &mut model)?,
        "gltf" | "glb" => gltf(path, &mut model)?,
        _ => return Err("仅支持 OBJ、GLB 和 glTF".into()),
    }
    if model.triangles.is_empty() {
        return Err("模型没有三角网格".into());
    }
    for t in &mut model.triangles {
        if t.positions.iter().flatten().any(|x| !x.is_finite()) {
            return Err("模型坐标包含非有限值".into());
        }
        let p = t.positions.map(Vec3::from_array);
        let geometric = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        if geometric == Vec3::ZERO {
            return Err(format!("对象 {} 面 {} 的几何退化", t.object, t.source_face));
        }
        for n in &mut t.normals {
            let normalized = Vec3::from_array(*n).normalize_or_zero();
            *n = if normalized.is_finite() && normalized != Vec3::ZERO {
                normalized
            } else {
                geometric
            }
            .to_array();
        }
        for p in t.positions {
            for axis in 0..3 {
                model.bounds[0][axis] = model.bounds[0][axis].min(p[axis]);
                model.bounds[1][axis] = model.bounds[1][axis].max(p[axis]);
            }
        }
    }
    Ok(model)
}
fn import_budget(bytes: u64) -> Result<(), String> {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    if bytes.saturating_mul(20) > system.available_memory() / 2 {
        return Err(format!(
            "模型解析内存预检查失败：输入缓冲区 {} MiB，当前可用内存 {} MiB",
            bytes / 1048576,
            system.available_memory() / 1048576
        ));
    }
    Ok(())
}
fn obj(path: &Path, out: &mut Model) -> Result<(), String> {
    use std::io::BufRead;
    // tobj fills a missing corner UV/normal with an earlier index. Preserve the
    // source presence bits and object boundaries before material-based splitting.
    let mut source_faces = Vec::new();
    let mut source_object = 0usize;
    let mut object_names = vec!["默认对象".to_string()];
    for line in
        std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?).lines()
    {
        let line = line.map_err(|e| e.to_string())?;
        let mut parts = line.split('#').next().unwrap_or("").split_whitespace();
        match parts.next() {
            Some("o" | "g") => {
                object_names.push(parts.collect::<Vec<_>>().join(" "));
                source_object = object_names.len() - 1;
            }
            Some("f") => {
                let corners: Vec<_> = parts
                    .map(|p| p.split('/').map(str::to_owned).collect::<Vec<_>>())
                    .collect();
                if corners.len() != 3 {
                    return Err(format!(
                        "源面 {} 不是三角形，请三角化后导出",
                        source_faces.len()
                    ));
                }
                let uv = corners
                    .iter()
                    .all(|c| c.get(1).is_some_and(|s| !s.is_empty()));
                let normal = std::array::from_fn::<_, 3, _>(|i| {
                    corners[i].get(2).is_some_and(|s| !s.is_empty())
                });
                source_faces.push((source_object, uv, normal));
            }
            _ => {}
        }
    }
    let mut object_map = BTreeMap::new();
    for (source, _, _) in &source_faces {
        if !object_map.contains_key(source) {
            let id = out.objects.len();
            out.objects.push(Named {
                id,
                name: object_names[*source].clone(),
            });
            object_map.insert(*source, id);
        }
    }
    let mut source_index = 0;
    let (models, materials) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            triangulate: false,
            single_index: false,
            ignore_points: true,
            ignore_lines: true,
            ..Default::default()
        },
    )
    .map_err(|e| format!("OBJ: {e}"))?;
    let materials = materials.map_err(|e| format!("OBJ 材质文件: {e}"))?;
    out.materials = materials
        .iter()
        .enumerate()
        .map(|(id, m)| Named {
            id,
            name: m.name.clone(),
        })
        .collect();
    let default = out.materials.len();
    out.materials.push(Named {
        id: default,
        name: "默认材质".into(),
    });
    for m in models {
        let mesh = m.mesh;
        let arities = if mesh.face_arities.is_empty() {
            vec![3; mesh.indices.len() / 3]
        } else {
            mesh.face_arities.clone()
        };
        let mut offset = 0;
        for (face, arity) in arities.into_iter().enumerate() {
            let (source, source_uv, source_normals) =
                source_faces.get(source_index).ok_or("OBJ 源面映射不一致")?;
            let object = object_map[source];
            let arity = arity as usize;
            // Input contract is a triangle mesh. Reject polygons rather than incorrectly fan-triangulating concave faces.
            if arity != 3 {
                return Err(format!(
                    "对象 {object} 面 {face} 不是三角形，请在建模软件中三角化后导出"
                ));
            }
            let mut t = Triangle {
                positions: [[0.; 3]; 3],
                normals: [[0.; 3]; 3],
                uvs: BTreeMap::new(),
                object,
                material: mesh.material_id.unwrap_or(default),
                source_face: source_index,
            };
            let mut uv = [[0.; 2]; 3];
            let mut has_uv = *source_uv;
            for k in 0..3 {
                let i = offset + k;
                let p = mesh.indices[i] as usize * 3;
                t.positions[k].copy_from_slice(mesh.positions.get(p..p + 3).ok_or("OBJ 索引越界")?);
                if let Some(n) = mesh.normal_indices.get(i) {
                    let n = *n as usize * 3;
                    if let Some(v) = mesh.normals.get(n..n + 3) {
                        t.normals[k].copy_from_slice(v);
                    }
                }
                if !source_normals[k] {
                    t.normals[k] = [0.; 3];
                }
                if let Some(u) = mesh.texcoord_indices.get(i) {
                    let u = *u as usize * 2;
                    if let Some(v) = mesh.texcoords.get(u..u + 2) {
                        uv[k].copy_from_slice(v);
                    } else {
                        has_uv = false;
                    }
                } else {
                    has_uv = false;
                }
            }
            if has_uv {
                t.uvs.insert(0, uv);
            }
            out.triangles.push(t);
            source_index += 1;
            offset += arity;
        }
    }
    Ok(())
}
fn gltf(path: &Path, out: &mut Model) -> Result<(), String> {
    // Only buffers are needed: material image codecs never constrain geometry import.
    let document = gltf::Gltf::open(path).map_err(|e| format!("glTF: {e}"))?;
    let buffer_bytes = document
        .buffers()
        .try_fold(0u64, |sum, b| sum.checked_add(b.length() as u64))
        .ok_or("glTF 缓冲区大小溢出")?;
    import_budget(buffer_bytes)?;
    let buffers = gltf::import_buffers(&document.document, path.parent(), document.blob.clone())
        .map_err(|e| format!("glTF 缓冲区: {e}"))?;
    out.units = "米（glTF）".into();
    out.materials = document
        .materials()
        .map(|m| Named {
            id: m.index().unwrap(),
            name: m.name().unwrap_or("材质").into(),
        })
        .collect();
    let default = out.materials.len();
    out.materials.push(Named {
        id: default,
        name: "默认材质".into(),
    });
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or("glTF 没有场景")?;
    for node in scene.nodes() {
        visit(node, Mat4::IDENTITY, &buffers, default, out)?;
    }
    Ok(())
}
fn visit(
    node: gltf::Node,
    parent: Mat4,
    buffers: &[gltf::buffer::Data],
    default: usize,
    out: &mut Model,
) -> Result<(), String> {
    if node.skin().is_some() {
        return Err(format!("节点 {} 含蒙皮，请导出静态网格", node.index()));
    }
    let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
    let det = transform.determinant();
    if !det.is_finite() || det.abs() < 1e-15 {
        return Err(format!("节点 {} 变换不可逆", node.index()));
    }
    if let Some(mesh) = node.mesh() {
        let object = out.objects.len();
        out.objects.push(Named {
            id: object,
            name: node.name().or(mesh.name()).unwrap_or("对象").into(),
        });
        let normal_transform = Mat3::from_mat4(transform).inverse().transpose();
        let mut source_face = 0;
        for primitive in mesh.primitives() {
            if primitive.morph_targets().next().is_some() {
                return Err(format!("对象 {object} 含形变目标，请导出静态网格"));
            }
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                return Err(format!("对象 {object} 不是三角列表，请三角化后导出"));
            }
            let reader = primitive.reader(|b| Some(&buffers[b.index()]));
            let positions: Vec<_> = reader
                .read_positions()
                .ok_or("缺失 POSITION")?
                .map(|p| transform.transform_point3(Vec3::from_array(p)).to_array())
                .collect();
            let normals: Vec<_> = reader
                .read_normals()
                .map(|r| {
                    r.map(|n| {
                        (normal_transform * Vec3::from_array(n))
                            .normalize_or_zero()
                            .to_array()
                    })
                    .collect()
                })
                .unwrap_or_default();
            let indices: Vec<u32> = reader
                .read_indices()
                .map(|r| r.into_u32().collect())
                .unwrap_or_else(|| (0..positions.len() as u32).collect());
            if indices.len() % 3 != 0 {
                return Err("三角索引数量不是 3 的倍数".into());
            }
            let channels: BTreeMap<u32, Vec<[f32; 2]>> = primitive
                .attributes()
                .filter_map(|(semantic, _)| {
                    if let gltf::Semantic::TexCoords(c) = semantic {
                        reader
                            .read_tex_coords(c)
                            .map(|r| (c, r.into_f32().map(|uv| [uv[0], 1. - uv[1]]).collect()))
                    } else {
                        None
                    }
                })
                .collect();
            for face in indices.chunks_exact(3) {
                let order = if det < 0. {
                    [face[0], face[2], face[1]]
                } else {
                    [face[0], face[1], face[2]]
                };
                let mut t = Triangle {
                    positions: [[0.; 3]; 3],
                    normals: [[0.; 3]; 3],
                    uvs: BTreeMap::new(),
                    object,
                    material: primitive.material().index().unwrap_or(default),
                    source_face,
                };
                for k in 0..3 {
                    t.positions[k] = *positions.get(order[k] as usize).ok_or("glTF 索引越界")?;
                    t.normals[k] = normals.get(order[k] as usize).copied().unwrap_or([0.; 3]);
                }
                for (c, values) in &channels {
                    let mut uv = [[0.; 2]; 3];
                    for k in 0..3 {
                        uv[k] = *values.get(order[k] as usize).ok_or("UV 索引越界")?;
                    }
                    t.uvs.insert(*c, uv);
                }
                out.triangles.push(t);
                source_face += 1;
            }
        }
    }
    for child in node.children() {
        visit(child, transform, buffers, default, out)?;
    }
    Ok(())
}
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}
pub fn overlap(a: [[f32; 2]; 3], b: [[f32; 2]; 3]) -> bool {
    let mut polygon: Vec<Vec2> = a.into_iter().map(Vec2::from_array).collect();
    let b = b.map(Vec2::from_array);
    let sign = cross(b[1] - b[0], b[2] - b[0]).signum();
    for edge in 0..3 {
        let p = b[edge];
        let q = b[(edge + 1) % 3];
        let input = std::mem::take(&mut polygon);
        if input.is_empty() {
            return false;
        }
        let mut previous = *input.last().unwrap();
        let mut pd = sign * cross(q - p, previous - p);
        for current in input {
            let cd = sign * cross(q - p, current - p);
            if (cd >= 0.) != (pd >= 0.) {
                polygon.push(previous + (current - previous) * (pd / (pd - cd)));
            }
            if cd >= 0. {
                polygon.push(current);
            }
            previous = current;
            pd = cd;
        }
    }
    let area = (0..polygon.len())
        .map(|i| cross(polygon[i], polygon[(i + 1) % polygon.len()]))
        .sum::<f32>()
        .abs()
        * 0.5;
    area > 1e-10
}
pub fn inspect(model: &Model, material: usize, channel: u32, objects: &[usize]) -> UvReport {
    let mut issues = vec![];
    let mut count = 0;
    let mut valid = vec![];
    let mut add = |kind: &str, index: usize, other: Option<usize>| {
        count += 1;
        if issues.len() < 10000 {
            let t = &model.triangles[index];
            issues.push(Issue {
                kind: kind.into(),
                object: t.object,
                face: t.source_face,
                triangle: index,
                other_triangle: other,
            });
        }
    };
    for (index, t) in model
        .triangles
        .iter()
        .enumerate()
        .filter(|(_, t)| t.material == material && objects.contains(&t.object))
    {
        let Some(uv) = t.uvs.get(&channel) else {
            add("缺失 UV", index, None);
            continue;
        };
        if uv
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || *v < 0. || *v > 1.)
        {
            add("UV 超出 0–1", index, None);
            continue;
        }
        let v = uv.map(Vec2::from_array);
        if cross(v[1] - v[0], v[2] - v[0]).abs() < 1e-12 {
            add("退化 UV", index, None);
            continue;
        }
        let min = v.iter().fold(Vec2::splat(f32::INFINITY), |a, b| a.min(*b));
        let max = v
            .iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), |a, b| a.max(*b));
        valid.push((index, *uv, min, max));
    }
    valid.sort_by(|a, b| a.2.x.total_cmp(&b.2.x));
    for i in 0..valid.len() {
        let a = &valid[i];
        for b in &valid[i + 1..] {
            if b.2.x >= a.3.x {
                break;
            }
            if b.2.y >= a.3.y || b.3.y <= a.2.y {
                continue;
            }
            if overlap(a.1, b.1) {
                add("UV 重叠", a.0, Some(b.0));
            }
        }
    }
    UvReport {
        material,
        channel,
        valid: count == 0,
        issues,
        issue_count: count,
    }
}

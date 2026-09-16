use glam::{Mat3, Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Write, path::Path};
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
    /// 零面积（叉积归零）面在导入时被跳过的数量：这类面光栅化不可见、对
    /// 烘焙零贡献，跳过而非拒收整个模型。serde(default) 兼容旧 model.json。
    #[serde(default)]
    pub degenerate_faces: usize,
    #[serde(default)]
    pub degenerate_examples: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub source_format: String,
    #[serde(default)]
    pub generated_channels: BTreeMap<usize, u32>,
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
        degenerate_faces: 0,
        degenerate_examples: vec![],
        warnings: vec![],
        source_format: path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase(),
        generated_channels: BTreeMap::new(),
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
    let mut kept = Vec::with_capacity(model.triangles.len());
    for mut t in std::mem::take(&mut model.triangles) {
        if t.positions.iter().flatten().any(|x| !x.is_finite()) {
            return Err("模型坐标包含非有限值".into());
        }
        let p = t.positions.map(Vec3::from_array);
        let geometric = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
        if geometric == Vec3::ZERO {
            model.degenerate_faces += 1;
            if model.degenerate_examples.len() < 5 {
                model
                    .degenerate_examples
                    .push(format!("对象 {} 面 {}", t.object, t.source_face));
            }
            continue;
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
        kept.push(t);
    }
    model.triangles = kept;
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
    let mut mtllibs: Vec<String> = Vec::new();
    let mut source_material: Option<String> = None;
    let mut source_materials: Vec<String> = Vec::new();
    for line in
        std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?).lines()
    {
        let line = line.map_err(|e| e.to_string())?;
        let mut parts = line.split('#').next().unwrap_or("").split_whitespace();
        match parts.next() {
            Some("mtllib") => {
                let name = parts.collect::<Vec<_>>().join(" ");
                if !name.is_empty() {
                    mtllibs.push(name);
                }
            }
            Some("o" | "g") => {
                object_names.push(parts.collect::<Vec<_>>().join(" "));
                source_object = object_names.len() - 1;
            }
            Some("usemtl") => {
                let name = parts.collect::<Vec<_>>().join(" ");
                source_material = (!name.is_empty()).then_some(name.clone());
                if !name.is_empty() && !source_materials.contains(&name) {
                    source_materials.push(name);
                }
            }
            Some("f") => {
                // 只需要知道每个角是否带非空 UV/法线；百万级角上分配
                // String/Vec 是导入的次要瓶颈，直接零分配解析三段。
                let mut corner = 0usize;
                let mut uv = [false; 3];
                let mut normal = [false; 3];
                for p in parts {
                    if corner < 3 {
                        let mut segments = p.split('/');
                        let _vertex = segments.next();
                        uv[corner] = segments.next().is_some_and(|s| !s.is_empty());
                        normal[corner] = segments.next().is_some_and(|s| !s.is_empty());
                    }
                    corner += 1;
                }
                if corner != 3 {
                    return Err(format!(
                        "源面 {} 不是三角形，请三角化后导出",
                        source_faces.len()
                    ));
                }
                source_faces.push((
                    source_object,
                    uv.iter().all(|flag| *flag),
                    normal,
                    source_material.clone(),
                ));
            }
            _ => {}
        }
    }
    let mut object_map = BTreeMap::new();
    for (source, _, _, _) in &source_faces {
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
    // 材质定义不参与几何和贴图烘焙。MTL 缺失时保留 usemtl 槽位并给出警告，
    // 不再拒绝整个 OBJ。
    for name in &mtllibs {
        let mtl = path.parent().unwrap_or(Path::new(".")).join(name);
        if !mtl.is_file() {
            out.warnings.push(format!(
                "找不到 OBJ 引用的材质文件 {name}，已从 usemtl 恢复材质槽。"
            ));
        }
    }
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
    let loaded_materials = match materials {
        Ok(materials) => materials,
        Err(error) => {
            if out.warnings.is_empty() {
                out.warnings.push(format!(
                    "OBJ 材质文件不可读（{error}），已从 usemtl 恢复材质槽。"
                ));
            }
            Vec::new()
        }
    };
    let mut material_names: Vec<String> = loaded_materials.iter().map(|m| m.name.clone()).collect();
    for name in source_materials {
        if !material_names.contains(&name) {
            material_names.push(name);
        }
    }
    out.materials = material_names
        .iter()
        .enumerate()
        .map(|(id, name)| Named {
            id,
            name: name.clone(),
        })
        .collect();
    let material_ids: BTreeMap<String, usize> = out
        .materials
        .iter()
        .map(|m| (m.name.clone(), m.id))
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
            let (source, source_uv, source_normals, source_material) =
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
                material: source_material
                    .as_ref()
                    .and_then(|name| material_ids.get(name))
                    .copied()
                    .unwrap_or(default),
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

fn generate_material_uv(model: &mut Model, material: usize) -> Result<u32, String> {
    unsafe extern "C" {
        fn aias_xatlas_generate(
            positions: *const f32,
            indices: *const u32,
            vertex_count: u32,
            index_count: u32,
            output_uvs: *mut f32,
            atlas_width: *mut u32,
            atlas_height: *mut u32,
        ) -> i32;
    }
    let triangle_indices: Vec<usize> = model
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(index, triangle)| (triangle.material == material).then_some(index))
        .collect();
    if triangle_indices.is_empty() {
        return Err(format!("材质 {material} 没有三角形"));
    }
    // The normalized model stores triangle corners independently. Weld equal
    // positions before sending the mesh to xatlas so it sees the real surface
    // topology and can build coherent charts instead of one chart per face.
    let mut positions = Vec::<[f32; 3]>::with_capacity(triangle_indices.len() * 2);
    let mut indices = Vec::<u32>::with_capacity(triangle_indices.len() * 3);
    let mut welded = std::collections::HashMap::<(usize, u32, u32, u32), u32>::new();
    for &index in &triangle_indices {
        let triangle = &model.triangles[index];
        for position in triangle.positions {
            let key = (
                triangle.object,
                position[0].to_bits(),
                position[1].to_bits(),
                position[2].to_bits(),
            );
            let next = positions.len() as u32;
            let vertex = *welded.entry(key).or_insert_with(|| {
                positions.push(position);
                next
            });
            indices.push(vertex);
        }
    }
    // 焊合后退化的三角形（共点或零面积）不送展开器：xatlas 对这类输入存在
    // 越界访问缺陷（SEH 故障被 catch(...) 吞掉，表现为「展开器内部异常」）。
    let mut corner_ids = Vec::<[u32; 3]>::with_capacity(triangle_indices.len());
    let mut cursor = 0usize;
    for _ in &triangle_indices {
        corner_ids.push([indices[cursor], indices[cursor + 1], indices[cursor + 2]]);
        cursor += 3;
    }
    let degenerate: Vec<bool> = corner_ids
        .iter()
        .map(|c| {
            let (p0, p1, p2) = (
                positions[c[0] as usize],
                positions[c[1] as usize],
                positions[c[2] as usize],
            );
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let cross = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2] <= 1e-24
        })
        .collect();

    let channel = model
        .triangles
        .iter()
        .filter(|t| t.material == material)
        .flat_map(|t| t.uvs.keys().copied())
        .max()
        .map_or(0, |value| value.saturating_add(1));

    // 平面投影回退：取包围盒两个最大延展轴归一化。展开器任何形式的失败
    // （含 SEH 故障被 catch(...) 吞掉）都回退到它，保证材质始终有可用 UV。
    let mut bmin = [f64::MAX; 3];
    let mut bmax = [-f64::MAX; 3];
    for p in &positions {
        for a in 0..3 {
            bmin[a] = bmin[a].min(p[a] as f64);
            bmax[a] = bmax[a].max(p[a] as f64);
        }
    }
    let mut axes = [0usize, 1, 2];
    axes.sort_by(|a, b| {
        (bmax[*b] - bmin[*b])
            .partial_cmp(&(bmax[*a] - bmin[*a]))
            .unwrap()
    });
    let (u_axis, v_axis) = (axes[0], axes[1]);
    let planar = |p: &[f32; 3]| -> [f32; 2] {
        let u = if bmax[u_axis] - bmin[u_axis] > 1e-12 {
            ((p[u_axis] as f64 - bmin[u_axis]) / (bmax[u_axis] - bmin[u_axis])) as f32
        } else {
            0.5
        };
        let v = if bmax[v_axis] - bmin[v_axis] > 1e-12 {
            ((p[v_axis] as f64 - bmin[v_axis]) / (bmax[v_axis] - bmin[v_axis])) as f32
        } else {
            0.5
        };
        [u, 1.0 - v]
    };
    let planar_triangle = |c: &[u32; 3]| -> [[f32; 2]; 3] {
        [
            planar(&positions[c[0] as usize]),
            planar(&positions[c[1] as usize]),
            planar(&positions[c[2] as usize]),
        ]
    };

    let mut generated = vec![[f32::NAN; 2]; indices.len()];
    let live_count = degenerate.iter().filter(|d| !**d).count();
    let mut fallback = live_count == 0;
    if live_count > 0 {
        let mut live_indices = Vec::<u32>::with_capacity(live_count * 3);
        for (local, tri) in corner_ids.iter().enumerate() {
            if !degenerate[local] {
                live_indices.extend_from_slice(tri);
            }
        }
        let mut live_uvs = vec![[f32::NAN; 2]; live_indices.len()];
        let mut width = 0_u32;
        let mut height = 0_u32;
        let status = unsafe {
            aias_xatlas_generate(
                positions.as_ptr().cast::<f32>(),
                live_indices.as_ptr(),
                positions.len() as u32,
                live_indices.len() as u32,
                live_uvs.as_mut_ptr().cast::<f32>(),
                &mut width,
                &mut height,
            )
        };
        if status == -5 {
            return Err(format!(
                "材质 {material} 自动 UV 生成失败：展开器内存不足，请关闭其它程序后重试，或将模型拆分为更小的部分"
            ));
        }
        if status != 0 {
            fallback = true;
        } else {
            let mut offset = 0usize;
            for (local, tri) in corner_ids.iter().enumerate() {
                if degenerate[local] {
                    continue;
                }
                for k in 0..3 {
                    generated[local * 3 + k] = live_uvs[offset + k];
                }
                offset += 3;
            }
        }
    }
    if fallback {
        // 展开器失败：全部三角形（含 live）统一平面投影，不留 NaN 角点
        for (local, tri) in corner_ids.iter().enumerate() {
            let uvs = planar_triangle(tri);
            for k in 0..3 {
                generated[local * 3 + k] = uvs[k];
            }
        }
        model.warnings.push(format!(
            "材质 {material} 自动 UV 展开器异常，已改用简易平面投影（该材质接缝位置与展开器方案不同）"
        ));
    } else {
        // 展开成功：仅退化三角形补平面投影
        for (local, tri) in corner_ids.iter().enumerate() {
            if degenerate[local] {
                let uvs = planar_triangle(tri);
                for k in 0..3 {
                    generated[local * 3 + k] = uvs[k];
                }
            }
        }
    }
    // 任何残留 NaN 角点（防御）：以 0.5 中性填充
    for uv in &mut generated {
        for c in uv {
            if !c.is_finite() {
                *c = 0.5;
            }
        }
    }
    for (local, &triangle_index) in triangle_indices.iter().enumerate() {
        let base = local * 3;
        model.triangles[triangle_index].uvs.insert(
            channel,
            [generated[base], generated[base + 1], generated[base + 2]],
        );
    }
    model.generated_channels.insert(material, channel);
    Ok(channel)
}

fn repair_generated_uv(model: &mut Model, material: usize, channel: u32, report: &UvReport) {
    let bad: std::collections::BTreeSet<usize> = report
        .issues
        .iter()
        .flat_map(|issue| [Some(issue.triangle), issue.other_triangle])
        .flatten()
        .collect();
    if bad.is_empty() {
        return;
    }
    for (index, triangle) in model
        .triangles
        .iter_mut()
        .enumerate()
        .filter(|(_, t)| t.material == material)
    {
        if bad.contains(&index) {
            continue;
        }
        if let Some(uv) = triangle.uvs.get_mut(&channel) {
            for point in uv {
                point[0] *= 0.94;
            }
        }
    }
    let columns = (bad.len() as f32).sqrt().ceil().max(1.0) as usize;
    let rows = bad.len().div_ceil(columns);
    for (slot, index) in bad.into_iter().enumerate() {
        let column = slot % columns;
        let row = slot / columns;
        let x0 = 0.95 + 0.04 * column as f32 / columns as f32;
        let x1 = 0.95 + 0.04 * (column + 1) as f32 / columns as f32;
        let y0 = 0.01 + 0.98 * row as f32 / rows as f32;
        let y1 = 0.01 + 0.98 * (row + 1) as f32 / rows as f32;
        let px = (x1 - x0) * 0.1;
        let py = (y1 - y0) * 0.1;
        model.triangles[index].uvs.insert(
            channel,
            [[x0 + px, y0 + py], [x1 - px, y0 + py], [x0 + px, y1 - py]],
        );
    }
}

/// 极端非流形网格的最终兜底：每个三角形独占网格单元，保证有限、0–1、
/// 非退化且无重叠。正常模型和可局部修复的模型不会走到这里。
fn grid_pack_material(model: &mut Model, material: usize, channel: u32) {
    let indices: Vec<usize> = model
        .triangles
        .iter()
        .enumerate()
        .filter_map(|(index, t)| (t.material == material).then_some(index))
        .collect();
    let columns = (indices.len() as f32).sqrt().ceil().max(1.0) as usize;
    let rows = indices.len().div_ceil(columns);
    for (slot, index) in indices.into_iter().enumerate() {
        let column = slot % columns;
        let row = slot / columns;
        let x0 = column as f32 / columns as f32;
        let x1 = (column + 1) as f32 / columns as f32;
        let y0 = row as f32 / rows as f32;
        let y1 = (row + 1) as f32 / rows as f32;
        let px = (x1 - x0) * 0.08;
        let py = (y1 - y0) * 0.08;
        model.triangles[index].uvs.insert(
            channel,
            [[x0 + px, y0 + py], [x1 - px, y0 + py], [x0 + px, y1 - py]],
        );
    }
}

/// 为每个材质选择实际烘焙通道；智能模式只为不存在合法源通道的材质生成 UV。
pub fn prepare_uvs_with_progress(
    model: &mut Model,
    mode: &str,
    mut progress: impl FnMut(usize, usize),
) -> Result<BTreeMap<usize, u32>, String> {
    let objects: Vec<usize> = model.objects.iter().map(|o| o.id).collect();
    let material_ids: Vec<usize> = model
        .materials
        .iter()
        .filter(|m| model.triangles.iter().any(|t| t.material == m.id))
        .map(|m| m.id)
        .collect();
    let mut selected = BTreeMap::new();
    let total = material_ids.len();
    for (position, material) in material_ids.into_iter().enumerate() {
        progress(position, total);
        let channels: std::collections::BTreeSet<u32> = model
            .triangles
            .iter()
            .filter(|t| t.material == material)
            .flat_map(|t| t.uvs.keys().copied())
            .collect();
        let valid = channels
            .iter()
            .copied()
            .filter(|channel| inspect(model, material, *channel, &objects).valid)
            .collect::<Vec<_>>();
        let selected_channel = match mode {
            "regenerateAll" => generate_material_uv(model, material)?,
            "strictSource" => channels
                .iter()
                .copied()
                .find(|c| *c == 0)
                .or_else(|| channels.iter().next().copied())
                .unwrap_or(0),
            _ => valid
                .iter()
                .copied()
                .find(|c| *c == 0)
                .or_else(|| valid.first().copied())
                .map(Ok)
                .unwrap_or_else(|| generate_material_uv(model, material))?,
        };
        selected.insert(material, selected_channel);
    }
    Ok(selected)
}

#[cfg(test)]
pub fn prepare_uvs(model: &mut Model, mode: &str) -> Result<BTreeMap<usize, u32>, String> {
    prepare_uvs_with_progress(model, mode, |_, _| {})
}

/// 写出供渲染器使用的紧凑预览。偏移量均为文件内字节偏移，数据为小端 f32。
pub fn write_preview(model: &Model, dir: &Path) -> Result<serde_json::Value, String> {
    let path = dir.join("preview.bin");
    let mut writer =
        std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
    let mut offset = 0_u64;
    let mut grouped: BTreeMap<(usize, usize), Vec<(usize, &Triangle)>> = BTreeMap::new();
    for (index, triangle) in model.triangles.iter().enumerate() {
        grouped
            .entry((triangle.object, triangle.material))
            .or_default()
            .push((index, triangle));
    }
    let mut batches = Vec::new();
    for ((object, material), triangles) in grouped {
        let vertex_count = triangles.len() * 3;
        let position_offset = offset;
        for (_, triangle) in &triangles {
            for value in triangle.positions.iter().flatten() {
                writer
                    .write_all(&value.to_le_bytes())
                    .map_err(|e| e.to_string())?;
                offset += 4;
            }
        }
        let normal_offset = offset;
        for (_, triangle) in &triangles {
            for value in triangle.normals.iter().flatten() {
                writer
                    .write_all(&value.to_le_bytes())
                    .map_err(|e| e.to_string())?;
                offset += 4;
            }
        }
        let triangle_offset = offset;
        for (index, _) in &triangles {
            writer
                .write_all(&(*index as u32).to_le_bytes())
                .map_err(|e| e.to_string())?;
            offset += 4;
        }
        let channel_ids: std::collections::BTreeSet<u32> = triangles
            .iter()
            .flat_map(|(_, t)| t.uvs.keys().copied())
            .collect();
        let mut uv_offsets = serde_json::Map::new();
        for channel in channel_ids {
            uv_offsets.insert(channel.to_string(), serde_json::json!(offset));
            for (_, triangle) in &triangles {
                let uv = triangle
                    .uvs
                    // 批内个别三角形缺失该通道时填 0.5 中性 UV：NaN 属性会让
                    // WebGL 整批 draw 消失，drawUv 也画不出对应三角形。
                    .get(&channel)
                    .copied()
                    .unwrap_or([[0.5; 2]; 3]);
                for value in uv.iter().flatten() {
                    writer
                        .write_all(&value.to_le_bytes())
                        .map_err(|e| e.to_string())?;
                    offset += 4;
                }
            }
        }
        batches.push(serde_json::json!({"object":object,"material":material,"vertexCount":vertex_count,"triangleCount":triangles.len(),"positionOffset":position_offset,"normalOffset":normal_offset,"triangleOffset":triangle_offset,"uvOffsets":uv_offsets}));
    }
    writer.flush().map_err(|e| e.to_string())?;
    let manifest_path = dir.join("preview.json");
    let manifest =
        serde_json::json!({"version":1,"byteLength":offset,"bufferPath":path,"batches":batches});
    std::fs::write(
        &manifest_path,
        serde_json::to_vec(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(
        serde_json::json!({"manifestPath":manifest_path,"bufferPath":path,"byteLength":offset,"version":1,"batches":manifest["batches"]}),
    )
}

fn export_name(value: &str) -> String {
    let cleaned: String = value
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
    let cleaned = cleaned.trim_matches([' ', '.']);
    if cleaned.is_empty() {
        "model".into()
    } else {
        cleaned.into()
    }
}

fn export_obj(
    model: &Model,
    channels: &BTreeMap<usize, u32>,
    output: &Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    let stem = format!("{}_bake", export_name(&model.name));
    let obj_path = output.join(format!("{stem}.obj"));
    let mtl_path = output.join(format!("{stem}.mtl"));
    let mut obj =
        std::io::BufWriter::new(std::fs::File::create(&obj_path).map_err(|e| e.to_string())?);
    writeln!(obj, "# AIAS bake-ready static mesh\nmtllib {stem}.mtl").map_err(|e| e.to_string())?;
    let mut index = 1usize;
    let mut active_object = usize::MAX;
    let mut active_material = usize::MAX;
    for triangle in &model.triangles {
        if triangle.object != active_object {
            active_object = triangle.object;
            active_material = usize::MAX;
            writeln!(
                obj,
                "o {}",
                export_name(&model.objects[triangle.object].name)
            )
            .map_err(|e| e.to_string())?;
        }
        if triangle.material != active_material {
            active_material = triangle.material;
            writeln!(
                obj,
                "usemtl {}",
                export_name(&model.materials[triangle.material].name)
            )
            .map_err(|e| e.to_string())?;
        }
        let channel = channels.get(&triangle.material).copied().unwrap_or(0);
        let uv = triangle
            .uvs
            .get(&channel)
            .ok_or_else(|| format!("材质 {} 缺少导出 UV 通道 {channel}", triangle.material))?;
        for p in triangle.positions {
            writeln!(obj, "v {} {} {}", p[0], p[1], p[2]).map_err(|e| e.to_string())?;
        }
        for t in uv {
            writeln!(obj, "vt {} {}", t[0], t[1]).map_err(|e| e.to_string())?;
        }
        for n in triangle.normals {
            writeln!(obj, "vn {} {} {}", n[0], n[1], n[2]).map_err(|e| e.to_string())?;
        }
        writeln!(
            obj,
            "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}",
            index,
            index + 1,
            index + 2
        )
        .map_err(|e| e.to_string())?;
        index += 3;
    }
    obj.flush().map_err(|e| e.to_string())?;
    let mut mtl =
        std::io::BufWriter::new(std::fs::File::create(&mtl_path).map_err(|e| e.to_string())?);
    writeln!(
        mtl,
        "# Material slots only. Baked outputs are listed in bake-manifest.json."
    )
    .map_err(|e| e.to_string())?;
    for material in &model.materials {
        writeln!(
            mtl,
            "\nnewmtl {}\nKd 0.8 0.8 0.8\nKa 0 0 0\nKs 0 0 0\nd 1",
            export_name(&material.name)
        )
        .map_err(|e| e.to_string())?;
    }
    mtl.flush().map_err(|e| e.to_string())?;
    Ok(vec![obj_path, mtl_path])
}

fn append_f32(buffer: &mut Vec<u8>, value: f32) {
    buffer.extend_from_slice(&value.to_le_bytes());
}

fn export_glb(
    model: &Model,
    channels: &BTreeMap<usize, u32>,
    output: &Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    let path = output.join(format!("{}_bake.glb", export_name(&model.name)));
    let mut binary = Vec::<u8>::new();
    let mut views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut by_object: BTreeMap<usize, BTreeMap<usize, Vec<&Triangle>>> = BTreeMap::new();
    for triangle in &model.triangles {
        by_object
            .entry(triangle.object)
            .or_default()
            .entry(triangle.material)
            .or_default()
            .push(triangle);
    }
    for (object, materials) in by_object {
        let mut primitives = Vec::new();
        for (material, triangles) in materials {
            let count = triangles.len() * 3;
            let position_offset = binary.len();
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for triangle in &triangles {
                for p in triangle.positions {
                    for axis in 0..3 {
                        min[axis] = min[axis].min(p[axis]);
                        max[axis] = max[axis].max(p[axis]);
                        append_f32(&mut binary, p[axis]);
                    }
                }
            }
            let position_view = views.len();
            views.push(serde_json::json!({"buffer":0,"byteOffset":position_offset,"byteLength":count*12,"target":34962}));
            let position_accessor = accessors.len();
            accessors.push(serde_json::json!({"bufferView":position_view,"componentType":5126,"count":count,"type":"VEC3","min":min,"max":max}));
            let normal_offset = binary.len();
            for triangle in &triangles {
                for n in triangle.normals {
                    for value in n {
                        append_f32(&mut binary, value);
                    }
                }
            }
            let normal_view = views.len();
            views.push(serde_json::json!({"buffer":0,"byteOffset":normal_offset,"byteLength":count*12,"target":34962}));
            let normal_accessor = accessors.len();
            accessors.push(serde_json::json!({"bufferView":normal_view,"componentType":5126,"count":count,"type":"VEC3"}));
            let uv_offset = binary.len();
            let channel = channels.get(&material).copied().unwrap_or(0);
            for triangle in &triangles {
                let uv = triangle
                    .uvs
                    .get(&channel)
                    .ok_or_else(|| format!("材质 {material} 缺少导出 UV 通道 {channel}"))?;
                for point in uv {
                    append_f32(&mut binary, point[0]);
                    append_f32(&mut binary, 1.0 - point[1]);
                }
            }
            let uv_view = views.len();
            views.push(serde_json::json!({"buffer":0,"byteOffset":uv_offset,"byteLength":count*8,"target":34962}));
            let uv_accessor = accessors.len();
            accessors.push(serde_json::json!({"bufferView":uv_view,"componentType":5126,"count":count,"type":"VEC2"}));
            primitives.push(serde_json::json!({"attributes":{"POSITION":position_accessor,"NORMAL":normal_accessor,"TEXCOORD_0":uv_accessor},"material":material,"mode":4}));
        }
        let mesh_index = meshes.len();
        meshes.push(serde_json::json!({"name":model.objects[object].name,"primitives":primitives}));
        nodes.push(serde_json::json!({"name":model.objects[object].name,"mesh":mesh_index}));
    }
    let scene_nodes: Vec<usize> = (0..nodes.len()).collect();
    let materials: Vec<_> = model.materials.iter().map(|m| serde_json::json!({"name":m.name,"pbrMetallicRoughness":{"baseColorFactor":[0.8,0.8,0.8,1.0],"metallicFactor":0,"roughnessFactor":1}})).collect();
    let json = serde_json::json!({
        "asset":{"version":"2.0","generator":"AIAS Model Bake"},"scene":0,"scenes":[{"nodes":scene_nodes}],
        "nodes":nodes,"meshes":meshes,"materials":materials,"buffers":[{"byteLength":binary.len()}],
        "bufferViews":views,"accessors":accessors
    });
    let mut json_bytes = serde_json::to_vec(&json).map_err(|e| e.to_string())?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    while binary.len() % 4 != 0 {
        binary.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + binary.len();
    let mut writer =
        std::io::BufWriter::new(std::fs::File::create(&path).map_err(|e| e.to_string())?);
    writer
        .write_all(&0x46546C67_u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&2_u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&(total as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&(json_bytes.len() as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&0x4E4F534A_u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&json_bytes).map_err(|e| e.to_string())?;
    writer
        .write_all(&(binary.len() as u32).to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer
        .write_all(&0x004E4942_u32.to_le_bytes())
        .map_err(|e| e.to_string())?;
    writer.write_all(&binary).map_err(|e| e.to_string())?;
    writer.flush().map_err(|e| e.to_string())?;
    Ok(vec![path])
}

pub fn export_bake_model(
    model: &Model,
    channels: &BTreeMap<usize, u32>,
    output: &Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    match model.source_format.as_str() {
        "obj" => export_obj(model, channels, output),
        "gltf" | "glb" => export_glb(model, channels, output),
        _ => Err("无法确定烘焙模型的导出格式".into()),
    }
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
    // Clipping shared edges in f32 can manufacture a nonzero polygon area.
    // Promote the stored UV coordinates before all intersection arithmetic.
    use glam::DVec2;
    let cross = |a: DVec2, b: DVec2| a.x * b.y - a.y * b.x;
    let point = |v: [f32; 2]| DVec2::new(v[0] as f64, v[1] as f64);
    let mut polygon: Vec<DVec2> = a.into_iter().map(point).collect();
    let b = b.map(point);
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
    let origin = polygon.first().copied().unwrap_or_default();
    let area = (0..polygon.len())
        .map(|i| {
            cross(
                polygon[i] - origin,
                polygon[(i + 1) % polygon.len()] - origin,
            )
        })
        .sum::<f64>()
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
        // 明细只保留前 256 条：前端每材质只渲染 30 条，其余用于视口标红；
        // 总数单独保留，避免无效源 UV 把紧凑预览 manifest 膨胀到数 MiB。
        if issues.len() < 256 {
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

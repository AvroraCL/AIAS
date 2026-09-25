use image_dds::ddsfile::Dds;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackIssue {
    level: &'static str,
    message: String,
    file: Option<String>,
    line: Option<usize>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackFile {
    name: String,
    size: u64,
    width: Option<u32>,
    height: Option<u32>,
    mip_levels: Option<u32>,
    sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackScan {
    blks: Vec<String>,
    selected_blk: Option<String>,
    included: Vec<PackFile>,
    excluded: Vec<String>,
    issues: Vec<PackIssue>,
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PackExport {
    directory: String,
    selected_blk: String,
    package_name: String,
    output_path: String,
    expected_fingerprint: String,
    expected_output: Option<String>,
}

#[derive(Debug)]
struct Reference {
    to: String,
    line: usize,
}

fn issue(
    level: &'static str,
    message: impl Into<String>,
    file: Option<&str>,
    line: Option<usize>,
) -> PackIssue {
    PackIssue {
        level,
        message: message.into(),
        file: file.map(str::to_owned),
        line,
    }
}

fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || matches!(name, "." | "..")
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return false;
    }
    !matches!(
        name.split('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "con"
            | "prn"
            | "aux"
            | "nul"
            | "com1"
            | "com2"
            | "com3"
            | "com4"
            | "com5"
            | "com6"
            | "com7"
            | "com8"
            | "com9"
            | "lpt1"
            | "lpt2"
            | "lpt3"
            | "lpt4"
            | "lpt5"
            | "lpt6"
            | "lpt7"
            | "lpt8"
            | "lpt9"
    )
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escaped = false;
    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' && quoted {
            escaped = true;
            continue;
        }
        if ch == '"' {
            quoted = !quoted;
        }
        if ch == '/' && !quoted && line[index..].starts_with("//") {
            return &line[..index];
        }
    }
    line
}

fn field(line: &str, key: &str) -> Option<String> {
    let value = line
        .strip_prefix(key)?
        .trim_start()
        .strip_prefix('=')?
        .trim_start();
    let value = value.strip_prefix('"')?.strip_suffix('"')?;
    if value.contains('"') || value.contains('\\') || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_owned())
}

fn parse_blk(content: &str, name: &str, issues: &mut Vec<PackIssue>) -> Vec<Reference> {
    let mut refs = Vec::new();
    let mut block: Option<(&str, usize, Option<String>, Option<String>)> = None;
    let mut seen_from = HashSet::new();
    for (index, raw) in content.trim_start_matches('\u{feff}').lines().enumerate() {
        let line_no = index + 1;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some((kind, start, from, to)) = &mut block {
            if line == "}" {
                match (from.take(), to.take()) {
                    (Some(from), Some(to)) if !from.is_empty() && !to.is_empty() => {
                        if !seen_from.insert(from.to_lowercase()) {
                            issues.push(issue(
                                "error",
                                format!("原贴图名 {from} 重复。"),
                                Some(name),
                                Some(*start),
                            ));
                        }
                        refs.push(Reference { to, line: *start });
                    }
                    _ => issues.push(issue(
                        "error",
                        format!("{kind} 缺少 from 或 to。"),
                        Some(name),
                        Some(*start),
                    )),
                }
                block = None;
            } else if let Some(value) = field(line, "from:t") {
                if from.replace(value).is_some() {
                    issues.push(issue(
                        "error",
                        "重复的 from 字段。",
                        Some(name),
                        Some(line_no),
                    ));
                }
            } else if let Some(value) = field(line, "to:t") {
                if to.replace(value).is_some() {
                    issues.push(issue(
                        "error",
                        "重复的 to 字段。",
                        Some(name),
                        Some(line_no),
                    ));
                }
            } else if field(line, "param:t").is_none() {
                issues.push(issue(
                    "error",
                    "无法解析的映射字段。",
                    Some(name),
                    Some(line_no),
                ));
            }
        } else if let Some(kind) = line.strip_suffix('{').map(str::trim) {
            if kind == "replace_tex" || kind == "set_tex" {
                block = Some((kind, line_no, None, None));
            } else {
                issues.push(issue(
                    "error",
                    format!("无法解析的 BLK 规则块 {kind}。"),
                    Some(name),
                    Some(line_no),
                ));
            }
        } else if field(line, "name:t").is_none() {
            issues.push(issue(
                "error",
                "无法解析的 BLK 内容。",
                Some(name),
                Some(line_no),
            ));
        }
    }
    if let Some((kind, start, _, _)) = block {
        issues.push(issue(
            "error",
            format!("{kind} 规则块没有结束。"),
            Some(name),
            Some(start),
        ));
    }
    if refs.is_empty() {
        issues.push(issue(
            "error",
            "BLK 没有可打包的贴图映射。",
            Some(name),
            None,
        ));
    }
    refs
}

fn hash_file(path: &Path) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    io::copy(&mut file, &mut hash).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hash.finalize()))
}

fn validate_texture(
    path: &Path,
    name: &str,
    issues: &mut Vec<PackIssue>,
) -> (Option<u32>, Option<u32>, Option<u32>) {
    if name.to_ascii_lowercase().ends_with(".dds") {
        match fs::File::open(path)
            .and_then(|file| Dds::read(&mut io::BufReader::new(file)).map_err(io::Error::other))
        {
            Ok(dds) => {
                if let Err(error) = dds.get_data(0) {
                    issues.push(issue(
                        "error",
                        format!("DDS 图像数据不完整：{error}"),
                        Some(name),
                        None,
                    ));
                    return (None, None, None);
                }
                let (w, h, mips) = (
                    dds.get_width(),
                    dds.get_height(),
                    dds.get_num_mipmap_levels(),
                );
                if mips <= 1 {
                    issues.push(issue(
                        "warning",
                        "DDS 没有完整 Mipmap 链，远景可能闪烁。",
                        Some(name),
                        None,
                    ));
                }
                if !w.is_power_of_two() || !h.is_power_of_two() || w > 4096 || h > 4096 {
                    issues.push(issue(
                        "warning",
                        format!("贴图尺寸 {w}×{h} 不属于常见投稿尺寸。"),
                        Some(name),
                        None,
                    ));
                }
                (Some(w), Some(h), Some(mips))
            }
            Err(error) => {
                issues.push(issue(
                    "error",
                    format!("DDS 无法解码：{error}"),
                    Some(name),
                    None,
                ));
                (None, None, None)
            }
        }
    } else {
        match image::ImageReader::open(path)
            .and_then(|reader| reader.with_guessed_format())
            .and_then(|reader| reader.decode().map_err(io::Error::other))
        {
            Ok(image) => {
                let (w, h) = (image.width(), image.height());
                if !w.is_power_of_two() || !h.is_power_of_two() || w > 4096 || h > 4096 {
                    issues.push(issue(
                        "warning",
                        format!("贴图尺寸 {w}×{h} 不属于常见投稿尺寸。"),
                        Some(name),
                        None,
                    ));
                }
                (Some(w), Some(h), None)
            }
            Err(error) => {
                issues.push(issue(
                    "error",
                    format!("TGA 无法解码：{error}"),
                    Some(name),
                    None,
                ));
                (None, None, None)
            }
        }
    }
}

fn scan(directory: &str, selected_blk: Option<&str>) -> Result<PackScan, String> {
    let dir = Path::new(directory);
    if !dir.is_dir() {
        return Err("请选择已有的涂装目录。".into());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_type().map_err(|e| e.to_string())?.is_file() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    let blks: Vec<_> = names
        .iter()
        .filter(|n| n.to_ascii_lowercase().ends_with(".blk"))
        .cloned()
        .collect();
    let mut issues = Vec::new();
    let mut normalized = HashSet::new();
    for name in &names {
        if !normalized.insert(name.to_lowercase()) {
            issues.push(issue(
                "error",
                format!("目录中存在大小写冲突的文件名：{name}。"),
                Some(name),
                None,
            ));
        }
    }
    let chosen = match selected_blk {
        Some(value) if blks.iter().any(|n| n == value) => Some(value.to_owned()),
        Some(_) => {
            issues.push(issue(
                "error",
                "所选 BLK 已不存在，请重新选择。",
                None,
                None,
            ));
            None
        }
        None if blks.len() == 1 => Some(blks[0].clone()),
        None if blks.is_empty() => {
            issues.push(issue("error", "目录中没有 BLK，请先生成。", None, None));
            None
        }
        None => {
            issues.push(issue(
                "error",
                "目录中有多份 BLK，请选择本次打包的一份。",
                None,
                None,
            ));
            None
        }
    };
    let mut included = Vec::new();
    let mut used = HashSet::new();
    if let Some(ref blk) = chosen {
        if !safe_name(blk) {
            issues.push(issue("error", "BLK 文件名不安全。", Some(blk), None));
        } else {
            let bytes = fs::read(dir.join(blk)).map_err(|e| e.to_string())?;
            match String::from_utf8(bytes) {
                Ok(content) => {
                    let refs = parse_blk(&content, blk, &mut issues);
                    for reference in refs {
                        let target = &reference.to;
                        if !safe_name(target) {
                            issues.push(issue(
                                "error",
                                format!("贴图路径 {target} 不安全或不在当前目录。"),
                                Some(blk),
                                Some(reference.line),
                            ));
                            continue;
                        }
                        if !target.to_ascii_lowercase().ends_with(".dds")
                            && !target.to_ascii_lowercase().ends_with(".tga")
                        {
                            issues.push(issue(
                                "error",
                                format!("贴图 {target} 不是支持的 DDS/TGA 格式。"),
                                Some(blk),
                                Some(reference.line),
                            ));
                            continue;
                        }
                        if !names.contains(target) {
                            let case_only = names.iter().any(|n| n.eq_ignore_ascii_case(target));
                            issues.push(issue(
                                "error",
                                if case_only {
                                    format!("贴图 {target} 的文件名大小写与 BLK 不一致。")
                                } else {
                                    format!("找不到贴图 {target}。")
                                },
                                Some(blk),
                                Some(reference.line),
                            ));
                            continue;
                        }
                        if !used.insert(target.clone()) {
                            continue;
                        }
                        let path = dir.join(target);
                        let (width, height, mip_levels) =
                            validate_texture(&path, target, &mut issues);
                        let size = fs::metadata(&path).map_err(|e| e.to_string())?.len();
                        included.push(PackFile {
                            name: target.clone(),
                            size,
                            width,
                            height,
                            mip_levels,
                            sha256: hash_file(&path)?,
                        });
                    }
                }
                Err(_) => issues.push(issue(
                    "error",
                    "BLK 不是 UTF-8/ASCII 文本。",
                    Some(blk),
                    None,
                )),
            }
            used.insert(blk.clone());
            included.insert(
                0,
                PackFile {
                    name: blk.clone(),
                    size: fs::metadata(dir.join(blk))
                        .map_err(|e| e.to_string())?
                        .len(),
                    width: None,
                    height: None,
                    mip_levels: None,
                    sha256: hash_file(&dir.join(blk))?,
                },
            );
        }
    }
    let excluded: Vec<_> = names.into_iter().filter(|n| !used.contains(n)).collect();
    if !excluded.is_empty() {
        issues.push(issue(
            "warning",
            format!(
                "另有 {} 个文件未被 BLK 引用，不会收入 ZIP。",
                excluded.len()
            ),
            None,
            None,
        ));
    }
    let mut digest = Sha256::new();
    for file in &included {
        digest.update(file.name.as_bytes());
        digest.update([0]);
        digest.update(file.sha256.as_bytes());
        digest.update([0]);
    }
    let fingerprint = format!("{:x}", digest.finalize());
    Ok(PackScan {
        blks,
        selected_blk: chosen,
        included,
        excluded,
        issues,
        fingerprint,
    })
}

#[tauri::command]
pub(crate) async fn skin_pack_scan(
    directory: String,
    selected_blk: Option<String>,
) -> Result<PackScan, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&directory, selected_blk.as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn skin_pack_export(options: PackExport) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || export(options))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn skin_pack_output_hash(path: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = Path::new(&path);
        if !path.exists() {
            return Ok(None);
        }
        if !path.is_file() {
            return Err("ZIP 目标不是文件。".into());
        }
        hash_file(path).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn export(options: PackExport) -> Result<String, String> {
    if !safe_name(&options.package_name) {
        return Err("包名无效。".into());
    }
    let output = PathBuf::from(&options.output_path);
    if !output
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".zip"))
    {
        return Err("请选择 ZIP 输出文件。".into());
    }
    let scanned = scan(&options.directory, Some(&options.selected_blk))?;
    if scanned.issues.iter().any(|issue| issue.level == "error") {
        return Err("涂装检查未通过，请重新扫描。".into());
    }
    if scanned.fingerprint != options.expected_fingerprint {
        return Err("贴图或 BLK 在预览后发生变化，请重新扫描。".into());
    }
    let output_before = if output.exists() {
        Some(hash_file(&output)?)
    } else {
        None
    };
    if output_before != options.expected_output {
        return Err("目标 ZIP 在确认后发生变化，请重新选择保存位置。".into());
    }
    let parent = output.parent().ok_or("输出路径无效。")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    {
        let mut zip = zip::ZipWriter::new(temp.as_file_mut());
        let folder = format!("{}/", options.package_name);
        zip.add_directory(&folder, zip::write::SimpleFileOptions::default())
            .map_err(|e| e.to_string())?;
        for file in &scanned.included {
            let method = if file.name.to_ascii_lowercase().ends_with(".dds") {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            zip.start_file(
                format!("{folder}{}", file.name),
                zip::write::SimpleFileOptions::default().compression_method(method),
            )
            .map_err(|e| e.to_string())?;
            let mut source = fs::File::open(Path::new(&options.directory).join(&file.name))
                .map_err(|e| e.to_string())?;
            let mut hash = Sha256::new();
            let mut buffer = [0u8; 65536];
            loop {
                let count = source.read(&mut buffer).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                zip.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
                hash.update(&buffer[..count]);
            }
            if format!("{:x}", hash.finalize()) != file.sha256 {
                return Err(format!("{} 在打包时发生变化，请重新扫描。", file.name));
            }
        }
        zip.finish().map_err(|e| e.to_string())?;
    }
    let after = scan(&options.directory, Some(&options.selected_blk))?;
    if after.fingerprint != scanned.fingerprint
        || after.issues.iter().any(|item| item.level == "error")
    {
        return Err("源文件在打包期间发生变化，请重新扫描。".into());
    }
    let output_now = if output.exists() {
        Some(hash_file(&output)?)
    } else {
        None
    };
    if output_now != output_before {
        return Err("目标 ZIP 在打包期间发生变化，旧文件未被覆盖。".into());
    }
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(&output).map_err(|e| e.to_string())?;
    Ok(output.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let dds = crate::encode_dds(&image::RgbaImage::new(4, 4), "DXT5").unwrap();
        fs::write(dir.path().join("body_c.dds"), dds).unwrap();
        image::RgbaImage::new(4, 4)
            .save(dir.path().join("track.tga"))
            .unwrap();
        let blk = "name:t=\"user\"\r\n\r\nreplace_tex{\r\n  from:t=\"body_c*\"\r\n  to:t=\"body_c.dds\"\r\n}\r\n\r\nset_tex{ // second source\r\n  from:t=\"other_c*\"\r\n  to:t=\"body_c.dds\"\r\n}\r\n\r\nreplace_tex{\r\n  from:t=\"track*\"\r\n  to:t=\"track.tga\"\r\n}\r\n";
        fs::write(dir.path().join("skin.blk"), blk).unwrap();
        fs::write(dir.path().join("unused.txt"), "draft").unwrap();
        (dir, blk.into())
    }

    #[test]
    fn scan_and_zip_include_only_referenced_files() {
        let (dir, blk) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        let scanned = scan(&path, None).unwrap();
        assert_eq!(scanned.selected_blk.as_deref(), Some("skin.blk"));
        assert_eq!(scanned.included.len(), 3);
        assert_eq!(scanned.excluded, vec!["unused.txt"]);
        assert!(!scanned.issues.iter().any(|item| item.level == "error"));
        let output = dir.path().join("output.zip");
        export(PackExport {
            directory: path,
            selected_blk: "skin.blk".into(),
            package_name: "skin".into(),
            output_path: output.to_string_lossy().to_string(),
            expected_fingerprint: scanned.fingerprint,
            expected_output: None,
        })
        .unwrap();
        let mut archive = zip::ZipArchive::new(fs::File::open(output).unwrap()).unwrap();
        assert_eq!(archive.len(), 4);
        let mut content = String::new();
        archive
            .by_name("skin/skin.blk")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, blk);
        assert!(archive.by_name("skin/body_c.dds").is_ok());
        assert!(archive.by_name("skin/track.tga").is_ok());
    }

    #[test]
    fn rejects_stale_files_and_preserves_old_zip() {
        let (dir, _) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        let scanned = scan(&path, None).unwrap();
        let output = dir.path().join("output.zip");
        fs::write(&output, "old archive").unwrap();
        fs::write(dir.path().join("body_c.dds"), "changed").unwrap();
        let error = export(PackExport {
            directory: path,
            selected_blk: "skin.blk".into(),
            package_name: "skin".into(),
            output_path: output.to_string_lossy().to_string(),
            expected_fingerprint: scanned.fingerprint,
            expected_output: Some(hash_file(&output).unwrap()),
        })
        .unwrap_err();
        assert!(error.contains("检查未通过") || error.contains("变化"));
        assert_eq!(fs::read(output).unwrap(), b"old archive");
    }

    #[test]
    fn reports_missing_case_changed_and_unsafe_references() {
        let (dir, _) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        fs::write(
            dir.path().join("skin.blk"),
            "name:t=\"user\"\nreplace_tex{\nfrom:t=\"x*\"\nto:t=\"BODY_C.DDS\"\n}\n",
        )
        .unwrap();
        let scanned = scan(&path, None).unwrap();
        assert!(scanned
            .issues
            .iter()
            .any(|item| item.message.contains("大小写")));
        fs::write(
            dir.path().join("skin.blk"),
            "name:t=\"user\"\nreplace_tex{\nfrom:t=\"x*\"\nto:t=\"../outside.dds\"\n}\n",
        )
        .unwrap();
        let scanned = scan(&path, None).unwrap();
        assert!(scanned
            .issues
            .iter()
            .any(|item| item.message.contains("不安全")));
    }

    #[test]
    fn requires_blk_selection_and_rejects_truncated_dds() {
        let (dir, _) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        fs::copy(dir.path().join("skin.blk"), dir.path().join("second.blk")).unwrap();
        let scanned = scan(&path, None).unwrap();
        assert!(scanned
            .issues
            .iter()
            .any(|item| item.message.contains("多份 BLK")));
        fs::write(dir.path().join("body_c.dds"), vec![0u8; 128]).unwrap();
        let scanned = scan(&path, Some("skin.blk")).unwrap();
        assert!(scanned
            .issues
            .iter()
            .any(|item| item.level == "error" && item.file.as_deref() == Some("body_c.dds")));
    }

    #[test]
    fn changed_output_is_never_overwritten() {
        let (dir, _) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        let scanned = scan(&path, None).unwrap();
        let output = dir.path().join("output.zip");
        fs::write(&output, b"newer archive").unwrap();
        let error = export(PackExport {
            directory: path,
            selected_blk: "skin.blk".into(),
            package_name: "skin".into(),
            output_path: output.to_string_lossy().to_string(),
            expected_fingerprint: scanned.fingerprint,
            expected_output: Some("previous hash".into()),
        })
        .unwrap_err();
        assert!(error.contains("目标 ZIP"));
        assert_eq!(fs::read(output).unwrap(), b"newer archive");
    }

    #[test]
    fn confirmed_existing_zip_is_replaced_atomically() {
        let (dir, _) = fixture();
        let path = dir.path().to_string_lossy().to_string();
        let scanned = scan(&path, None).unwrap();
        let output = dir.path().join("output.zip");
        fs::write(&output, b"old archive").unwrap();
        let previous = hash_file(&output).unwrap();
        export(PackExport {
            directory: path,
            selected_blk: "skin.blk".into(),
            package_name: "skin".into(),
            output_path: output.to_string_lossy().to_string(),
            expected_fingerprint: scanned.fingerprint,
            expected_output: Some(previous),
        }).unwrap();
        assert!(zip::ZipArchive::new(fs::File::open(&output).unwrap()).is_ok());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
    }
}

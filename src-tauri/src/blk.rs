use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs, io::Write, path::Path};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlkScan {
    files: Vec<String>,
    existing_content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlkRule {
    to: String,
    from: String,
    enabled: bool,
    command: String,
    camo_skin_tex: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BlkExport {
    directory: String,
    file_name: String,
    rules: Vec<BlkRule>,
    expected_existing: Option<String>,
    preview_content: String,
}

fn file_stem(name: &str) -> Result<&str, String> {
    let stem = if name.to_ascii_lowercase().ends_with(".blk") {
        &name[..name.len() - 4]
    } else {
        name
    };
    if stem.is_empty()
        || matches!(stem, "." | "..")
        || stem.ends_with(['.', ' '])
        || stem
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        || matches!(
            stem.split('.')
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
    {
        return Err("BLK 文件名无效。".into());
    }
    Ok(stem)
}

fn scan(directory: &str, name: &str) -> Result<BlkScan, String> {
    let dir = Path::new(directory);
    if !dir.is_dir() {
        return Err("请选择已有的 DDS 目录。".into());
    }
    let stem = file_stem(name)?;
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().ends_with(".dds") {
            files.push(name);
        }
    }
    files.sort_by_key(|name| name.to_lowercase());
    let output = dir.join(format!("{stem}.blk"));
    let existing_content = if output.exists() {
        Some(fs::read_to_string(output).map_err(|e| format!("无法读取已有 BLK：{e}"))?)
    } else {
        None
    };
    Ok(BlkScan {
        files,
        existing_content,
    })
}

fn render(rules: &[BlkRule], files: &[String]) -> Result<String, String> {
    let available: HashSet<String> = files.iter().map(|f| f.to_lowercase()).collect();
    let mut seen = HashSet::new();
    let mut result = String::from("name:t=\"user\"\r\n");
    let mut count = 0;
    for rule in rules.iter().filter(|rule| rule.enabled) {
        count += 1;
        if rule.to.to_ascii_lowercase().ends_with("_n.dds") && rule.command != "replace_tex" {
            return Err(format!("{} 固定使用 replace_tex。", rule.to));
        }
        if rule.command != "replace_tex" && rule.command != "set_tex" {
            return Err(format!("请为 {} 选择规则指令。", rule.to));
        }
        if rule.from.is_empty()
            || rule
                .from
                .chars()
                .any(|c| c.is_control() || "\"{}".contains(c))
        {
            return Err(format!("请检查 {} 的原贴图名。", rule.to));
        }
        if !seen.insert(rule.from.to_lowercase()) {
            return Err(format!("原贴图名 {} 重复。", rule.from));
        }
        if !available.contains(&rule.to.to_lowercase())
            || !rule.to.to_ascii_lowercase().ends_with(".dds")
            || rule
                .to
                .chars()
                .any(|c| c.is_control() || "\"{}/\\".contains(c))
        {
            return Err(format!(
                "贴图 {} 已不存在或文件名无效，请重新扫描。",
                rule.to
            ));
        }
        result.push_str(&format!(
            "\r\n{}{{\r\n  from:t=\"{}\"\r\n  to:t=\"{}\"\r\n",
            rule.command, rule.from, rule.to
        ));
        if rule.command == "set_tex" && rule.camo_skin_tex {
            result.push_str("  param:t=\"camo_skin_tex\"\r\n");
        }
        result.push_str("}\r\n");
    }
    if count == 0 {
        return Err("请至少保留一条贴图规则。".into());
    }
    Ok(result)
}

#[tauri::command]
pub(crate) async fn blk_scan(directory: String, file_name: String) -> Result<BlkScan, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&directory, &file_name))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) async fn blk_export(options: BlkExport) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || export(options))
        .await
        .map_err(|e| e.to_string())?
}

fn export(options: BlkExport) -> Result<String, String> {
    let scanned = scan(&options.directory, &options.file_name)?;
    let current: HashSet<String> = scanned
        .files
        .iter()
        .map(|file| file.to_lowercase())
        .collect();
    let reviewed: HashSet<String> = options
        .rules
        .iter()
        .map(|rule| rule.to.to_lowercase())
        .collect();
    if current != reviewed {
        return Err("DDS 文件列表发生变化，请重新扫描。".into());
    }
    if scanned.existing_content != options.expected_existing {
        return Err("已有 BLK 在预览后发生变化，请重新检查并确认。".into());
    }
    let content = render(&options.rules, &scanned.files)?;
    if content != options.preview_content {
        return Err("预览内容已变化，请重新预览后生成。".into());
    }
    let path =
        Path::new(&options.directory).join(format!("{}.blk", file_stem(&options.file_name)?));
    let mut temp =
        tempfile::NamedTempFile::new_in(&options.directory).map_err(|e| e.to_string())?;
    temp.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    temp.persist(&path).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_sample_shape_and_rejects_duplicate_sources() {
        let files: Vec<String> = vec![
            "mig_21_bis_finland_c.dds".into(),
            "mig_21_bis_finland_n.dds".into(),
        ];
        let mut rules = vec![
            BlkRule {
                to: files[0].clone(),
                from: "mig_21_bis_finland_c*".into(),
                enabled: true,
                command: "set_tex".into(),
                camo_skin_tex: false,
            },
            BlkRule {
                to: files[1].clone(),
                from: "mig_21_bis_finland_n*".into(),
                enabled: true,
                command: "replace_tex".into(),
                camo_skin_tex: false,
            },
        ];
        let text = render(&rules, &files).unwrap();
        assert!(text.contains("set_tex{\r\n  from:t=\"mig_21_bis_finland_c*\""));
        assert!(text.contains("replace_tex{\r\n  from:t=\"mig_21_bis_finland_n*\""));
        rules.push(BlkRule {
            to: files[0].clone(),
            from: "legacy_c*".into(),
            enabled: true,
            command: "replace_tex".into(),
            camo_skin_tex: false,
        });
        assert_eq!(
            render(&rules, &files)
                .unwrap()
                .matches(&format!("to:t=\"{}\"", files[0]))
                .count(),
            2
        );
        rules[1].from = rules[0].from.clone();
        assert!(render(&rules, &files).unwrap_err().contains("重复"));
        rules[1].from = "mig_21_bis_finland_n*".into();
        rules[1].command = "set_tex".into();
        assert!(render(&rules, &files).unwrap_err().contains("固定使用"));
    }

    #[test]
    fn scans_and_exports_without_overwriting_stale_content() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("body_c.dds"), b"dds").unwrap();
        fs::write(dir.path().join("body_n.DDS"), b"dds").unwrap();
        fs::write(dir.path().join("old.blk"), b"old").unwrap();
        let path = dir.path().to_string_lossy().to_string();
        let scanned = scan(&path, "old").unwrap();
        assert_eq!(scanned.files.len(), 2);
        let rule = BlkRule {
            to: "body_c.dds".into(),
            from: "body_c*".into(),
            enabled: true,
            command: "replace_tex".into(),
            camo_skin_tex: false,
        };
        let content = render(std::slice::from_ref(&rule), &scanned.files).unwrap();
        let options = || BlkExport {
            directory: path.clone(),
            file_name: "old".into(),
            rules: vec![
                BlkRule {
                    to: rule.to.clone(),
                    from: rule.from.clone(),
                    enabled: true,
                    command: rule.command.clone(),
                    camo_skin_tex: false,
                },
                BlkRule {
                    to: "body_n.DDS".into(),
                    from: "body_n*".into(),
                    enabled: false,
                    command: String::new(),
                    camo_skin_tex: false,
                },
            ],
            expected_existing: Some("old".into()),
            preview_content: content.clone(),
        };
        fs::write(dir.path().join("old.blk"), b"changed").unwrap();
        assert!(export(options()).unwrap_err().contains("发生变化"));
        assert_eq!(
            fs::read_to_string(dir.path().join("old.blk")).unwrap(),
            "changed"
        );
        fs::write(dir.path().join("old.blk"), b"old").unwrap();
        export(options()).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("old.blk")).unwrap(),
            content
        );
        fs::write(dir.path().join("new.dds"), b"dds").unwrap();
        assert!(export(options()).unwrap_err().contains("文件列表发生变化"));
    }
}

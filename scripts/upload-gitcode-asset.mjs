// 通过 GitCode upload_url 预签名接口上传 release 附件（stable）
// 用法：node scripts/upload-gitcode-asset.mjs <本地文件> [远端文件名]
// 同名附件已存在时自动先删除再上传（GitCode 不允许同名覆盖）。
import { readFileSync } from "node:fs";

const API = "https://api.gitcode.com/api/v5/repos/HelenaSG/Aias/releases";
const TAG = "stable";
const [file, remoteNameArg] = process.argv.slice(2);
if (!file) {
  console.error("用法: node scripts/upload-gitcode-asset.mjs <本地文件> [远端文件名]");
  process.exit(1);
}
const remoteName = remoteNameArg ?? file.split(/[\\/]/).pop();
const token = readFileSync("C:/Users/86133/.gitcode_token/新建文本文档.txt", "utf8").trim();
const auth = { headers: { Authorization: `Bearer ${token}` } };

async function listAssets() {
  const release = await (await fetch(`${API}/tags/${TAG}`, auth)).json();
  return release.assets ?? [];
}

async function deleteAsset(id) {
  const res = await fetch(`${API}/${TAG}/attach_files/${id}`, { ...auth, method: "DELETE" });
  if (res.status !== 204) {
    throw new Error(`删除旧附件 #${id} 失败：HTTP ${res.status}（可到发行版网页手动删除后重试）`);
  }
}

const existing = (await listAssets()).find((a) => a.name === remoteName);
if (existing) {
  console.log(`同名附件已存在 (#${existing.id})，先删除旧版本…`);
  await deleteAsset(existing.id);
}

const meta = await (await fetch(`${API}/${TAG}/upload_url?file_name=${encodeURIComponent(remoteName)}`, auth)).json();
if (!meta.url) throw new Error("未取得上传地址: " + JSON.stringify(meta).slice(0, 200));

const body = readFileSync(file);
const headers = { ...meta.headers, "Content-Type": "application/octet-stream" };
const res = await fetch(meta.url, { method: "PUT", headers, body });
// GitCode 上传经 OBS 回调注册；同名冲突等失败会以 203 + 回调错误文本返回，不能当作成功
const respText = await res.text().catch(() => "");
const failed = res.status !== 200 || /"code"/i.test(respText.slice(0, 120));
console.log(`上传 ${remoteName} (${body.length} 字节) → HTTP ${res.status}${failed ? "（失败）" : ""}`);
if (failed) {
  console.error(respText.slice(0, 300) || `预期 HTTP 200，实际 ${res.status}`);
  process.exit(1);
}

// 确认附件已注册到发行版，且字节数一致
const assets = await listAssets();
const uploaded = assets.find((a) => a.name === remoteName);
if (!uploaded) {
  console.error("上传后未在附件列表中找到 " + remoteName + "，当前附件:", assets.map((a) => a.name).join(", "));
  process.exit(1);
}
console.log("已确认注册。当前附件:", assets.map((a) => a.name).join(", "));

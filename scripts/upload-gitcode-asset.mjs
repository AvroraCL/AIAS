// 通过 GitCode upload_url 预签名接口上传 release 附件
// 用法：node scripts/upload-gitcode-asset.mjs <本地文件> [远端文件名]
import { readFileSync } from "node:fs";

const [file, remoteNameArg] = process.argv.slice(2);
if (!file) {
  console.error("用法: node scripts/upload-gitcode-asset.mjs <本地文件> [远端文件名]");
  process.exit(1);
}
const remoteName = remoteNameArg ?? file.split(/[\\/]/).pop();
const token = readFileSync("C:/Users/86133/.gitcode_token/新建文本文档.txt", "utf8").trim();

const meta = await (
  await fetch(
    `https://api.gitcode.com/api/v5/repos/HelenaSG/Aias/releases/stable/upload_url?file_name=${encodeURIComponent(remoteName)}`,
    { headers: { Authorization: `Bearer ${token}` } },
  )
).json();
if (!meta.url) throw new Error("未取得上传地址: " + JSON.stringify(meta).slice(0, 200));

const body = readFileSync(file);
const headers = { ...meta.headers, "Content-Type": "application/octet-stream" };
const res = await fetch(meta.url, { method: "PUT", headers, body });
console.log(`上传 ${remoteName} (${body.length} 字节) → HTTP ${res.status}`);
if (!res.ok) console.error(await res.text().then((t) => t.slice(0, 300)));
else {
  // 确认附件已注册到发行版
  const list = await (
    await fetch("https://api.gitcode.com/api/v5/repos/HelenaSG/Aias/releases/tags/stable")
  ).json();
  const names = (list.assets ?? []).map((a) => a.name).join(", ");
  console.log("当前附件:", names);
}

// 生成更新器清单：dist/latest.json（GitHub 主源）+ dist/latest-gitcode.json（GitCode 备用源）
// 用法：node scripts/make-latest.mjs ["更新说明"]；不传说明时沿用上次 dist/latest.json 的内容
// 前提：已用 tauri signer sign 生成 dist/AIAS_<版本>_x64-setup.exe.sig
import { existsSync, readFileSync, writeFileSync } from "node:fs";

const version = JSON.parse(readFileSync("package.json", "utf8")).version;
const exe = `AIAS_${version}_x64-setup.exe`;
const sigPath = `dist/${exe}.sig`;
if (!existsSync(sigPath)) {
  throw new Error(`缺少 ${sigPath}，请先构建安装包并签名`);
}
const signature = readFileSync(sigPath, "utf8").trim();

const previous = existsSync("dist/latest.json")
  ? JSON.parse(readFileSync("dist/latest.json", "utf8"))
  : null;
const notes = process.argv[2] || previous?.notes || "";
const pubDate =
  previous?.version === version && previous.pub_date
    ? previous.pub_date
    : new Date().toISOString().replace(/\.\d+Z$/, "Z");

function manifest(url) {
  return JSON.stringify(
    { version, notes, pub_date: pubDate, platforms: { "windows-x86_64": { signature, url } } },
    null,
    2,
  ) + "\n";
}

writeFileSync(
  "dist/latest.json",
  manifest(`https://github.com/AvroraCL/AIAS/releases/download/v${version}/${exe}`),
);
writeFileSync(
  "dist/latest-gitcode.json",
  manifest(`https://gitcode.com/HelenaSG/Aias/releases/download/stable/${exe}`),
);
console.log(`已生成 dist/latest.json 与 dist/latest-gitcode.json（${version}，pub_date ${pubDate}）`);

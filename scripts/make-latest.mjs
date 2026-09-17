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

// 签名文件内嵌 minisign 注释行（trusted comment: ... file:<文件名>）。校验它与
// 当前版本一致：stale 的 .sig（上次忘重签）会静默生成验签必败的清单——
// 5.5.17 曾把 5.5.14 的签名发进 latest.json。
const sigInfo = (() => {
  try {
    return Buffer.from(signature, "base64").toString("utf8");
  } catch {
    return "";
  }
})();
const sigFileLine = sigInfo.split(/\r?\n/).find((line) => line.startsWith("file:"));
if (sigFileLine !== `file:${exe}`) {
  throw new Error(
    `${sigPath} 的签名目标不是当前安装包（${sigFileLine || "无法解析"}，期望 file:${exe}）。` +
      `请先用 tauri signer sign 重新签名 dist/${exe}`,
  );
}

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

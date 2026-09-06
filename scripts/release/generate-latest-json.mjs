// 由 Release 元数据生成 latest.json 更新元数据。
//
// 用法：node scripts/release/generate-latest-json.mjs <release.json> <输出路径>
// 环境变量：REPO（owner/repo）、TAG（回退用的 release tag）
//
// 只收录本版本的 6 项安装资产。历史上曾因版本号推导错误产生
// CodexPlusPlus-main-macos-*.dmg 这类文件（见 v1.2.57），若无条件收录 Release
// 中的全部资产，latest.json 会把错误文件当作可用更新推给用户。
// 缺少任一本版本资产时拒绝生成，不输出残缺的更新元数据。
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function expectedAssets(version) {
  return [
    `CodexPlusPlus-${version}-windows-x64-setup.exe`,
    `CodexPlusPlus-${version}-windows-x64.zip`,
    `CodexPlusPlus-${version}-macos-x64.dmg`,
    `CodexPlusPlus-${version}-macos-x64.zip`,
    `CodexPlusPlus-${version}-macos-arm64.dmg`,
    `CodexPlusPlus-${version}-macos-arm64.zip`
  ];
}

export function buildPayload(release, repo, fallbackTag) {
  const tag = release.tagName || fallbackTag;
  if (!tag) {
    throw new Error("缺少 release tag，无法生成 latest.json。");
  }
  const version = tag.replace(/^v/i, "");
  const baseUrl = `https://github.com/${repo}/releases/download/${tag}`;
  const expected = expectedAssets(version);
  const present = new Set((release.assets || []).map((asset) => asset.name));
  const missing = expected.filter((name) => !present.has(name));
  if (missing.length > 0) {
    throw new Error(`缺少本版本安装资产，拒绝生成 latest.json：${missing.join(", ")}`);
  }
  return {
    version: `v${version}`,
    url: release.url || `https://github.com/${repo}/releases/tag/${tag}`,
    body: release.body || "",
    assets: expected.map((name) => ({
      name,
      url: `${baseUrl}/${encodeURIComponent(name)}`
    }))
  };
}

// 仅在作为入口脚本执行时运行，被测试 import 时不执行。
// import.meta.url 是解析过符号链接的 realpath，而 process.argv[1] 不是；
// 若路径中任一段是符号链接，直接比较两者会永远不相等，导致本脚本以退出码 0
// 静默不生成 latest.json。因此两边都归一到 realpath 再比较。
function isMainModule() {
  if (!process.argv[1]) {
    return false;
  }
  try {
    const entry = fs.realpathSync(path.resolve(process.argv[1]));
    const self = fs.realpathSync(fileURLToPath(import.meta.url));
    return entry === self;
  } catch {
    return false;
  }
}

if (isMainModule()) {
  const [releasePath, outputPath] = process.argv.slice(2);
  if (!releasePath || !outputPath) {
    console.error("用法：node generate-latest-json.mjs <release.json> <输出路径>");
    process.exit(1);
  }
  try {
    const release = JSON.parse(fs.readFileSync(releasePath, "utf8"));
    const payload = buildPayload(release, process.env.REPO, process.env.TAG);
    fs.writeFileSync(outputPath, `${JSON.stringify(payload, null, 2)}\n`);
    console.log(JSON.stringify(payload, null, 2));
  } catch (error) {
    console.error(`::error::${error.message}`);
    process.exit(1);
  }
}

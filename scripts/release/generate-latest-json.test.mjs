// generate-latest-json.mjs 的行为测试。
// 用法：node scripts/release/generate-latest-json.test.mjs
import fs from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { buildPayload } from "./generate-latest-json.mjs";

const fsRequire = createRequire(import.meta.url);

let pass = 0;
let fail = 0;

function check(name, fn) {
  try {
    fn();
    pass += 1;
    console.log(`PASS: ${name}`);
  } catch (error) {
    fail += 1;
    console.log(`FAIL: ${name}`);
    console.log(`    ${error.message}`);
  }
}

function assert(condition, message) {
  if (!condition) {
    throw new Error(message);
  }
}

const REPO = "cfgxy/CodexPlusPlus";

const correctAssets = [
  "CodexPlusPlus-1.2.57-windows-x64-setup.exe",
  "CodexPlusPlus-1.2.57-windows-x64.zip",
  "CodexPlusPlus-1.2.57-macos-x64.dmg",
  "CodexPlusPlus-1.2.57-macos-x64.zip",
  "CodexPlusPlus-1.2.57-macos-arm64.dmg",
  "CodexPlusPlus-1.2.57-macos-arm64.zip"
].map((name) => ({ name }));

check("正常情况：收录 6 项本版本安装资产", () => {
  const payload = buildPayload(
    { tagName: "v1.2.57", assets: correctAssets },
    REPO,
    null
  );
  assert(payload.version === "v1.2.57", `版本号错误：${payload.version}`);
  assert(payload.assets.length === 6, `资产数量应为 6，实际 ${payload.assets.length}`);
});

check("latest.json 自身不得被收录", () => {
  const payload = buildPayload(
    { tagName: "v1.2.57", assets: [...correctAssets, { name: "latest.json" }] },
    REPO,
    null
  );
  assert(
    !payload.assets.some((a) => a.name === "latest.json"),
    "latest.json 不应出现在 assets 中"
  );
});

// 核心回归点：复刻 v1.2.57 的实际故障态。
check("错误命名的 main 资产必须被排除", () => {
  const payload = buildPayload(
    {
      tagName: "v1.2.57",
      assets: [
        ...correctAssets,
        { name: "CodexPlusPlus-main-macos-arm64.dmg" },
        { name: "CodexPlusPlus-main-macos-arm64.zip" }
      ]
    },
    REPO,
    null
  );
  assert(payload.assets.length === 6, `资产数量应为 6，实际 ${payload.assets.length}`);
  assert(
    !payload.assets.some((a) => a.name.includes("main")),
    `main 错误资产泄漏进 latest.json：${JSON.stringify(payload.assets.map((a) => a.name))}`
  );
});

check("陈旧版本资产必须被排除", () => {
  const payload = buildPayload(
    {
      tagName: "v1.2.57",
      assets: [...correctAssets, { name: "CodexPlusPlus-1.2.56-windows-x64.zip" }]
    },
    REPO,
    null
  );
  assert(
    !payload.assets.some((a) => a.name.includes("1.2.56")),
    "陈旧版本资产泄漏进 latest.json"
  );
});

check("缺少本版本资产时拒绝生成", () => {
  let threw = false;
  try {
    buildPayload(
      { tagName: "v1.2.57", assets: correctAssets.slice(0, 5) },
      REPO,
      null
    );
  } catch (error) {
    threw = true;
    assert(
      error.message.includes("macos-arm64.zip"),
      `错误信息应指出缺失项，实际：${error.message}`
    );
  }
  assert(threw, "缺少资产时应抛错，实际正常返回");
});

// 只有错误资产、没有正确资产时（若清理未生效）必须失败，不得生成空 assets。
check("仅有错误资产时拒绝生成", () => {
  let threw = false;
  try {
    buildPayload(
      {
        tagName: "v1.2.57",
        assets: [
          { name: "CodexPlusPlus-main-macos-arm64.dmg" },
          { name: "CodexPlusPlus-main-macos-arm64.zip" }
        ]
      },
      REPO,
      null
    );
  } catch {
    threw = true;
  }
  assert(threw, "仅有错误资产时应抛错");
});

check("资产 URL 指向本版本 tag", () => {
  const payload = buildPayload(
    { tagName: "v1.2.57", assets: correctAssets },
    REPO,
    null
  );
  for (const asset of payload.assets) {
    assert(
      asset.url === `https://github.com/${REPO}/releases/download/v1.2.57/${encodeURIComponent(asset.name)}`,
      `URL 错误：${asset.url}`
    );
  }
});

// CLI 入口回归：入口守卫写错会让脚本以退出码 0 静默不生成文件，
// CI 里表现为「步骤成功但 latest.json 不存在」，必须实际执行一次验证。
check("CLI 入口：实际生成 latest.json 且排除错误资产", () => {
  const os = fsRequire("node:os");
  const pathMod = fsRequire("node:path");
  const cp = fsRequire("node:child_process");
  const tmp = fs.mkdtempSync(pathMod.join(os.tmpdir(), "latest-json-test-"));
  const releasePath = pathMod.join(tmp, "release.json");
  const outPath = pathMod.join(tmp, "latest.json");
  fs.writeFileSync(
    releasePath,
    JSON.stringify({
      tagName: "v1.2.57",
      url: "https://example.invalid/r",
      body: "note",
      assets: [
        ...correctAssets,
        { name: "CodexPlusPlus-main-macos-arm64.dmg" },
        { name: "latest.json" }
      ]
    })
  );
  // 经由符号链接的绝对路径调用脚本：此时 process.argv[1] 保留符号链接路径，
  // 而 import.meta.url 是解析后的 realpath，两者不相等。入口守卫若不把两边
  // 都归一到 realpath，就会静默不生成文件（退出码仍为 0）。
  // 本仓库的开发 worktree 正处在这种路径下（/home/guxy → /mnt/data）。
  const repoRoot = pathMod.resolve(fileURLToPath(import.meta.url), "../../..");
  const linkRoot = pathMod.join(tmp, "link-to-repo");
  fs.symlinkSync(repoRoot, linkRoot, "dir");
  const scriptViaLink = pathMod.join(
    linkRoot,
    "scripts/release/generate-latest-json.mjs"
  );
  const res = cp.spawnSync(
    process.execPath,
    [scriptViaLink, releasePath, outPath],
    {
      env: { ...process.env, REPO, TAG: "v1.2.57" },
      encoding: "utf8"
    }
  );
  assert(res.status === 0, `退出码应为 0，实际 ${res.status}：${res.stderr}`);
  assert(fs.existsSync(outPath), "latest.json 未被生成（入口守卫可能失效）");
  const written = JSON.parse(fs.readFileSync(outPath, "utf8"));
  assert(written.assets.length === 6, `应写入 6 项，实际 ${written.assets.length}`);
  assert(
    !written.assets.some((a) => a.name.includes("main")),
    "错误资产泄漏进生成的 latest.json"
  );
  fs.rmSync(tmp, { recursive: true, force: true });
});

console.log();
console.log(`通过 ${pass} 项，失败 ${fail} 项。`);
process.exit(fail === 0 ? 0 : 1);

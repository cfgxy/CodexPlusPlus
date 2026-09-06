// 概览页赞助商展示位的内容约束。
//
// 这块是 App.tsx 里的硬编码 JSX，没有可单独 import 的数据模块，因此按源码断言：
// 目标不是校验渲染，而是防止赞助商名称、文案和跳转地址被改回或漏改——
// 它对外展示，改错等于把用户导去错误站点。
//
// 刻意只框定概览面板一段：Provider preset 里的 JOJO Code 仍有独立业务含义
// （用户可能真的在用这家中转），不在本约束范围内。
import assert from "node:assert";
import { describe, it } from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
const appSource = readFileSync(path.join(here, "App.tsx"), "utf8");
const enSource = readFileSync(path.join(here, "i18n-en.ts"), "utf8");

// 概览赞助位：从 Panel className="sponsor-overview" 起到该 Panel 结束。
function sponsorPanel(): string {
  const start = appSource.indexOf('<Panel className="sponsor-overview">');
  assert.notStrictEqual(start, -1, "未找到概览赞助位 Panel（className 应为 sponsor-overview）");
  const end = appSource.indexOf("</Panel>", start);
  assert.notStrictEqual(end, -1, "概览赞助位 Panel 未闭合");
  return appSource.slice(start, end);
}

describe("概览页赞助商展示位", () => {
  it("显示派诺云，不再出现 JOJO Code", () => {
    const panel = sponsorPanel();
    assert.match(panel, /<h2>派诺云<\/h2>/, "赞助商名称应为派诺云");
    assert.doesNotMatch(panel, /JOJO/i, `概览赞助位仍含 JOJO 字样：\n${panel}`);
  });

  it("按钮跳转严格指向 https://www.apinoria.com/", () => {
    const panel = sponsorPanel();
    assert.match(
      panel,
      /openExternalUrl\("https:\/\/www\.apinoria\.com\/"\)/,
      "跳转地址必须严格为 https://www.apinoria.com/"
    );
    assert.doesNotMatch(panel, /jojocode\.com/i, "概览赞助位不得再跳转 jojocode.com");
  });

  it("中英文界面都能展示派诺云赞助信息", () => {
    // 英文缺键时 t() 会回退中文原文，界面不会报错，只是静默显示中文——
    // 因此必须显式断言英文词典里有对应键，否则英文界面的缺失无人发现。
    assert.match(enSource, /"打开派诺云":/, "i18n-en.ts 缺少「打开派诺云」按钮键");
    assert.match(enSource, /"派诺云提供/, "i18n-en.ts 缺少派诺云介绍文案键");
    assert.doesNotMatch(enSource, /打开 JOJO Code/, "i18n-en.ts 仍保留旧的「打开 JOJO Code」键");
  });

  it("不误伤仍有独立业务含义的 JOJO Code Provider preset", () => {
    const presets = readFileSync(path.join(here, "presets.ts"), "utf8");
    assert.match(presets, /id: "jojocode"/, "jojocode preset 不应被删除");
    assert.match(presets, /id: "jojocode-max"/, "jojocode-max preset 不应被删除");
  });
});

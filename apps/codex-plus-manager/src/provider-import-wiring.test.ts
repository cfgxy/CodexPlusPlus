import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const source = readFileSync(new URL("./App.tsx", import.meta.url), "utf8");
const desktop = readFileSync(new URL("../src-tauri/src/lib.rs", import.meta.url), "utf8");
const importCore = readFileSync(new URL("../../../crates/codex-plus-core/src/provider_import.rs", import.meta.url), "utf8");

test("冷启动和 macOS 已运行窗口都由相同协议解析入口接收", () => {
  assert.match(desktop, /pub fn handle_provider_import_url\(url: &str\)/);
  assert.match(desktop, /tauri::RunEvent::Opened \{ urls \}[\s\S]*?handle_provider_import_url\(url\.as_str\(\)\)/);
});

test("预览期两项获取独立启动且离开预览先清理待确认请求", () => {
  assert.match(source, /useEffect\(\(\) => \{\s*active\.current = true;\s*void fetchBilling\(\);\s*void fetchModels\(\);/);
  assert.match(source, /if \(pendingProviderImport && next !== "relay"\)[\s\S]*?dismiss_pending_provider_import/);
  assert.match(source, /onConfirm\(request\.importId, replaceKey, multiplierValue, modelValues, draft\.model\)/);
  assert.match(source, /showModelField=\{false\}/);
  assert.match(source, /<fieldset disabled className="provider-import-fields">/);
  assert.match(source, /const fetchSub2ApiRate = async \(\) =>/);
  assert.match(source, /fetchRelayProfileModels\(\{/);
});

test("Windows 待确认文件在写入 Key 前限制为当前用户访问", () => {
  assert.match(importCore, /let mut file = options\.open\(&temp\)\?;\s*#\[cfg\(windows\)\]\s*restrict_pending_file_to_current_user\(&temp\)\?;\s*file\.write_all\(&contents\)\?/);
  assert.match(importCore, /\.args\(\["\/inheritance:r", "\/grant:r"/);
});

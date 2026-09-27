import assert from "node:assert/strict";
import test from "node:test";
import { maskSecretValue } from "./mask-secret.ts";

test("导入页面的短 Key 和长 Key 均不完整回显", () => {
  for (const key of ["s", "sk-test", "sk-test-FAKE-397"]) {
    const masked = maskSecretValue(key, "未填写");
    assert.ok(masked.includes("…") || masked === "••••");
    assert.ok(!masked.includes(key));
  }
  assert.equal(maskSecretValue("", "未填写"), "未填写");
});

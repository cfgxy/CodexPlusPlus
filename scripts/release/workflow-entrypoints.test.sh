#!/usr/bin/env bash
# 发布入口语义的静态约束测试。
#
# 背景：清理陈旧资产与最终严格集合验证只实现在 auto-release.yml。
# 若 release-assets.yml 仍能被独立触发（release: published / workflow_dispatch），
# 就存在绕过清理与验证的旁路入口：Release 可在残留错误资产的情况下报告成功。
# 本测试把「唯一正式发布入口」这一约束固化下来。
#
# 用法：bash scripts/release/workflow-entrypoints.test.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
AUTO="$ROOT/.github/workflows/auto-release.yml"
ASSETS="$ROOT/.github/workflows/release-assets.yml"

PASS=0
FAIL=0

ok() { PASS=$((PASS + 1)); echo "PASS: $1"; }
ng() { FAIL=$((FAIL + 1)); echo "FAIL: $1"; [ -n "${2:-}" ] && echo "    $2"; }

# 读取 workflow 的顶层 on: 触发方式列表。
triggers() {
  python3 - "$1" <<'PY'
import sys, yaml
d = yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
# PyYAML 会把裸 on 解析为布尔 True
on = d.get("on", d.get(True))
print("\n".join(on if isinstance(on, dict) else [on]))
PY
}

# 断言 1：release-assets.yml 只能被 auto-release.yml 嵌套调用，
# 不得保留任何可独立触发的入口。
t="$(triggers "$ASSETS")"
if [ "$(echo "$t" | sort | tr '\n' ',')" = "workflow_call," ]; then
  ok "release-assets.yml 仅保留 workflow_call 入口"
else
  ng "release-assets.yml 仅保留 workflow_call 入口" "实际触发方式：$(echo "$t" | tr '\n' ' ')"
fi

# 断言 2：auto-release.yml 必须同时保留 push 与 workflow_dispatch——
# 删掉旁路后，手动恢复历史 tag 必须仍有受控入口。
t="$(triggers "$AUTO")"
for want in push workflow_dispatch; do
  # 用 <<< 而非管道：grep -q 命中即退出会让上游 echo 收到 SIGPIPE，
  # 在 pipefail 下把整条管道判为失败，导致断言随机翻转。
  if grep -qx "$want" <<<"$t"; then
    ok "auto-release.yml 保留 $want 入口"
  else
    ng "auto-release.yml 保留 $want 入口" "实际触发方式：$(echo "$t" | tr '\n' ' ')"
  fi
done

# 断言 3：唯一入口必须真正串起清理与严格验证两个 job。
for job in cleanup-assets verify; do
  if python3 -c "
import sys, yaml
d = yaml.safe_load(open('$AUTO', encoding='utf-8'))
sys.exit(0 if '$job' in d['jobs'] else 1)
"; then
    ok "auto-release.yml 包含 $job job"
  else
    ng "auto-release.yml 包含 $job job"
  fi
done

# 断言 4：latest-json 必须显式 checkout 一个确定含生成脚本的 ref。
# 为历史 tag 补件时，目标 tag 上可能没有该脚本（v1.2.57 即如此），
# 默认 checkout（未指定 ref）在 release: published 下会取目标 tag 而失败。
if python3 - <<'PY'
import sys, yaml
d = yaml.safe_load(open(".github/workflows/release-assets.yml", encoding="utf-8"))
steps = d["jobs"]["latest-json"]["steps"]
checkout = next(
    (s for s in steps if isinstance(s.get("uses"), str) and s["uses"].startswith("actions/checkout")),
    None,
)
if checkout is None:
    print("latest-json 缺少 checkout 步骤")
    sys.exit(1)
ref = (checkout.get("with") or {}).get("ref")
if not ref:
    print("latest-json 的 checkout 未显式指定 ref")
    sys.exit(1)
# 必须指向工作流自身的 SHA，而不是被发布的 tag
if "github.sha" not in str(ref) and "workflow_sha" not in str(ref):
    print(f"latest-json 的 checkout ref 未指向工作流自身版本：{ref}")
    sys.exit(1)
PY
then
  ok "latest-json 显式 checkout 含生成脚本的受控 ref"
else
  ng "latest-json 显式 checkout 含生成脚本的受控 ref"
fi

# 断言 5：安装包必须仍从目标 tag 构建——
# 修 latest-json 的 ref 时不能顺手把构建 job 的 ref 也改掉。
if python3 - <<'PY'
import sys, yaml
d = yaml.safe_load(open(".github/workflows/release-assets.yml", encoding="utf-8"))
for job in ("windows-installer", "macos-dmg"):
    steps = d["jobs"][job]["steps"]
    checkout = next(
        (s for s in steps if isinstance(s.get("uses"), str) and s["uses"].startswith("actions/checkout")),
        None,
    )
    ref = str((checkout.get("with") or {}).get("ref", ""))
    if "tag_name" not in ref:
        print(f"{job} 的 checkout ref 未指向目标 tag：{ref}")
        sys.exit(1)
PY
then
  ok "安装包构建 job 仍从目标 tag checkout"
else
  ng "安装包构建 job 仍从目标 tag checkout"
fi

# 断言 6：注释不得与实现不符——曾出现注释声称「取工作流自身 ref」
# 但实际未写 ref 的情况。若注释提到 ref 语义，附近必须真有 ref 配置。
if grep -q 'ref:' <<<"$(sed -n '/latest-json:/,/Upload latest.json/p' "$ASSETS")"; then
  ok "latest-json 注释与实现一致（确有 ref 配置）"
else
  ng "latest-json 注释与实现一致（确有 ref 配置）"
fi

echo
echo "通过 $PASS 项，失败 $FAIL 项。"
[ "$FAIL" = 0 ]

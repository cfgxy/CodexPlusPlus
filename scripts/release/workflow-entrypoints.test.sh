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

# 断言 7：一次 main push 只能触发一条打包链。
# pr-build.yml 与 auto-release.yml 都会构建 Windows / macOS x64 / macOS arm64；
# 若两者同时监听 main push，同一次推送会把三份包各构建两遍，白烧一倍额度，
# 且两条链产出的产物无人比对，语义上也说不清哪份才是准的。
# 发布链（auto-release.yml）是 main push 后唯一自动打包入口。
if python3 - <<'PY'
import sys, yaml, glob

def triggers(path):
    d = yaml.safe_load(open(path, encoding="utf-8"))
    on = d.get("on", d.get(True))
    return (on if isinstance(on, dict) else {k: None for k in on}), d

# 会产出安装包的 job 名（各 workflow 内）
PACKAGING = {"windows-artifacts", "macos-dmg", "windows-installer", "release-assets"}

offenders = []
for path in sorted(glob.glob(".github/workflows/*.yml")):
    on, d = triggers(path)
    push = on.get("push")
    if not isinstance(push, dict):
        continue
    if "main" not in (push.get("branches") or []):
        continue
    packaging_jobs = sorted(set(d.get("jobs", {})) & PACKAGING)
    if packaging_jobs:
        offenders.append((path, packaging_jobs))

if len(offenders) != 1:
    print("监听 main push 且会打包的 workflow 必须恰好 1 个，实际：")
    for path, jobs in offenders:
        print(f"  {path} -> {jobs}")
    sys.exit(1)
if offenders[0][0] != ".github/workflows/auto-release.yml":
    print(f"main push 的唯一打包入口应为 auto-release.yml，实际：{offenders[0][0]}")
    sys.exit(1)
PY
then
  ok "main push 只触发一条打包链（auto-release.yml）"
else
  ng "main push 只触发一条打包链（auto-release.yml）"
fi

# 断言 8：去掉 main push 不得牵连 PR 校验与手动入口——
# PR 上的 Windows/macOS 构建验证是拦截问题的主要手段，必须保留。
if python3 - <<'PY'
import sys, yaml
d = yaml.safe_load(open(".github/workflows/pr-build.yml", encoding="utf-8"))
on = d.get("on", d.get(True))
on = on if isinstance(on, dict) else {k: None for k in on}
missing = [t for t in ("pull_request", "workflow_dispatch") if t not in on]
if missing:
    print(f"pr-build.yml 缺少必须保留的触发方式：{missing}")
    sys.exit(1)
for job in ("windows-artifacts", "macos-dmg", "release-scripts"):
    if job not in d.get("jobs", {}):
        print(f"pr-build.yml 缺少 job：{job}")
        sys.exit(1)
PY
then
  ok "pr-build.yml 保留 PR 校验与手动触发及三个 job"
else
  ng "pr-build.yml 保留 PR 校验与手动触发及三个 job"
fi

# 断言 9：main push 不再跑 pr-build.yml，发布链必须自己跑脚本测试。
# 判定脚本决定 Release 建不建、资产删不删，未经测试就发布等于把把关环节
# 从正式发布路径上摘掉；plan 必须依赖它，否则测试与发布并行、失败也拦不住。
if python3 - <<'PY'
import sys, yaml
d = yaml.safe_load(open(".github/workflows/auto-release.yml", encoding="utf-8"))
jobs = d.get("jobs", {})
job = jobs.get("release-scripts")
if not job:
    print("auto-release.yml 缺少 release-scripts job")
    sys.exit(1)
if "release-scripts.yml" not in str(job.get("uses", "")):
    print(f"release-scripts job 应复用 release-scripts.yml，实际：{job.get('uses')}")
    sys.exit(1)
needs = jobs.get("plan", {}).get("needs")
needs = [needs] if isinstance(needs, str) else (needs or [])
if "release-scripts" not in needs:
    print(f"plan 未依赖 release-scripts，测试失败拦不住发布：needs={needs}")
    sys.exit(1)
PY
then
  ok "auto-release.yml 在发布前自跑脚本测试且 plan 依赖它"
else
  ng "auto-release.yml 在发布前自跑脚本测试且 plan 依赖它"
fi

echo
echo "通过 $PASS 项，失败 $FAIL 项。"
[ "$FAIL" = 0 ]

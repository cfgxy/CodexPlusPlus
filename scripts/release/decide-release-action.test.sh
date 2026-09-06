#!/usr/bin/env bash
# decide-release-action.sh 的行为测试。
# 用法：bash scripts/release/decide-release-action.test.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="$SCRIPT_DIR/decide-release-action.sh"

PASS=0
FAIL=0

# 完整的 7 项资产清单（版本 1.2.57）。
full_assets() {
  cat <<'EOF'
CodexPlusPlus-1.2.57-windows-x64-setup.exe
CodexPlusPlus-1.2.57-windows-x64.zip
CodexPlusPlus-1.2.57-macos-x64.dmg
CodexPlusPlus-1.2.57-macos-x64.zip
CodexPlusPlus-1.2.57-macos-arm64.dmg
CodexPlusPlus-1.2.57-macos-arm64.zip
latest.json
EOF
}

# run_case <用例名> <期望退出码> <期望 stdout 子串（逗号分隔的多个断言）>
# 其余参数通过预设的环境变量传入。
check() {
  local name="$1" expected_status="$2" expected_out="$3" actual_out actual_status
  actual_out="$(TAG="${TAG:-}" RELEASE_EXISTS="${RELEASE_EXISTS:-}" \
    REMOTE_TAG_EXISTS="${REMOTE_TAG_EXISTS:-}" ASSETS="${ASSETS:-}" \
    bash "$TARGET" 2>&1)"
  actual_status=$?

  local ok=1
  if [ "$actual_status" != "$expected_status" ]; then
    ok=0
  fi
  local needle
  while IFS= read -r needle; do
    [ -z "$needle" ] && continue
    case "$actual_out" in
      *"$needle"*) ;;
      *) ok=0 ;;
    esac
  done <<<"$expected_out"

  if [ "$ok" = 1 ]; then
    PASS=$((PASS + 1))
    echo "PASS: $name"
  else
    FAIL=$((FAIL + 1))
    echo "FAIL: $name"
    echo "  期望退出码=$expected_status 实际=$actual_status"
    echo "  期望包含:"
    echo "$expected_out" | sed 's/^/    /'
    echo "  实际输出:"
    echo "$actual_out" | sed 's/^/    /'
  fi
}

# 用例 1：全新版本，Release 与 tag 都不存在 —— 建 Release 并构建全部资产。
TAG=v1.2.58 RELEASE_EXISTS=false REMOTE_TAG_EXISTS=false ASSETS="" \
  check "全新版本：创建 Release 并构建资产" 0 'create_release=true
build_assets=true'

# 用例 2：tag 已存在但没有 Release —— 状态异常，必须显式失败。
TAG=v1.2.58 RELEASE_EXISTS=false REMOTE_TAG_EXISTS=true ASSETS="" \
  check "悬挂 tag：显式失败" 1 'tag v1.2.58 已存在但没有对应的 Release'

# 用例 3：Release 已存在但资产为空（v1.2.57 的实际故障态）—— 不重建 Release，补齐资产。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="" \
  check "空 Release：补齐资产" 0 'create_release=false
build_assets=true'

# 用例 4：Release 存在且资产缺件 —— 补齐。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true \
  ASSETS="$(full_assets | grep -v 'macos-arm64.dmg')" \
  check "缺件 Release：补齐资产" 0 'create_release=false
build_assets=true
CodexPlusPlus-1.2.57-macos-arm64.dmg'

# 用例 5：Release 存在且资产完整 —— 幂等跳过，不重复发布。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets)" \
  check "完整 Release：幂等跳过" 0 'create_release=false
build_assets=false'

# 用例 6：缺少 TAG 输入 —— 显式失败，不得静默按默认值继续。
TAG="" RELEASE_EXISTS=false REMOTE_TAG_EXISTS=false ASSETS="" \
  check "缺少 TAG：显式失败" 1 '缺少 TAG'

# 用例 8：复刻 v1.2.57 当前线上状态——正确的 Windows 资产 + 2 个 `main` 错误资产。
# 期望资产尚缺，且必须识别出错误资产待清理。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="CodexPlusPlus-1.2.57-windows-x64-setup.exe
CodexPlusPlus-1.2.57-windows-x64.zip
CodexPlusPlus-main-macos-arm64.dmg
CodexPlusPlus-main-macos-arm64.zip" \
  check "线上现状：缺件且混有错误资产" 0 'create_release=false
build_assets=true
cleanup_assets=true
CodexPlusPlus-main-macos-arm64.dmg
CodexPlusPlus-main-macos-arm64.zip'

# 用例 9：期望的 7 项全部齐全，但仍混有 `main` 错误资产。
# 关键回归点：不得因「期望资产齐全」就判定完整并幂等跳过，否则错误资产永久残留。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets
echo 'CodexPlusPlus-main-macos-arm64.dmg'
echo 'CodexPlusPlus-main-macos-arm64.zip')" \
  check "齐全但混有错误资产：必须清理且不得误判完整" 0 'create_release=false
cleanup_assets=true
CodexPlusPlus-main-macos-arm64.dmg'

# 用例 10：非目标版本的陈旧安装包（上一版本残留）同样属于错误资产。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets
echo 'CodexPlusPlus-1.2.56-windows-x64.zip')" \
  check "陈旧版本资产：识别为待清理" 0 'cleanup_assets=true
CodexPlusPlus-1.2.56-windows-x64.zip'

# 用例 11：完整且没有任何多余资产时，才允许幂等跳过，且不得触发清理。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets)" \
  check "严格完整：幂等跳过且不清理" 0 'build_assets=false
cleanup_assets=false'

# 用例 12：latest.json 属于期望清单，绝不能被误判为待清理资产。
TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets)" \
  check "latest.json 不得被误判为错误资产" 0 'cleanup_assets=false'
out_check="$(TAG=v1.2.57 RELEASE_EXISTS=true REMOTE_TAG_EXISTS=true ASSETS="$(full_assets)" \
  bash "$TARGET" 2>&1)"
if grep -q 'stale=latest.json' <<<"$out_check"; then
  FAIL=$((FAIL + 1))
  echo "FAIL: latest.json 被错误列入清理清单"
fi

# 用例 7：期望资产清单必须与打包脚本/workflow 实际产出的文件名模板一致，
# 否则 verify 会因为对不上名字而永远判定“资产不完整”。
ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
name="资产清单与打包产物命名一致"
missing_pattern=()
grep -q 'CodexPlusPlus-${VERSION}-windows-x64-setup.exe' "$ROOT/scripts/installer/windows/CodexPlusPlus.nsi" \
  || missing_pattern+=("windows setup.exe 命名与 NSIS OutFile 不一致")
grep -q 'CodexPlusPlus-\$version-windows-x64.zip' "$ROOT/.github/workflows/release-assets.yml" \
  || missing_pattern+=("windows zip 命名与 release-assets.yml 不一致")
grep -q 'CodexPlusPlus-${VERSION}-macos-${ARCH}.dmg' "$ROOT/scripts/installer/macos/package-dmg.sh" \
  || missing_pattern+=("macos dmg 命名与 package-dmg.sh 不一致")
grep -q 'CodexPlusPlus-${VERSION}-macos-${{ matrix.arch }}.zip' "$ROOT/.github/workflows/release-assets.yml" \
  || missing_pattern+=("macos zip 命名与 release-assets.yml 不一致")
if [ "${#missing_pattern[@]}" -eq 0 ]; then
  PASS=$((PASS + 1))
  echo "PASS: $name"
else
  FAIL=$((FAIL + 1))
  echo "FAIL: $name"
  printf '    %s\n' "${missing_pattern[@]}"
fi

echo
echo "通过 $PASS 项，失败 $FAIL 项。"
[ "$FAIL" = 0 ]

#!/usr/bin/env bash
# 判定一次 main push 应当执行的发布动作，供 auto-release.yml 调用。
#
# 输入（环境变量）：
#   TAG                目标 release tag，如 v1.2.57
#   RELEASE_EXISTS     该 tag 的 Release 是否已存在：true / false
#   REMOTE_TAG_EXISTS  远端 tag 是否已存在：true / false
#   ASSETS             该 Release 现有资产名，每行一个；无资产则为空
#
# 输出（写入 $GITHUB_OUTPUT，同时打印到 stdout 便于本地测试与日志核对）：
#   create_release=true|false   是否需要创建 Release
#   build_assets=true|false     是否需要构建并上传安装资产
#
# 语义要点：
#   - Release 已存在但资产缺失时必须补齐，不得因“Release 已存在”直接跳过。
#   - 资产完整时不重复构建，保证版本号未变化的重复 push 幂等。
#   - tag 存在但 Release 缺失属异常状态，显式失败而非静默跳过。
set -euo pipefail

TAG="${TAG:-}"
if [ -z "$TAG" ]; then
  echo "::error::缺少 TAG，无法判定发布动作。" >&2
  exit 1
fi

VERSION="${TAG#v}"
VERSION="${VERSION#V}"

# 期望的完整资产清单，与 release-assets.yml 各 job 的产物一一对应。
expected_assets=(
  "CodexPlusPlus-${VERSION}-windows-x64-setup.exe"
  "CodexPlusPlus-${VERSION}-windows-x64.zip"
  "CodexPlusPlus-${VERSION}-macos-x64.dmg"
  "CodexPlusPlus-${VERSION}-macos-x64.zip"
  "CodexPlusPlus-${VERSION}-macos-arm64.dmg"
  "CodexPlusPlus-${VERSION}-macos-arm64.zip"
  "latest.json"
)

emit() {
  echo "$1"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    echo "$1" >> "$GITHUB_OUTPUT"
  fi
}

if [ "${RELEASE_EXISTS:-false}" != "true" ]; then
  if [ "${REMOTE_TAG_EXISTS:-false}" = "true" ]; then
    echo "::error::tag $TAG 已存在但没有对应的 Release，请检查仓库状态，或升级 Cargo.toml 的 workspace 版本号后再发布。" >&2
    exit 1
  fi
  echo "Release $TAG 不存在，将创建 Release 并构建全部安装资产。"
  emit "create_release=true"
  emit "build_assets=true"
  exit 0
fi

# Release 已存在：按资产完整性决定是补齐还是幂等跳过。
missing=()
for asset in "${expected_assets[@]}"; do
  if ! printf '%s\n' "${ASSETS:-}" | grep -Fxq "$asset"; then
    missing+=("$asset")
  fi
done

emit "create_release=false"

if [ "${#missing[@]}" -eq 0 ]; then
  echo "Release $TAG 已存在且 ${#expected_assets[@]} 项资产齐全，跳过本次发布。"
  emit "build_assets=false"
  exit 0
fi

echo "Release $TAG 已存在但资产缺失 ${#missing[@]} 项，将补齐："
printf '  %s\n' "${missing[@]}"
emit "build_assets=true"

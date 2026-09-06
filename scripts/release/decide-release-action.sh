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
#   cleanup_assets=true|false   是否存在需要先行删除的错误/陈旧资产
#   stale_assets=<逗号分隔>     待删除的资产名清单
#
# 语义要点：
#   - Release 已存在但资产缺失时必须补齐，不得因“Release 已存在”直接跳过。
#   - 期望清单之外的任何资产都属错误或陈旧产物（如版本号推导错误产生的
#     CodexPlusPlus-main-macos-*），必须删除：它们会被 latest.json 收录，
#     导致更新元数据指向错误文件。只检查“缺件”不足以保证 Release 正确。
#   - 只有资产集合与期望清单严格相等时才算完整并幂等跳过。
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
  emit "cleanup_assets=false"
  emit "stale_assets="
  exit 0
fi

# Release 已存在：同时检查“缺件”与“多余”，两者都会让 Release 不合格。
missing=()
for asset in "${expected_assets[@]}"; do
  if ! printf '%s\n' "${ASSETS:-}" | grep -Fxq "$asset"; then
    missing+=("$asset")
  fi
done

# 期望清单之外的资产一律视为错误或陈旧产物，需在重建前删除。
stale=()
while IFS= read -r existing; do
  [ -z "$existing" ] && continue
  keep=false
  for asset in "${expected_assets[@]}"; do
    if [ "$existing" = "$asset" ]; then
      keep=true
      break
    fi
  done
  if [ "$keep" = false ]; then
    stale+=("$existing")
  fi
done <<<"${ASSETS:-}"

emit "create_release=false"

if [ "${#stale[@]}" -gt 0 ]; then
  echo "Release $TAG 存在 ${#stale[@]} 项错误或陈旧资产，将删除："
  printf '  stale=%s\n' "${stale[@]}"
  emit "cleanup_assets=true"
  emit "stale_assets=$(IFS=,; echo "${stale[*]}")"
else
  emit "cleanup_assets=false"
  emit "stale_assets="
fi

if [ "${#missing[@]}" -eq 0 ] && [ "${#stale[@]}" -eq 0 ]; then
  echo "Release $TAG 已存在且 ${#expected_assets[@]} 项资产严格齐全，跳过本次发布。"
  emit "build_assets=false"
  exit 0
fi

if [ "${#missing[@]}" -gt 0 ]; then
  echo "Release $TAG 已存在但资产缺失 ${#missing[@]} 项，将补齐："
  printf '  %s\n' "${missing[@]}"
fi
# 存在错误资产时即使期望清单齐全也要重建：被删除的错误资产可能与正确资产
# 同源（如 arm64 的 dmg/zip），删除后必须重新构建才能保证集合完整。
emit "build_assets=true"

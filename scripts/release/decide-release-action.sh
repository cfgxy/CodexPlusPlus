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
#   stale_assets=<每行一个>     待删除的资产名清单（多行 output）
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

# 多行值必须用 GitHub Actions 的 heredoc 语法写 output，否则会被截断。
# 资产名清单不能用逗号拼接：文件名本身允许含逗号，拼接后无法无损还原，
# 会把一个真实文件名拆成两个不存在的名字，删除失败或漏删。
emit_multiline() {
  local key="$1"
  shift
  printf '%s\n' "$key:"
  [ "$#" -gt 0 ] && printf '  %s\n' "$@"
  if [ -n "${GITHUB_OUTPUT:-}" ]; then
    local delim="ghadelim_$$_${RANDOM}"
    {
      echo "$key<<$delim"
      [ "$#" -gt 0 ] && printf '%s\n' "$@"
      echo "$delim"
    } >> "$GITHUB_OUTPUT"
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
  emit_multiline "stale_assets"
  exit 0
fi

# Release 已存在：同时检查“缺件”与“多余”，两者都会让 Release 不合格。
#
# 现有资产先读进数组，全程用字符串比较判定，不经过管道。
# 曾用 `printf ... | grep -Fxq`：grep 命中即退出，printf 收到 SIGPIPE 返回 141，
# 在 `set -o pipefail` 下把整条管道判为失败，于是完整 Release 有约一半概率
# 被误判为缺件并重建——同一输入随机翻转，属竞态而非逻辑错误。
present=()
while IFS= read -r existing; do
  [ -z "$existing" ] && continue
  present+=("$existing")
done <<<"${ASSETS:-}"

contains() {
  local needle="$1"
  shift
  local item
  for item in "$@"; do
    [ "$item" = "$needle" ] && return 0
  done
  return 1
}

missing=()
for asset in "${expected_assets[@]}"; do
  if ! contains "$asset" ${present[@]+"${present[@]}"}; then
    missing+=("$asset")
  fi
done

# 期望清单之外的资产一律视为错误或陈旧产物，需在重建前删除。
stale=()
for existing in ${present[@]+"${present[@]}"}; do
  if ! contains "$existing" "${expected_assets[@]}"; then
    stale+=("$existing")
  fi
done

emit "create_release=false"

if [ "${#stale[@]}" -gt 0 ]; then
  echo "Release $TAG 存在 ${#stale[@]} 项错误或陈旧资产，将删除："
  printf '  stale=%s\n' "${stale[@]}"
  emit "cleanup_assets=true"
  emit_multiline "stale_assets" "${stale[@]}"
else
  emit "cleanup_assets=false"
  emit_multiline "stale_assets"
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

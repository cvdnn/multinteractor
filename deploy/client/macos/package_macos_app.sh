#!/usr/bin/env bash
set -euo pipefail

# 将 mi-ui 编译并打包为标准的 macOS .app 桌面应用程序（支持 vN 版本号管理）
# 用法:
#   ./deploy/client/macos/package_macos_app.sh                  # 使用 VERSION 中的 vN 构建 release 版本
#   ./deploy/client/macos/package_macos_app.sh --bump           # 版本号 vN -> v(N+1) 后构建 release 版本
#   ./deploy/client/macos/package_macos_app.sh --version v3     # 指定版本号 v3 后构建 release 版本
#   ./deploy/client/macos/package_macos_app.sh --debug          # 构建 debug 版本

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
CLIENT_DIR="${REPO_ROOT}/client"
VERSION_FILE="${REPO_ROOT}/VERSION"

PROFILE="release"
CARGO_FLAGS="--release"
BUMP_VERSION=0
EXPLICIT_VERSION=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)
            PROFILE="debug"
            CARGO_FLAGS=""
            shift
            ;;
        --bump)
            BUMP_VERSION=1
            shift
            ;;
        --version)
            EXPLICIT_VERSION="${2:-}"
            if [[ -z "${EXPLICIT_VERSION}" ]]; then
                echo "❌ --version 需要传入形如 v1, v2, v3 的版本号" >&2
                exit 1
            fi
            shift 2
            ;;
        *)
            echo "❌ 未知参数: $1" >&2
            exit 1
            ;;
    esac
done

if [[ ! -f "${VERSION_FILE}" ]]; then
    echo "v1" > "${VERSION_FILE}"
fi

APP_VERSION="$(tr -d '[:space:]' < "${VERSION_FILE}")"
if [[ -n "${EXPLICIT_VERSION}" ]]; then
    APP_VERSION="${EXPLICIT_VERSION}"
fi

if [[ ! "${APP_VERSION}" =~ ^v([1-9][0-9]*)$ ]]; then
    echo "❌ 无效版本号 '${APP_VERSION}'，必须符合 vN 格式（N 为大于等于 1 的自然数，如 v1, v2, v3）" >&2
    exit 1
fi

VERSION_NUM="${BASH_REMATCH[1]}"
if [[ "${BUMP_VERSION}" -eq 1 ]]; then
    VERSION_NUM=$((VERSION_NUM + 1))
    APP_VERSION="v${VERSION_NUM}"
fi

echo "${APP_VERSION}" > "${VERSION_FILE}"

echo "==> [1/4] 编译 mi-ui (${PROFILE}, 版本: ${APP_VERSION})..."
cd "${CLIENT_DIR}"
MI_APP_VERSION="${APP_VERSION}" cargo build ${CARGO_FLAGS} -p mi-ui

BIN_PATH="${CLIENT_DIR}/target/${PROFILE}/mi-ui"
APP_NAME="Multinteractor.app"
VERSIONED_APP_NAME="Multinteractor-${APP_VERSION}.app"
APP_DIR="${CLIENT_DIR}/target/${PROFILE}/${APP_NAME}"
VERSIONED_APP_DIR="${CLIENT_DIR}/target/${PROFILE}/${VERSIONED_APP_NAME}"
CONTENTS_DIR="${APP_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

echo "==> [2/4] 组装 ${APP_NAME} 目录结构 (${APP_VERSION})..."
rm -rf "${APP_DIR}" "${VERSIONED_APP_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

cp "${BIN_PATH}" "${MACOS_DIR}/mi-ui"
chmod +x "${MACOS_DIR}/mi-ui"
sed -e "s|<string>0\.1\.0</string>|<string>${APP_VERSION}</string>|" "${SCRIPT_DIR}/Info.plist" > "${CONTENTS_DIR}/Info.plist"
if command -v plutil >/dev/null 2>&1; then
    plutil -replace CFBundleShortVersionString -string "${APP_VERSION}" "${CONTENTS_DIR}/Info.plist"
    plutil -replace CFBundleVersion -string "${VERSION_NUM}" "${CONTENTS_DIR}/Info.plist"
fi

echo "==> [3/4] 生成应用图标 AppIcon.icns..."
PYTHON_BIN=""
if [[ -x "/usr/bin/python3" ]]; then
    PYTHON_BIN="/usr/bin/python3"
elif command -v python3 >/dev/null 2>&1; then
    PYTHON_BIN="$(command -v python3)"
fi

if command -v sips >/dev/null 2>&1 && command -v iconutil >/dev/null 2>&1 && [[ -n "${PYTHON_BIN}" ]]; then
    ICON_TMP="$(mktemp -d)"
    ICONSET_DIR="${ICON_TMP}/AppIcon.iconset"
    BASE_PNG="${ICON_TMP}/icon_1024.png"
    mkdir -p "${ICONSET_DIR}"

    "${PYTHON_BIN}" - "${BASE_PNG}" <<'PY'
import math, struct, sys, zlib

out_path = sys.argv[1]
size = 1024
radius = 220

def write_png(path, w, h, rgba_rows):
    def chunk(tag, data):
        return struct.pack("!I", len(data)) + tag + data + struct.pack("!I", zlib.crc32(tag + data) & 0xFFFFFFFF)
    raw = b"".join(b"\x00" + row for row in rgba_rows)
    ihdr = struct.pack("!IIBBBBB", w, h, 8, 6, 0, 0, 0)
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", ihdr))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))

rows = []
cx = cy = size / 2.0
for y in range(size):
    row = bytearray(size * 4)
    for x in range(size):
        # 圆角矩形判定
        dx = max(abs(x - cx) - (size / 2.0 - radius), 0.0)
        dy = max(abs(y - cy) - (size / 2.0 - radius), 0.0)
        dist = math.hypot(dx, dy)
        if dist > radius:
            continue
        alpha = 255 if dist <= radius - 1.5 else int(max(0.0, min(1.0, (radius - dist) / 1.5)) * 255)
        # Royal Blue 渐变背景 (#2563EB -> #1D4ED8)
        t = y / size
        r = int(37 * (1 - t) + 29 * t)
        g = int(99 * (1 - t) + 78 * t)
        b = int(235 * (1 - t) + 216 * t)

        # 中心双圆环 + 协同节点符号（纯白）
        cd = math.hypot(x - cx, y - cy)
        in_ring = (230 <= cd <= 275) or (110 <= cd <= 155)
        # 节点连线与圆点
        dot_top = math.hypot(x - cx, y - (cy - 252)) <= 46
        dot_bl = math.hypot(x - (cx - 218), y - (cy + 126)) <= 46
        dot_br = math.hypot(x - (cx + 218), y - (cy + 126)) <= 46
        if in_ring or dot_top or dot_bl or dot_br:
            r, g, b = 255, 255, 255

        idx = x * 4
        row[idx] = r
        row[idx + 1] = g
        row[idx + 2] = b
        row[idx + 3] = alpha
    rows.append(bytes(row))

write_png(out_path, size, size, rows)
PY

    for sz in 16 32 128 256 512; do
        sips -z "${sz}" "${sz}" "${BASE_PNG}" --out "${ICONSET_DIR}/icon_${sz}x${sz}.png" >/dev/null
        sz2=$((sz * 2))
        sips -z "${sz2}" "${sz2}" "${BASE_PNG}" --out "${ICONSET_DIR}/icon_${sz}x${sz}@2x.png" >/dev/null
    done
    iconutil -c icns "${ICONSET_DIR}" -o "${RESOURCES_DIR}/AppIcon.icns"
    rm -rf "${ICON_TMP}"
else
    echo "    跳过图标生成（未检测到 sips/iconutil/python3）"
fi

echo "==> [4/4] 刷新签名与生成版本化产物..."
if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "${APP_DIR}" >/dev/null 2>&1 || true
fi
cp -R "${APP_DIR}" "${VERSIONED_APP_DIR}"
if command -v codesign >/dev/null 2>&1; then
    codesign --force --deep --sign - "${VERSIONED_APP_DIR}" >/dev/null 2>&1 || true
fi

echo "✅ 打包完成 (${APP_VERSION}):"
echo "   标准应用: ${APP_DIR}"
echo "   版本归档: ${VERSIONED_APP_DIR}"
echo "   可直接双击打开，或运行: open \"${APP_DIR}\""

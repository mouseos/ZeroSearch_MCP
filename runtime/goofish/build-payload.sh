#!/usr/bin/env bash
# Build the Goofish signing payload (goofish-payload/) that ships
# next to the server binary in the release ZIP / .mcpb:
#
#   goofish-payload/
#     jre/                      a redistributable Java 8 runtime
#                               (Eclipse Temurin / Azul Zulu, pinned)
#     harness/
#       classes/                local.goofish.SgHarness (compiled here)
#       classes-patched/        the two patched unidbg classes
#       lib/*.jar               the pinned unidbg 0.9.9 dependency set
#     payload.json              what the payload is + where the server
#                               gets the Goofish app package from
#
# The Goofish APK itself is NOT part of the payload (Alibaba-
# proprietary, ~269 MB, not redistributable). The server downloads
# it on first Goofish use from the pinned app-store CDN URL in
# payload.json, verifies its SHA-256, and assembles the runtime in
# its own data directory. Users never run this script and never see
# Maven or a JDK: this runs at release time (CI) only.
#
# Usage:
#   ./build-payload.sh [platform] [output-dir]
#     platform: linux-x64 | win32-x64 | macos-arm64
#               (default: detected from the host)
#     output-dir default: <repo>/runtime/goofish/payload-dist/<platform>
#               (the payload itself lands in <output-dir>/goofish-payload)
#
# Build-time requirements (release machines only):
#   - a JDK 8 with javac: $JAVA_HOME, else $HOME/tools/jdk8,
#     else javac on PATH (version-checked to be 1.8)
#   - dependency jars, resolved in this order:
#       1. $GOOFISH_DEPS_LIB — a flat directory of jars
#       2. <repo>/runtime/goofish/.deps-lib/ — same, conventional spot
#       3. Maven (`mvn dependency:copy-dependencies`, Maven Central)
#       4. the pinned jar paths inside a Maven local repository
#          ($MAVEN_REPO_LOCAL, else ~/.m2/repository,
#          else $HOME/.m2/repository)
#   - curl, python3, unzip (Windows JRE), tar
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"

PLATFORM="${1:-}"
if [ -z "$PLATFORM" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) PLATFORM="linux-x64" ;;
    Linux-aarch64) PLATFORM="linux-arm64" ;;
    Darwin-arm64) PLATFORM="macos-arm64" ;;
    Darwin-x86_64) PLATFORM="macos-x64" ;;
    MINGW*-x86_64|MSYS*-x86_64) PLATFORM="win32-x64" ;;
    *) echo "cannot detect platform; pass one explicitly" >&2; exit 1 ;;
  esac
fi
OUT_BASE="${2:-$HERE/payload-dist/$PLATFORM}"
OUT="$OUT_BASE/goofish-payload"

# ---------------------------------------------------------------------------
# Pinned inputs. The APK constants must match the provider's
# provisioning defaults (crates/providers/goofish/src/provision.rs).
# ---------------------------------------------------------------------------

APK_VERSION="7.28.70"
# Tencent app-store (应用宝) CDN, content-addressed by the file's
# MD5: this exact build (the `_yyb` distribution build whose channel
# the emulated identity uses) is served at this URL. Verified
# 2026-11-08: range GET returns 206 with the APK content type and
# the full download hashes to APK_SHA256 below.
APK_URL="https://imtt.dd.qq.com/16891/apk/361C15B2C9BC291581DBE4FF8194A899.apk"
APK_SHA256="905718aff71859527c53b80c88a971fc47c7aa5e8c1a6d3d661b9e073b837530"
APK_SIZE="268743306"
MAIN_SO="libsgmainso-6.7.260202.so"

case "$PLATFORM" in
  linux-x64)
    JRE_URL="https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u512-b09/OpenJDK8U-jre_x64_linux_hotspot_8u512b09.tar.gz"
    JRE_SHA256="57468dc8bfd71dd0578d9717e63a8c13b9efe0a2f2c85c2a13966d1c0f9b1e5f"
    JRE_KIND="tar.gz"; JRE_VENDOR="Eclipse Temurin"; JRE_VERSION="8u512-b09"
    ;;
  win32-x64)
    JRE_URL="https://github.com/adoptium/temurin8-binaries/releases/download/jdk8u512-b09/OpenJDK8U-jre_x64_windows_hotspot_8u512b09.zip"
    JRE_SHA256="ae1ae12f817d7735477c6555107eece73854da0654da69077309ef9bff0b78bf"
    JRE_KIND="zip"; JRE_VENDOR="Eclipse Temurin"; JRE_VERSION="8u512-b09"
    ;;
  macos-arm64)
    # Temurin publishes no Java 8 JRE for macOS/aarch64; Azul Zulu
    # does (verified: Contents/Home/bin/java is a Mach-O arm64
    # executable). SHA-256 measured on the downloaded archive.
    JRE_URL="https://cdn.azul.com/zulu/bin/zulu8.98.0.111-ca-jre8.0.512-macosx_aarch64.tar.gz"
    JRE_SHA256="54758f5d72e8832c4862ec7d9b53be24616048db42ca325cdf7d29a20bb63132"
    JRE_KIND="tar.gz"; JRE_VENDOR="Azul Zulu"; JRE_VERSION="8.0.512 (zulu8.98.0.111-ca-jre)"
    ;;
  *)
    echo "no pinned JRE for platform '$PLATFORM' (supported: linux-x64, win32-x64, macos-arm64)" >&2
    exit 1
    ;;
esac

# The pinned dependency set (unidbg 0.9.9 tree) — the same 19 jars
# the signing harness has always run with, in Maven-repo layout.
PINNED_JARS="
com/github/zhkl0228/unidbg-android/0.9.9/unidbg-android-0.9.9.jar
com/github/zhkl0228/unidbg-api/0.9.9/unidbg-api-0.9.9.jar
com/github/zhkl0228/unidbg-unicorn2/0.9.9/unidbg-unicorn2-0.9.9.jar
com/github/zhkl0228/unidbg-dynarmic/0.9.9/unidbg-dynarmic-0.9.9.jar
com/github/zhkl0228/unidbg-hypervisor/0.9.9/unidbg-hypervisor-0.9.9.jar
com/github/zhkl0228/unidbg-kvm/0.9.9/unidbg-kvm-0.9.9.jar
com/github/zhkl0228/unicorn/1.0.15/unicorn-1.0.15.jar
com/github/zhkl0228/capstone/3.1.8/capstone-3.1.8.jar
com/github/zhkl0228/keystone/0.9.7/keystone-0.9.7.jar
com/github/zhkl0228/demumble/1.0.4/demumble-1.0.4.jar
net/dongliu/apk-parser/2.6.10/apk-parser-2.6.10.jar
com/alibaba/fastjson/1.2.83/fastjson-1.2.83.jar
commons-codec/commons-codec/1.21.0/commons-codec-1.21.0.jar
org/apache/commons/commons-collections4/4.5.0/commons-collections4-4.5.0.jar
commons-io/commons-io/2.21.0/commons-io-2.21.0.jar
org/slf4j/slf4j-api/1.7.36/slf4j-api-1.7.36.jar
net/java/dev/jna/jna/5.10.0/jna-5.10.0.jar
org/scijava/native-lib-loader/2.3.5/native-lib-loader-2.3.5.jar
org/slf4j/slf4j-simple/1.7.36/slf4j-simple-1.7.36.jar
"

# ---------------------------------------------------------------------------
# JDK 8 (build-time only)
# ---------------------------------------------------------------------------

JAVAC=""
for home in "${JAVA_HOME:-}" "$HOME/tools/jdk8"; do
  if [ -n "$home" ] && [ -x "$home/bin/javac" ]; then JAVAC="$home/bin/javac"; break; fi
done
if [ -z "$JAVAC" ] && command -v javac >/dev/null 2>&1; then JAVAC="$(command -v javac)"; fi
[ -n "$JAVAC" ] || { echo "no javac found (set JAVA_HOME to a JDK 8)" >&2; exit 1; }
if ! "$JAVAC" -version 2>&1 | grep -q '1\.8'; then
  echo "javac is not Java 8: $("$JAVAC" -version 2>&1 | head -1)" >&2; exit 1
fi
echo "[build-payload] javac: $JAVAC"

# ---------------------------------------------------------------------------
# Dependency jars -> $WORK/lib (flat)
# ---------------------------------------------------------------------------

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/lib"

collect_from_flat() {  # $1 = flat dir with jars
  local n=0
  for rel in $PINNED_JARS; do
    local base; base="$(basename "$rel")"
    if [ -f "$1/$base" ]; then cp "$1/$base" "$WORK/lib/$base"; n=$((n+1)); fi
  done
  [ "$n" -eq 19 ]
}
collect_from_repo() {  # $1 = maven local repository root
  local n=0
  for rel in $PINNED_JARS; do
    if [ -f "$1/$rel" ]; then cp "$1/$rel" "$WORK/lib/$(basename "$rel")"; n=$((n+1)); fi
  done
  [ "$n" -eq 19 ]
}

DEPS_OK=0
if [ -n "${GOOFISH_DEPS_LIB:-}" ] && collect_from_flat "$GOOFISH_DEPS_LIB"; then
  echo "[build-payload] dependency jars from \$GOOFISH_DEPS_LIB"
  DEPS_OK=1
elif collect_from_flat "$HERE/.deps-lib"; then
  echo "[build-payload] dependency jars from runtime/goofish/.deps-lib"
  DEPS_OK=1
elif command -v mvn >/dev/null 2>&1; then
  echo "[build-payload] resolving dependency jars with Maven"
  mkdir -p "$WORK/mvnharness"
  cp "$HERE/harness/pom.xml" "$WORK/mvnharness/pom.xml"
  # Maven Central rate-limits datacenter egress at times; retry, and
  # keep the log so a failure is diagnosable (this used to discard
  # all Maven output, which made CI failures opaque).
  for attempt in 1 2 3; do
    if (cd "$WORK/mvnharness" && mvn -B org.apache.maven.plugins:maven-dependency-plugin:3.6.1:copy-dependencies -DoutputDirectory="$WORK/mvnlib" >"$WORK/mvn.log" 2>&1) \
       && collect_from_flat "$WORK/mvnlib"; then
      DEPS_OK=1
      break
    fi
    echo "[build-payload] Maven resolution attempt $attempt failed" >&2
    tail -n 30 "$WORK/mvn.log" >&2 || true
    [ "$attempt" -lt 3 ] && sleep 20
  done
fi
if [ "$DEPS_OK" -eq 0 ]; then
  for repo in "${MAVEN_REPO_LOCAL:-}" "$HOME/.m2/repository" "/root/.m2/repository"; do
    if [ -n "$repo" ] && collect_from_repo "$repo"; then
      echo "[build-payload] dependency jars from Maven local repository: $repo"
      DEPS_OK=1; break
    fi
  done
fi
[ "$DEPS_OK" -eq 1 ] || { echo "could not assemble the pinned dependency jars" >&2; exit 1; }

CP=""
for j in "$WORK"/lib/*.jar; do CP="$CP:$j"; done
CP="${CP#:}"

# ---------------------------------------------------------------------------
# Compile the harness (SgHarness + the two patched unidbg classes)
# ---------------------------------------------------------------------------

mkdir -p "$WORK/classes-patched" "$WORK/classes"
"$JAVAC" -encoding UTF-8 -cp "$CP" -d "$WORK/classes-patched" \
  "$HERE/harness/patched/src/com/github/unidbg/linux/"*.java
"$JAVAC" -encoding UTF-8 -cp "$WORK/classes-patched:$CP" -d "$WORK/classes" \
  "$HERE/harness/src/main/java/local/goofish/SgHarness.java"
echo "[build-payload] harness compiled"

# ---------------------------------------------------------------------------
# JRE (download once, verify, normalise to a bin/java root)
# ---------------------------------------------------------------------------

CACHE="$HERE/.jre-cache"
mkdir -p "$CACHE"
ARCHIVE="$CACHE/$(basename "$JRE_URL")"
if [ ! -f "$ARCHIVE" ]; then
  echo "[build-payload] downloading JRE: $JRE_URL"
  curl -sL --fail -o "$ARCHIVE.part" "$JRE_URL"
  mv "$ARCHIVE.part" "$ARCHIVE"
fi
# sha256sum is absent on stock macOS.
if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SHA="$(sha256sum "$ARCHIVE" | cut -d' ' -f1)"
else
  ACTUAL_SHA="$(shasum -a 256 "$ARCHIVE" | cut -d' ' -f1)"
fi
[ "$ACTUAL_SHA" = "$JRE_SHA256" ] || {
  echo "JRE sha256 mismatch: expected $JRE_SHA256, got $ACTUAL_SHA" >&2; exit 1; }
echo "[build-payload] JRE archive verified ($JRE_VENDOR $JRE_VERSION)"

mkdir -p "$WORK/jre-x"
if [ "$JRE_KIND" = "zip" ]; then
  python3 - "$ARCHIVE" "$WORK/jre-x" <<'PY'
import sys, zipfile
zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])
PY
else
  tar -xzf "$ARCHIVE" -C "$WORK/jre-x"
fi
# The runtime root is the directory that contains bin/java[.exe]
# (Temurin: the single top directory; Zulu macOS: <top>/Contents/Home).
JRE_ROOT=""
for cand in "$WORK"/jre-x/*/ "$WORK"/jre-x/*/Contents/Home/; do
  if [ -f "${cand}bin/java" ] || [ -f "${cand}bin/java.exe" ]; then JRE_ROOT="$cand"; break; fi
done
[ -n "$JRE_ROOT" ] || { echo "could not locate bin/java in the JRE archive" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Assemble the payload
# ---------------------------------------------------------------------------

rm -rf "$OUT"
mkdir -p "$OUT/harness/lib"
cp -r "${JRE_ROOT%/}" "$OUT/jre"
cp -r "$WORK/classes" "$OUT/harness/classes"
cp -r "$WORK/classes-patched" "$OUT/harness/classes-patched"
cp "$WORK"/lib/*.jar "$OUT/harness/lib/"
chmod +x "$OUT/jre/bin/java" 2>/dev/null || true

JAR_NAMES="$(for j in "$OUT"/harness/lib/*.jar; do basename "$j"; done | python3 -c 'import json,sys; print(json.dumps([l.strip() for l in sys.stdin if l.strip()]))')"
python3 - "$OUT/payload.json" "$PLATFORM" "$JRE_VENDOR" "$JRE_VERSION" \
  "$APK_VERSION" "$APK_URL" "$APK_SHA256" "$APK_SIZE" "$MAIN_SO" "$JAR_NAMES" <<'PY'
import json, sys
(path, platform, jre_vendor, jre_version, apk_version, apk_url,
 apk_sha256, apk_size, main_so, jar_names) = sys.argv[1:11]
doc = {
    "format": 1,
    "platform": platform,
    "jre": {"vendor": jre_vendor, "version": jre_version},
    "apk": {
        "version": apk_version,
        "url": apk_url,
        "sha256": apk_sha256,
        "size": int(apk_size),
    },
    "main_so": main_so,
    "harness": {"main_class": "local.goofish.SgHarness",
                "jars": json.loads(jar_names)},
}
with open(path, "w") as f:
    json.dump(doc, f, indent=2)
    f.write("\n")
PY

echo "[build-payload] payload assembled at: $OUT"
du -sh "$OUT" | sed 's/^/[build-payload] payload size: /'
find "$OUT" -maxdepth 2 -type d | sort | sed 's/^/  /'

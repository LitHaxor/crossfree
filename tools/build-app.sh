#!/bin/sh
# Builds target/crossfree.app: the SwiftUI app (app/) with the crossfree
# command-line tool (crates/launcher) inside it.
#
#   CONFIG=debug|release   build configuration (default: release)
#   VERSION=X.Y.Z           the app's version (default: 0.1.0; releases pass
#                           the tag's)
#   CROSSFREE_ENGINE=DIR    engine bundle to put inside the app
#                           (Contents/SharedSupport/engine), as CrossOver
#                           ships its Wine: the app then needs no setup, and
#                           macOS does not ask for access to wherever DIR is
#   SIGN_IDENTITY=NAME      a Developer ID Application identity: signs every
#                           program inside with the hardened runtime and a
#                           secure timestamp, as notarization requires
#                           (default: ad hoc, for local builds)
#   HARDENED_RUNTIME=1      signs ad hoc the way a Developer ID build does, to
#                           try that build locally
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
config=${CONFIG:-release}
version=${VERSION:-0.1.0}
case "$config" in
    release) cargo_flags=--release ;;
    debug) cargo_flags= ;;
    *) echo "CONFIG must be debug or release" >&2; exit 2 ;;
esac

# shellcheck disable=SC2086 # empty for debug builds: no argument at all
cargo build $cargo_flags -p launcher --manifest-path "$root/Cargo.toml"
swift build -c "$config" --package-path "$root/app"

app="$root/target/crossfree.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$root/app/.build/$config/Crossfree" "$app/Contents/MacOS/Crossfree"
cp "$root/target/$config/crossfree" "$app/Contents/Resources/crossfree"
# Images the app shows: its icon (tools/make-icon.swift) and the Directory's
# store logos (tools/store-logos.py).
ditto "$root/app/Resources" "$app/Contents/Resources"

if [ -n "${CROSSFREE_ENGINE:-}" ]; then
    # ditto clones on APFS (no extra space) and keeps the engine's signed
    # loader exactly as it is.
    ditto "$CROSSFREE_ENGINE" "$app/Contents/SharedSupport/engine"
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>crossfree</string>
    <key>CFBundleDisplayName</key>
    <string>crossfree</string>
    <key>CFBundleIdentifier</key>
    <string>app.crossfree.Crossfree</string>
    <key>CFBundleExecutable</key>
    <string>Crossfree</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>26.4</string>
    <key>LSApplicationCategoryType</key>
    <string>public.app-category.games</string>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

identity=${SIGN_IDENTITY:--}
if [ "$identity" != - ]; then
    sign_flags="--options runtime --timestamp"
elif [ "${HARDENED_RUNTIME:-0}" = 1 ]; then
    sign_flags="--options runtime"
else
    sign_flags=
fi

sign() {
    # shellcheck disable=SC2086 # the flags are separate words, or none
    codesign --force $sign_flags --sign "$identity" "$@"
}

# A Developer ID build signs the engine's programs and libraries as its
# own; ad hoc ones keep the signatures they came with. The loader,
# wine.app, always keeps mach9's notarized signature, which carries the
# entitlements Wine needs (and needs no more for notarization).
engine="$app/Contents/SharedSupport/engine"
if [ -n "$sign_flags" ] && [ -d "$engine" ]; then
    find "$engine" -type f ! -path '*/wine.app/*' -print0 \
        | xargs -0 file -N --mime-type -F ' :: ' \
        | sed -n 's/ :: *application\/x-mach-binary$//p' \
        | while IFS= read -r program; do sign "$program"; done
fi
sign "$app/Contents/Resources/crossfree"
sign "$app"
echo "$app"

#!/bin/bash
# Build a local macOS application bundle with a stable microphone identity.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/bundle.sh [--debug] [--no-build] [--preview] [--output PATH.app]

Build Voice.app for the current Mac. Release is the default profile.
Use --debug for a development build or --no-build to package an existing binary.
Use --preview for an isolated app identity that preserves a running Voice session.
The default output is dist/Voice.app. Bundles are signed ad hoc for local use.
EOF
}

profile=release
build=true
preview=false
output=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug) profile=debug; shift ;;
        --no-build) build=false; shift ;;
        --preview) preview=true; shift ;;
        --output)
            [[ $# -ge 2 ]] || { usage >&2; exit 2; }
            output=$2
            shift 2
            ;;
        --help|-h) usage; exit 0 ;;
        *) printf 'Unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
    esac
done

[[ $(uname -s) == Darwin ]] || { printf 'Voice.app bundling requires macOS.\n' >&2; exit 1; }
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo_root"
cargo_bin=${CARGO:-cargo}
if ! command -v "$cargo_bin" >/dev/null 2>&1; then
    cargo_bin="$HOME/.cargo/bin/cargo"
fi
[[ -x $(command -v "$cargo_bin" 2>/dev/null || true) ]] || { printf 'Cargo is required. Install Rust before bundling.\n' >&2; exit 1; }

if [[ "$build" == true ]]; then
    if [[ "$profile" == release ]]; then
        "$cargo_bin" build --release --bin voice
    else
        "$cargo_bin" build --bin voice
    fi
fi

metadata=$("$cargo_bin" metadata --format-version 1 --no-deps)
target_dir=$(printf '%s' "$metadata" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
version=$(printf '%s' "$metadata" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(next(p["version"] for p in d["packages"] if p["name"]=="voice"))')
binary_dir="$target_dir/$profile"
if [[ -n ${CARGO_BUILD_TARGET:-} ]]; then
    binary_dir="$target_dir/$CARGO_BUILD_TARGET/$profile"
fi
binary="$binary_dir/voice"
[[ -f "$binary" ]] || { printf 'Built executable not found: %s\n' "$binary" >&2; exit 1; }

if [[ "$preview" == true ]]; then
    output=${output:-"$repo_root/build/Voice Preview.app"}
else
    output=${output:-"$repo_root/dist/Voice.app"}
fi
case "$output" in *.app) ;; *) printf 'Output must have an .app extension.\n' >&2; exit 2 ;; esac
output_parent=$(dirname -- "$output")
mkdir -p -- "$output_parent"
output_parent=$(cd -- "$output_parent" && pwd)
output="$output_parent/$(basename -- "$output")"
stage_root=$(mktemp -d "$output_parent/.voice-bundle.XXXXXX")
trap 'rm -rf -- "$stage_root"' EXIT
bundle="$stage_root/Voice.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Frameworks" "$bundle/Contents/Resources"
cp "$binary" "$bundle/Contents/MacOS/voice"
chmod 755 "$bundle/Contents/MacOS/voice"
cp "$repo_root/macOS/Info.plist" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $version" "$bundle/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $version" "$bundle/Contents/Info.plist"
if [[ "$preview" == true ]]; then
    /usr/libexec/PlistBuddy -c 'Set :CFBundleIdentifier dev.mmonteiro.voice.preview' "$bundle/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Set :CFBundleName Voice Preview' "$bundle/Contents/Info.plist"
    /usr/libexec/PlistBuddy -c 'Set :CFBundleDisplayName Voice Preview' "$bundle/Contents/Info.plist"
    # A review build must not become the owner of the user's project file type.
    /usr/libexec/PlistBuddy -c 'Delete :CFBundleDocumentTypes' "$bundle/Contents/Info.plist"
fi
if [[ -d "$repo_root/assets" ]]; then
    /usr/bin/ditto "$repo_root/assets" "$bundle/Contents/Resources/assets"
fi

# ort's copy-dylibs feature places runtime libraries beside the executable.
# Preserve their filenames, then use a bundle-relative rpath for portability.
shopt -s nullglob
for library in "$binary_dir"/*.dylib; do
    cp -L "$library" "$bundle/Contents/Frameworks/$(basename -- "$library")"
done

bundle_binary="$bundle/Contents/MacOS/voice"
runtime_libraries=("$bundle/Contents/Frameworks"/*.dylib)
if [[ ${#runtime_libraries[@]} -gt 0 ]] && ! /usr/bin/otool -l "$bundle_binary" | /usr/bin/grep -Fq '@executable_path/../Frameworks'; then
    /usr/bin/install_name_tool -add_rpath '@executable_path/../Frameworks' "$bundle_binary"
fi

# Normalize absolute build-directory references in both the binary and copied
# libraries. System frameworks remain supplied by macOS.
for image in "$bundle_binary" "$bundle/Contents/Frameworks"/*.dylib; do
    while IFS= read -r dependency; do
        case "$dependency" in
            /System/*|/usr/lib/*) continue ;;
        esac
        library_name=$(basename -- "$dependency")
        bundled_library="$bundle/Contents/Frameworks/$library_name"
        if [[ -f "$bundled_library" ]]; then
            /usr/bin/install_name_tool -change "$dependency" "@rpath/$library_name" "$image"
        elif [[ "$dependency" == /* ]]; then
            printf 'Unbundled external dependency: %s\n' "$dependency" >&2
            exit 1
        fi
    done < <(/usr/bin/otool -L "$image" | /usr/bin/tail -n +2 | /usr/bin/sed -E 's/^[[:space:]]*(.*) \(compatibility version.*$/\1/')
done

for library in "$bundle/Contents/Frameworks"/*.dylib; do
    /usr/bin/install_name_tool -id "@rpath/$(basename -- "$library")" "$library"
    /usr/bin/codesign --force --sign - --timestamp=none "$library"
done
/usr/bin/plutil -lint "$bundle/Contents/Info.plist"
/usr/bin/codesign --force --sign - --timestamp=none "$bundle"
/usr/bin/codesign --verify --strict "$bundle"

if [[ -e "$output" ]]; then
    rm -rf -- "$output"
fi
mv -- "$bundle" "$output"
printf 'Built %s\nOpen with: open "%s"\n' "$output" "$output"

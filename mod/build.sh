#!/usr/bin/env bash
set -euo pipefail

# 1. Pick a Mono build tool: prefer msbuild, fall back to xbuild.
if command -v msbuild >/dev/null 2>&1; then
  BUILDER=msbuild
elif command -v xbuild >/dev/null 2>&1; then
  BUILDER=xbuild
else
  echo "No Mono build tool found (msbuild/xbuild). Install Mono:" >&2
  echo "  macOS:  brew install mono" >&2
  echo "  Arch:   pacman -S mono mono-msbuild" >&2
  echo "  Debian: apt install mono-complete mono-xbuild" >&2
  exit 1
fi

# Steam libraryfolders.vdf lives next to steamapps/ in each install root.
steam_vdf_paths() {
  local p
  for p in \
    "${STEAM_LIBRARY_VDF:-}" \
    "$HOME/.steam/steam/steamapps/libraryfolders.vdf" \
    "$HOME/.steam/root/steamapps/libraryfolders.vdf" \
    "$HOME/.local/share/Steam/steamapps/libraryfolders.vdf" \
    "$HOME/Library/Application Support/Steam/steamapps/libraryfolders.vdf"
  do
    [ -n "$p" ] && [ -f "$p" ] && printf '%s\n' "$p"
  done
}

# Collect Steam library roots from libraryfolders.vdf ("path" "…") plus common fallbacks.
steam_library_roots() {
  local vdf root
  while IFS= read -r vdf; do
    [ -f "$vdf" ] || continue
    # shellcheck disable=SC2016
    sed -n 's/.*"path"[[:space:]]*"\([^"]*\)".*/\1/p' "$vdf"
  done < <(steam_vdf_paths)
  for root in \
    "$HOME/.local/share/Steam" \
    "$HOME/.steam/steam" \
    "$HOME/.steam/root" \
    "$HOME/Library/Application Support/Steam" \
    "$HOME/Games/SteamLibrary"
  do
    printf '%s\n' "$root"
  done
}

# 2. Locate the game's Managed dir (override with MANAGED_DLL_PATH=...)
find_managed() {
  if [ -n "${MANAGED_DLL_PATH:-}" ]; then
    printf '%s\n' "$MANAGED_DLL_PATH"
    return 0
  fi
  local root candidate
  while IFS= read -r root; do
    [ -n "$root" ] || continue
    for candidate in \
      "$root/steamapps/common/Cities_Skylines/Cities_Data/Managed" \
      "$root/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed"
    do
      if [ -f "$candidate/ICities.dll" ]; then
        printf '%s\n' "$candidate"
        return 0
      fi
    done
  done < <(steam_library_roots)
  return 1
}

MANAGED="$(find_managed || true)"
if [ -z "$MANAGED" ] || [ ! -f "$MANAGED/ICities.dll" ]; then
  echo "Game assemblies not found (looked in Steam libraries for Cities_Skylines …/Managed)." >&2
  echo "Set MANAGED_DLL_PATH to your Cities_Data/Managed (Linux/Windows) or Cities.app/…/Data/Managed (macOS) directory." >&2
  exit 1
fi

# 3. Compile (Release)
DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Building with $BUILDER against: $MANAGED"
"$BUILDER" /p:Configuration=Release /p:ManagedDLLPath="$MANAGED" "$DIR/SkylineBenchMod.csproj"

# 4. Install
case "$(uname -s)" in
  Darwin)
    MODS="$HOME/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench"
    ;;
  *)
    MODS="$HOME/.local/share/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench"
    ;;
esac
mkdir -p "$MODS"
cp "$DIR/bin/Release/SkylineBenchMod.dll" "$MODS/"
echo "Installed SkylineBenchMod.dll -> $MODS"
echo "Now enable 'SkylineBench Bridge' in the game's Content Manager > Mods, then load a city."

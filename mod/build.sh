#!/usr/bin/env bash
set -euo pipefail

# 1. Pick a Mono build tool: prefer msbuild, fall back to xbuild.
if command -v msbuild >/dev/null 2>&1; then
  BUILDER=msbuild
elif command -v xbuild >/dev/null 2>&1; then
  BUILDER=xbuild
else
  echo "No Mono build tool found (msbuild/xbuild). Install Mono:  brew install mono" >&2
  exit 1
fi

# 2. Locate the game's Managed dir (override with MANAGED_DLL_PATH=...)
DEFAULT_MANAGED="$HOME/Library/Application Support/Steam/steamapps/common/Cities_Skylines/Cities.app/Contents/Resources/Data/Managed"
MANAGED="${MANAGED_DLL_PATH:-$DEFAULT_MANAGED}"
if [ ! -f "$MANAGED/ICities.dll" ]; then
  echo "Game assemblies not found at: $MANAGED" >&2
  echo "Set MANAGED_DLL_PATH to your Cities.app .../Data/Managed directory." >&2
  exit 1
fi

# 3. Compile (Release)
DIR="$(cd "$(dirname "$0")" && pwd)"
echo "Building with $BUILDER against: $MANAGED"
"$BUILDER" /p:Configuration=Release /p:ManagedDLLPath="$MANAGED" "$DIR/SkylineBenchMod.csproj"

# 4. Install
MODS="$HOME/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench"
mkdir -p "$MODS"
cp "$DIR/bin/Release/SkylineBenchMod.dll" "$MODS/"
echo "Installed SkylineBenchMod.dll -> $MODS"
echo "Now enable 'SkylineBench Bridge' in the game's Content Manager > Mods, then load a city."

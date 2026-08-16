using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Packaging;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Loads a named savegame mid-session (the reset_scenario primitive). This mirrors the
    /// game's own <c>LoadPanel.LoadRoutine</c> / <c>QuickLoad</c> path (confirmed via monodis
    /// on Assembly-CSharp.dll 1.21):
    /// find the <c>SaveGameMetaData</c> asset by name, instantiate it, build a
    /// <c>SimulationMetaData</c> with <c>m_updateMode = UpdateMode.LoadGame</c> and the city
    /// name, then call the 5-arg
    /// <c>LoadingManager.LoadLevel(metaData.assetRef, "Game", "InGame", meta, forceEnvironmentReload:false)</c>.
    ///
    /// The asset passed to LoadLevel MUST be <c>MetaData.assetRef</c> (the simulation save
    /// blob inside the .crp), not the SaveGameMetaData listing asset.
    /// <c>GetListingData</c> in LoadSavePanelBase is exactly <c>item.mmd.assetRef</c>.
    /// Passing the metadata asset makes the native DataSerializer read the CRP/metadata
    /// header as a simulation format version (e.g. 5788513 &gt; 121034) — the Linux
    /// "file format version not supported" failure. The in-game Load panel never hits
    /// that because it always uses assetRef.
    ///
    /// Heavyweight: LoadLevel tears down and reloads the level. The call is dispatched onto the
    /// sim thread and returns immediately after kick-off; the load itself proceeds asynchronously
    /// via the returned coroutine, so a true success/completion signal can only be observed
    /// in-game. We do NOT block on completion here.
    /// </summary>
    public static class SaveLoader
    {
        public static LoadResultDto Load(string saveName)
        {
            if (string.IsNullOrEmpty(saveName)) return new LoadResultDto { Ok = false, CityLoaded = false };

            Package.Asset target = FindSave(saveName);
            if (target == null) return Miss();

            SaveInfoDto resolved = Describe(target);

            SaveGameMetaData metaData = null;
            Package.Asset dataAsset = null;
            try
            {
                metaData = target.Instantiate<SaveGameMetaData>();
                if (metaData != null) dataAsset = metaData.assetRef;
            }
            catch
            {
                // Corrupt metadata: treat as a miss so the caller sees available names.
            }
            if (dataAsset == null)
            {
                var miss = Miss();
                miss.Resolved = resolved;
                return miss;
            }

            string cityName = metaData.cityName;
            string themeRef = metaData.mapThemeRef;

            SimThread.Run(delegate
            {
                SimulationMetaData meta = new SimulationMetaData();
                meta.m_CityName = cityName;
                meta.m_updateMode = SimulationManager.UpdateMode.LoadGame;
                ApplyMapTheme(meta, themeRef);
                Singleton<LoadingManager>.instance.LoadLevel(dataAsset, "Game", "InGame", meta, false);
            }, 8000);

            // Load runs asynchronously after kick-off; callers confirm completion
            // by polling /health (the bridge restarts on level reload).
            return new LoadResultDto { Ok = true, CityLoaded = true, Resolved = resolved };
        }

        public static List<SaveInfoDto> ListSaves()
        {
            var list = new List<SaveInfoDto>();
            foreach (Package.Asset asset in PackageManager.FilterAssets(UserAssetType.SaveGameMetaData))
            {
                if (asset == null) continue;
                list.Add(Describe(asset));
            }
            return list;
        }

        /// <summary>Match a currently-loaded city name against the save listing so
        /// /health can report the bound save file name (asset.name).</summary>
        public static string SaveNameForCity(string cityName)
        {
            if (string.IsNullOrEmpty(cityName)) return null;
            foreach (SaveInfoDto s in ListSaves())
            {
                if (s.CityName == cityName || s.Name == cityName || s.FullName == cityName)
                    return s.Name;
            }
            return null;
        }

        private static void ApplyMapTheme(SimulationMetaData meta, string themeRef)
        {
            if (string.IsNullOrEmpty(themeRef)) return;
            Package.Asset theme = PackageManager.FindAssetByName(themeRef, UserAssetType.MapThemeMetaData);
            if (theme == null) return;
            MapThemeMetaData themeMeta = theme.Instantiate<MapThemeMetaData>();
            if (themeMeta == null) return;
            themeMeta.SetSelfRef(theme);
            meta.m_MapThemeMetaData = themeMeta;
        }

        private static LoadResultDto Miss()
        {
            return new LoadResultDto { Ok = false, CityLoaded = false, Available = ListSaves() };
        }

        private static SaveInfoDto Describe(Package.Asset asset)
        {
            string cityName = null;
            try
            {
                SaveGameMetaData metaData = asset.Instantiate<SaveGameMetaData>();
                if (metaData != null) cityName = metaData.cityName;
            }
            catch
            {
                // Corrupt save: report name/fullName without cityName.
            }
            return new SaveInfoDto { Name = asset.name, CityName = cityName, FullName = asset.fullName };
        }

        // We iterate rather than use PackageManager.FindAssetByName: that method returns null for
        // any name without a '.' and otherwise only matches the package-qualified fullName
        // (e.g. "packageName.assetName"), never the bare save name or city name. Since the
        // agent/broker passes a human-friendly name (e.g. "MyCity"), we match flexibly instead.
        private static Package.Asset FindSave(string saveName)
        {
            if (string.IsNullOrEmpty(saveName)) return null;

            Package.Asset fullNameMatch = null;
            foreach (Package.Asset asset in PackageManager.FilterAssets(UserAssetType.SaveGameMetaData))
            {
                if (asset == null) continue;
                if (asset.name == saveName) return asset;

                try
                {
                    SaveGameMetaData metaData = asset.Instantiate<SaveGameMetaData>();
                    if (metaData != null && metaData.cityName == saveName) return asset;
                }
                catch
                {
                    // Corrupt save: skip cityName matching for this asset, keep searching.
                }

                if (asset.fullName == saveName) fullNameMatch = asset;
            }

            return fullNameMatch;
        }
    }
}

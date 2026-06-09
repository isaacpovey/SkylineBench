using ColossalFramework;
using ColossalFramework.Packaging;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Loads a named savegame mid-session (the reset_scenario primitive). This mirrors the
    /// game's own <c>LoadPanel.Load</c> path (confirmed via monodis on Assembly-CSharp.dll):
    /// find the <c>SaveGameMetaData</c> asset by name, build a <c>SimulationMetaData</c> with
    /// <c>m_updateMode = UpdateMode.LoadGame</c> and the city name, then call the 5-arg
    /// <c>LoadingManager.LoadLevel(asset, "Game", "InGame", meta, forceASync:false)</c>.
    ///
    /// Heavyweight: LoadLevel tears down and reloads the level. The call is dispatched onto the
    /// sim thread and returns immediately after kick-off; the load itself proceeds asynchronously
    /// via the returned coroutine, so a true success/completion signal can only be observed
    /// in-game (D1 exercises the actual load). We do NOT block on completion here.
    /// </summary>
    public static class SaveLoader
    {
        public static ActionResultDto Load(string saveName)
        {
            if (string.IsNullOrEmpty(saveName)) return ActionResultDto.Fail(ErrorCode.InvalidArgs);

            Package.Asset target = FindSave(saveName);
            if (target == null) return ActionResultDto.Fail(ErrorCode.InvalidArgs);

            SimThread.Run(delegate
            {
                SaveGameMetaData metaData = target.Instantiate<SaveGameMetaData>();
                SimulationMetaData meta = new SimulationMetaData();
                meta.m_CityName = metaData != null ? metaData.cityName : null;
                meta.m_updateMode = SimulationManager.UpdateMode.LoadGame;
                Singleton<LoadingManager>.instance.LoadLevel(target, "Game", "InGame", meta, false);
            }, 8000);

            return new ActionResultDto { Ok = true };
        }

        private static Package.Asset FindSave(string saveName)
        {
            return PackageManager.FindAssetByName(saveName, UserAssetType.SaveGameMetaData);
        }
    }
}

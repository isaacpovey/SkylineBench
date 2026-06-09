using ICities;
using UnityEngine;

namespace SkylineBench
{
    public sealed class Mod : IUserMod
    {
        public string Name { get { return "SkylineBench Bridge"; } }
        public string Description { get { return "Localhost HTTP bridge for the SkylineBench AI harness."; } }
    }

    public sealed class SkylineLoading : LoadingExtensionBase
    {
        public override void OnLevelLoaded(LoadMode mode)
        {
            if (mode == LoadMode.LoadGame || mode == LoadMode.NewGame || mode == LoadMode.NewGameFromScenario)
            {
                Bridge.ModRuntime.Start();
            }
        }

        public override void OnLevelUnloading()
        {
            Bridge.ModRuntime.Stop();
        }
    }

    public sealed class SkylineThreading : ThreadingExtensionBase
    {
        public override void OnBeforeSimulationTick()
        {
            Bridge.ModRuntime.SetThreading(threadingManager);
            Bridge.SimThread.DrainOnSimThread();
        }
    }
}

using ColossalFramework;
using ICities;
using SkylineBench.Http;

namespace SkylineBench.Bridge
{
    public struct HealthInfo
    {
        public string GameVersion;
        public bool CityLoaded;
        public bool Paused;
        public bool ForcedPaused;
        public uint Tick;
    }

    public static class GameAccess
    {
        public static HealthInfo ReadHealth()
        {
            var t = ModRuntime.Threading;
            return new HealthInfo
            {
                GameVersion = GameVersionString(),
                CityLoaded = t != null,
                Paused = t != null && t.simulationPaused,
                ForcedPaused = ForcedPaused(),
                Tick = t != null ? t.simulationTick : 0u
            };
        }

        /// <summary>Game modal dialogs set SimulationManager.ForcedSimulationPaused — a pause
        /// channel separate from IThreading.simulationPaused. Tick counters keep advancing
        /// while it is set, but no simulation happens.</summary>
        public static bool ForcedPaused()
        {
            try { return Singleton<SimulationManager>.instance.ForcedSimulationPaused; }
            catch { return false; }
        }

        private static string GameVersionString()
        {
            try { return BuildConfig.applicationVersion; }
            catch { return "unknown"; }
        }
    }

    public static class ModRuntime
    {
        private static HttpServer _server;
        public static IThreading Threading { get; private set; }

        public static void SetThreading(IThreading t) { Threading = t; }

        public static void Start()
        {
            if (_server != null) return;
            _server = new HttpServer(8787, Router.Route);
            _server.Start();
        }

        public static void Stop()
        {
            if (_server != null) { _server.Stop(); _server = null; }
            Threading = null;
        }
    }
}

using System;
using ColossalFramework;
using ColossalFramework.UI;
using ICities;
using SkylineBench.Http;
using UnityEngine;

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

        /// <summary>Dismiss the modal dialog that force-pauses the simulation.
        /// A benchmark run has no operator, so when the game pops a modal — the
        /// population-milestone celebration ("UnlockingPanel") is the common one
        /// past ~35k pop — nothing dismisses it and the sim is frozen for the
        /// rest of the run. Closing it must happen on Unity's main thread (UI
        /// access), so we dispatch through CaptureBehaviour. Each step retries
        /// this, so the modal is cleared the next time the game tries to advance
        /// time. All UI work is best-effort; clearing the flag is the actual
        /// unblock and is generic to whatever dialog was showing.</summary>
        public static void DismissForcedPauseModal()
        {
            try { CaptureBehaviour.RunOnMain(ClearModalNow, 5000); }
            catch { /* dispatch timed out; Step re-checks ForcedPaused and bails */ }
        }

        /// <summary>Main-thread-only modal teardown. The milestone celebration
        /// must be closed through the game's own handler (OnClosed — the same
        /// path as clicking its close button): that stops the fireworks
        /// particle system, pops the modal, fades out the grey dim overlay and
        /// un-force-pauses. Hiding the raw UIComponent leaves the fireworks
        /// playing and the dim overlay burnt into every later frame.</summary>
        public static void ClearModalNow()
        {
            bool closing = false;
            try
            {
                var panel = ToolsModifierControl.unlockingPanel;
                if (panel != null && panel.isVisible) { panel.OnClosed(); closing = true; }
            }
            catch { /* panel lookup varies by game version; fall through */ }
            if (!closing)
            {
                // Some other dialog: pop and hide it directly, and clear any
                // leftover dim overlay. Skipped when OnClosed ran — the game's
                // own teardown pops the modal itself and a second PopModal here
                // would underflow the modal stack.
                try
                {
                    int guard = 0;
                    while (UIView.HasModalInput() && guard++ < 16)
                    {
                        var modal = UIView.GetModalComponent();
                        UIView.PopModal();
                        if (modal != null && modal.isVisible) modal.Hide();
                    }
                    var view = UIView.GetAView();
                    var dim = view != null ? view.panelsLibraryModalEffect : null;
                    if (dim != null && dim.isVisible) dim.Hide();
                }
                catch { /* UI shape varies by game version; flag-clear below is the fallback */ }
            }
            try { Singleton<SimulationManager>.instance.ForcedSimulationPaused = false; }
            catch { }
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
        private static GameObject _capture;
        public static IThreading Threading { get; private set; }

        public static void SetThreading(IThreading t) { Threading = t; }

        public static void Start()
        {
            if (_server != null) return;
            _server = new HttpServer(8787, Router.Route);
            _server.Start();
            _capture = new GameObject("SkylineBenchCapture");
            _capture.AddComponent<CaptureBehaviour>();
            UnityEngine.Object.DontDestroyOnLoad(_capture);
        }

        public static void Stop()
        {
            if (_server != null) { _server.Stop(); _server = null; }
            if (_capture != null)
            {
                CaptureBehaviour.CancelAll(new Exception("mod stopping"));
                UnityEngine.Object.Destroy(_capture);
                _capture = null;
            }
            Threading = null;
        }
    }
}

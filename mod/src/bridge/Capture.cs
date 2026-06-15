using System;
using System.Collections;
using System.Collections.Generic;
using System.Threading;
using SkylineBench.Json;
using UnityEngine;

namespace SkylineBench.Bridge
{
    public sealed class CaptureRequest
    {
        public float X, Z, Size, Yaw, Pitch;
        public string InfoView;
        public byte[] Png;
        public Exception Error;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }

    public sealed class FlybyRequest
    {
        public KeyframeReq[] Keyframes;
        public float DurationS;
        public int CaptureFps;
        public string OutDir;
        public Exception Error;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }

    /// <summary>A unit of work that must run on Unity's main thread. Used for
    /// UI operations (e.g. dismissing a modal dialog) that cannot run on the
    /// HTTP thread. The caller enqueues and blocks on Done.</summary>
    public sealed class MainThreadAction
    {
        public Action Work;
        public Exception Error;
        public readonly ManualResetEvent Done = new ManualResetEvent(false);
    }

    /// <summary>Runs screenshot captures on Unity's main thread. The HTTP
    /// thread enqueues a request and blocks on Done; Update() drains the queue
    /// and runs one coroutine per request (the sim is paused between agent
    /// steps, so requests never race game mutations). Also drains a generic
    /// main-thread action queue (see RunOnMain) for UI work like dismissing a
    /// modal dialog.</summary>
    public sealed class CaptureBehaviour : MonoBehaviour
    {
        private static readonly Queue<CaptureRequest> _queue = new Queue<CaptureRequest>();
        private static readonly Queue<MainThreadAction> _actions = new Queue<MainThreadAction>();
        private static readonly Queue<FlybyRequest> _flybys = new Queue<FlybyRequest>();
        private static readonly object _lock = new object();

        public static byte[] Capture(float x, float z, float size, float yaw, float pitch, string infoView, int timeoutMs)
        {
            var req = new CaptureRequest { X = x, Z = z, Size = size, Yaw = yaw, Pitch = pitch, InfoView = infoView };
            lock (_lock) { _queue.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("screenshot capture timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
            return req.Png;
        }

        public static void Flyby(FlybyRequest req, int timeoutMs)
        {
            lock (_lock) { _flybys.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("flyby timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
        }

        /// <summary>Run an action on Unity's main thread and block until it
        /// completes. Update() drains the queue, so this works even while the
        /// simulation is force-paused (the main loop keeps running).</summary>
        public static void RunOnMain(Action work, int timeoutMs)
        {
            var job = new MainThreadAction { Work = work };
            lock (_lock) { _actions.Enqueue(job); }
            if (!job.Done.WaitOne(timeoutMs))
                throw new TimeoutException("main-thread action timed out after " + timeoutMs + "ms");
            if (job.Error != null) throw job.Error;
        }

        public static void CancelAll(Exception reason)
        {
            lock (_lock)
            {
                while (_queue.Count > 0)
                {
                    var req = _queue.Dequeue();
                    req.Error = reason;
                    req.Done.Set();
                }
                while (_actions.Count > 0)
                {
                    var job = _actions.Dequeue();
                    job.Error = reason;
                    job.Done.Set();
                }
                while (_flybys.Count > 0)
                {
                    var fb = _flybys.Dequeue();
                    fb.Error = reason;
                    fb.Done.Set();
                }
            }
        }

        private void Update()
        {
            // Re-assert the session pause every frame (runs even while the sim
            // is paused). Without this the game resumes the sim between agent
            // tool calls and the city declines on wall-clock time.
            PauseGuard.Enforce(ModRuntime.Threading);

            MainThreadAction action = null;
            lock (_lock) { if (_actions.Count > 0) action = _actions.Dequeue(); }
            if (action != null)
            {
                try { action.Work(); }
                catch (Exception e) { action.Error = e; }
                finally { action.Done.Set(); }
            }

            CaptureRequest req = null;
            lock (_lock) { if (_queue.Count > 0) req = _queue.Dequeue(); }
            if (req != null) StartCoroutine(Run(req));

            FlybyRequest fly = null;
            lock (_lock) { if (_flybys.Count > 0) fly = _flybys.Dequeue(); }
            if (fly != null) StartCoroutine(RunFlyby(fly));
        }

        private IEnumerator Run(CaptureRequest req)
        {
            // A milestone modal that popped up mid-step would otherwise be
            // burnt into the frame (fireworks + grey dim overlay). Close it
            // and give the close/fade animations (~0.7 s) time to finish.
            bool modalUp = false;
            try { modalUp = GameAccess.ForcedPaused() || ColossalFramework.UI.UIView.HasModalInput(); }
            catch { }
            if (modalUp)
            {
                try { GameAccess.ClearModalNow(); } catch { }
                yield return new WaitForSecondsRealtime(1f);
            }

            CameraController cc = null;
            bool prevFree = false;
            InfoManager im = null;
            InfoManager.InfoMode prevMode = InfoManager.InfoMode.None;
            InfoManager.SubInfoMode prevSub = InfoManager.SubInfoMode.Default;
            bool trafficOn = string.Equals(req.InfoView, "traffic", StringComparison.OrdinalIgnoreCase);
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                cc.m_freeCamera = true;
                var pos = new Vector3(req.X, 0f, req.Z);
                var angle = new Vector2(req.Yaw, req.Pitch);
                cc.m_targetPosition = pos; cc.m_currentPosition = pos;
                cc.m_targetSize = req.Size; cc.m_currentSize = req.Size;
                cc.m_targetAngle = angle; cc.m_currentAngle = angle;
                if (trafficOn)
                {
                    im = ColossalFramework.Singleton<InfoManager>.instance;
                    prevMode = im.CurrentMode; prevSub = im.CurrentSubMode;
                    im.SetCurrentMode(InfoManager.InfoMode.Traffic, InfoManager.SubInfoMode.Default);
                }
            }
            catch (Exception e) { req.Error = e; req.Done.Set(); yield break; }

            // End-of-frame waits so the moved camera renders; the longer wait when
            // the info view is on lets its colour fade settle.
            yield return new WaitForEndOfFrame();
            yield return new WaitForEndOfFrame();
            if (trafficOn) yield return new WaitForSecondsRealtime(0.5f);

            try
            {
                var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGB24, false);
                try
                {
                    tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                    tex.Apply();
                    req.Png = tex.EncodeToPNG();
                }
                finally
                {
                    UnityEngine.Object.Destroy(tex);
                }
            }
            catch (Exception e) { req.Error = e; }
            finally
            {
                if (im != null) try { im.SetCurrentMode(prevMode, prevSub); } catch { }
                if (cc != null) cc.m_freeCamera = prevFree;
                req.Done.Set();
            }
        }

        private IEnumerator RunFlyby(FlybyRequest req)
        {
            if (req.Keyframes == null || req.Keyframes.Length < 2) { req.Done.Set(); yield break; }
            var xs = new Vector2[req.Keyframes.Length];
            for (int i = 0; i < req.Keyframes.Length; i++)
                xs[i] = new Vector2(req.Keyframes[i].X, req.Keyframes[i].Z);
            int total = Mathf.Max(2, Mathf.RoundToInt(req.DurationS * req.CaptureFps));
            float interval = 1f / Mathf.Max(1, req.CaptureFps);

            var t = ModRuntime.Threading;
            CameraController cc = null;
            bool prevFree = false;
            bool prevPaused = t != null && t.simulationPaused;
            int prevSpeed = t != null ? t.simulationSpeed : 1;
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                cc.m_freeCamera = true;
                // Filming needs the sim running at speed 1; suspend the pause
                // guard so it doesn't re-pause mid-flyby, and re-arm it on every
                // exit path below.
                PauseGuard.Suspended = true;
                if (t != null) { t.simulationPaused = false; t.simulationSpeed = 1; }
                try { System.IO.Directory.CreateDirectory(req.OutDir); } catch { }
            }
            catch (Exception e)
            {
                if (cc != null) cc.m_freeCamera = prevFree;
                PauseGuard.Suspended = false;
                req.Error = e;
                req.Done.Set();
                yield break;
            }

            int frame = 0;
            for (int i = 0; i < total; i++)
            {
                float u = (float)i / (total - 1);
                Vector2 pos2 = FlybyMath.Sample(xs, u);
                float fk = u * (req.Keyframes.Length - 1);
                int k = Mathf.Min((int)fk, req.Keyframes.Length - 2);
                float kt = fk - k;
                var a = req.Keyframes[k];
                var b = req.Keyframes[Mathf.Min(k + 1, req.Keyframes.Length - 1)];
                float yaw = Mathf.Lerp(a.Yaw, b.Yaw, kt);
                float pitch = Mathf.Lerp(a.Pitch, b.Pitch, kt);
                float size = Mathf.Lerp(a.Size, b.Size, kt);

                Exception err = null;
                try
                {
                    var p = new Vector3(pos2.x, 0f, pos2.y);
                    cc.m_targetPosition = p; cc.m_currentPosition = p;
                    cc.m_targetSize = size; cc.m_currentSize = size;
                    cc.m_targetAngle = new Vector2(yaw, pitch); cc.m_currentAngle = new Vector2(yaw, pitch);
                }
                catch (Exception e) { err = e; }
                if (err != null) { req.Error = err; break; }

                yield return new WaitForEndOfFrame();

                try
                {
                    var tex = new Texture2D(Screen.width, Screen.height, TextureFormat.RGB24, false);
                    try
                    {
                        tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                        tex.Apply();
                        byte[] png = tex.EncodeToPNG();
                        frame++;
                        System.IO.File.WriteAllBytes(System.IO.Path.Combine(req.OutDir, frame.ToString("D5") + ".png"), png);
                    }
                    finally { UnityEngine.Object.Destroy(tex); }
                }
                catch (Exception e) { req.Error = e; break; }

                yield return new WaitForSecondsRealtime(interval);
            }

            if (cc != null) cc.m_freeCamera = prevFree;
            if (t != null) { t.simulationPaused = prevPaused; t.simulationSpeed = prevSpeed; }
            PauseGuard.Suspended = false;
            req.Done.Set();
        }
    }
}

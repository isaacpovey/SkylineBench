using System;
using System.Collections;
using System.Collections.Generic;
using System.Threading;
using UnityEngine;

namespace SkylineBench.Bridge
{
    public sealed class CaptureRequest
    {
        public float X, Z, Size;
        public bool TopDown;
        public byte[] Png;
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
        private static readonly object _lock = new object();

        public static byte[] Capture(float x, float z, float size, bool topDown, int timeoutMs)
        {
            var req = new CaptureRequest { X = x, Z = z, Size = size, TopDown = topDown };
            lock (_lock) { _queue.Enqueue(req); }
            if (!req.Done.WaitOne(timeoutMs))
                throw new TimeoutException("screenshot capture timed out after " + timeoutMs + "ms");
            if (req.Error != null) throw req.Error;
            return req.Png;
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
            }
        }

        private void Update()
        {
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
            try
            {
                cc = ToolsModifierControl.cameraController;
                prevFree = cc.m_freeCamera;
                // Free camera hides the game UI chrome so frames are clean.
                cc.m_freeCamera = true;
                var pos = new Vector3(req.X, 0f, req.Z);
                var angle = req.TopDown ? new Vector2(0f, 90f) : new Vector2(0f, 45f);
                // Setting target AND current skips the easing animation.
                cc.m_targetPosition = pos; cc.m_currentPosition = pos;
                cc.m_targetSize = req.Size; cc.m_currentSize = req.Size;
                cc.m_targetAngle = angle; cc.m_currentAngle = angle;
            }
            catch (Exception e) { req.Error = e; req.Done.Set(); yield break; }

            // Two end-of-frame waits so the moved camera actually renders.
            yield return new WaitForEndOfFrame();
            yield return new WaitForEndOfFrame();

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
                if (cc != null) cc.m_freeCamera = prevFree;
                req.Done.Set();
            }
        }
    }
}

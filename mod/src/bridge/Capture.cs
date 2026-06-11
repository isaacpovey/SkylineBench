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

    /// <summary>Runs screenshot captures on Unity's main thread. The HTTP
    /// thread enqueues a request and blocks on Done; Update() drains the queue
    /// and runs one coroutine per request (the sim is paused between agent
    /// steps, so requests never race game mutations).</summary>
    public sealed class CaptureBehaviour : MonoBehaviour
    {
        private static readonly Queue<CaptureRequest> _queue = new Queue<CaptureRequest>();
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

        private void Update()
        {
            CaptureRequest req = null;
            lock (_lock) { if (_queue.Count > 0) req = _queue.Dequeue(); }
            if (req != null) StartCoroutine(Run(req));
        }

        private IEnumerator Run(CaptureRequest req)
        {
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
                tex.ReadPixels(new Rect(0f, 0f, Screen.width, Screen.height), 0, 0);
                tex.Apply();
                req.Png = tex.EncodeToPNG();
                UnityEngine.Object.Destroy(tex);
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

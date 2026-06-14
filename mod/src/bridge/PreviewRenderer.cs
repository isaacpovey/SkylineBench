using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    /// <summary>An IRenderableManager that draws one or more proposed roads as a
    /// ghost each frame via the game's own CreateNode(test:true, visualize:true)
    /// path. test:true commits NOTHING — there is no segment to roll back.
    /// Registered once (no Unregister API exists); gated by Active.</summary>
    public sealed class PreviewRenderer : IRenderableManager
    {
        public struct Ghost { public NetInfo Prefab; public NetTool.ControlPoint A, Mid, B; }

        public static volatile bool Active;
        private static readonly List<Ghost> _ghosts = new List<Ghost>();
        private static readonly object _lock = new object();
        private static bool _registered;

        public static void SetGhosts(List<Ghost> ghosts)
        {
            lock (_lock) { _ghosts.Clear(); _ghosts.AddRange(ghosts); }
        }

        public static void Ensure()
        {
            if (_registered) return;
            RenderManager.RegisterRenderableManager(new PreviewRenderer());
            _registered = true;
        }

        /// <summary>Build a ghost (start/mid/end control points) from world XZ +
        /// per-endpoint elevation. Pure setup; no mutation.</summary>
        public static Ghost MakeGhost(NetInfo prefab, float sx, float sz, float ex, float ez, float fromElev, float toElev)
        {
            var tm = Singleton<TerrainManager>.instance;
            var sXZ = new Vector3(sx, 0f, sz);
            var eXZ = new Vector3(ex, 0f, ez);
            Vector3 dir = VectorUtils.NormalizeXZ(eXZ - sXZ);
            var a = new Vector3(sx, tm.SampleDetailHeight(sXZ) + fromElev, sz);
            var b = new Vector3(ex, tm.SampleDetailHeight(eXZ) + toElev, ez);
            return new Ghost
            {
                Prefab = prefab,
                A = Cp(a, dir, fromElev), Mid = Cp((a + b) * 0.5f, dir, (fromElev + toElev) * 0.5f), B = Cp(b, dir, toElev),
            };
        }

        private static NetTool.ControlPoint Cp(Vector3 pos, Vector3 dir, float elev)
        {
            return new NetTool.ControlPoint { m_position = pos, m_direction = dir, m_node = 0, m_segment = 0, m_elevation = elev, m_outside = false };
        }

        public string GetName() { return "SkylineBenchPreview"; }
        public DrawCallData GetDrawCallData() { return default(DrawCallData); }
        public void CheckReferences() { }
        public void InitRenderData() { }
        public bool CalculateGroupData(int groupX, int groupZ, int layer, ref int vertexCount, ref int triangleCount, ref int objectCount, ref RenderGroup.VertexArrays vertexArrays) { return false; }
        public void PopulateGroupData(int groupX, int groupZ, int layer, ref int vertexIndex, ref int triangleIndex, Vector3 groupPosition, RenderGroup.MeshData data, ref Vector3 min, ref Vector3 max, ref float maxRenderDistance, ref float maxInstanceDistance, ref bool requireSurfaceMaps) { }
        public void BeginRendering(RenderManager.CameraInfo cameraInfo) { }
        public void BeginOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void UndergroundOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void EndOverlay(RenderManager.CameraInfo cameraInfo) { }

        public void EndRendering(RenderManager.CameraInfo cameraInfo)
        {
            if (!Active) return;
            List<Ghost> snapshot;
            lock (_lock) { snapshot = new List<Ghost>(_ghosts); }
            foreach (var g in snapshot)
            {
                if (g.Prefab == null) continue;
                try
                {
                    ushort node, segment; int cost, prod;
                    NetTool.CreateNode(g.Prefab, g.A, g.Mid, g.B,
                        new FastList<NetTool.NodePosition>(), 1,
                        /*test*/ true, /*visualize*/ true, /*autoFix*/ true, /*needMoney*/ false,
                        false, false, 0, out node, out segment, out cost, out prod);
                }
                catch { }
            }
        }
    }
}

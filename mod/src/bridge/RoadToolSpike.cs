using System;
using System.Collections.Generic;
using System.Reflection;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// THROWAWAY feasibility spike (not part of the product). Drives the game's
    /// own NetTool to answer three questions before we commit a design:
    ///   1. validate — NetTool.CreateNode(test:true) returns the game's NATIVE
    ///      ToolErrors (collision with roads AND buildings, slope, height, area)
    ///      without building anything. Strictly richer than BuildValidator.
    ///   2. build    — NetTool.CreateNode(test:false) at an elevation actually
    ///      constructs a proper elevated/bridge segment (auto prefab-swap +
    ///      pillars). Confirms we can build overpasses the right way, and
    ///      underpins a build-then-rollback validate-with-screenshot.
    ///   3. ghost    — set NetTool as the active tool with our control points so
    ///      its in-game ghost (green/red) renders; we then capture a screenshot
    ///      to see whether the native preview lands in the framebuffer.
    /// Delete this file (and its route/handler) once the design is settled.
    /// </summary>
    public static class RoadToolSpike
    {
        private const int TimeoutMs = 8000;

        public sealed class SpikeReq
        {
            public string Action;   // "validate" | "build" | "ghost"
            public string Prefab;
            public float StartX, StartZ, EndX, EndZ;
            public float Elevation; // metres above terrain
        }

        public static SpikeReq Parse(JsonValue v)
        {
            return new SpikeReq
            {
                Action = v["action"].AsString(),
                Prefab = v["prefab"].AsString(),
                StartX = (float)v["start"]["x"].AsDouble(),
                StartZ = (float)v["start"]["z"].AsDouble(),
                EndX = (float)v["end"]["x"].AsDouble(),
                EndZ = (float)v["end"]["z"].AsDouble(),
                Elevation = (float)v["elevation"].AsDouble(),
            };
        }

        public static string Run(SpikeReq req)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return Err("INVALID_PREFAB", "no NetInfo named '" + req.Prefab + "'");

            if (req.Action == "ghost")
                return Ghost(prefab, req);

            if (req.Action == "preview" || req.Action == "preview_off")
                return Preview(prefab, req);

            bool test = req.Action != "build";
            return SimThread.Run<string>(delegate
            {
                ushort node, segment; int cost, prod;
                var cps = MakeControlPoints(prefab, req);
                ToolBase.ToolErrors err = NetTool.CreateNode(
                    prefab, cps[0], cps[1], cps[2],
                    new FastList<NetTool.NodePosition>(), 1,
                    test, /*visualize*/ false, /*autoFix*/ true, /*needMoney*/ false,
                    /*invert*/ false, /*switchDir*/ false, /*relocateBuildingID*/ 0,
                    out node, out segment, out cost, out prod);

                string builtPrefab = null;
                if (!test && segment != 0)
                {
                    var nm = Singleton<NetManager>.instance;
                    var info = nm.m_segments.m_buffer[segment].Info;
                    builtPrefab = info != null ? info.name : null;
                }

                var w = new JsonWriter();
                w.BeginObject()
                    .Name("ok").Value(err == ToolBase.ToolErrors.None)
                    .Name("action").Value(req.Action)
                    .Name("errors").BeginArray();
                foreach (var e in ErrorNames(err)) w.Value(e);
                w.EndArray()
                    .Name("error_bits").Value((long)(ulong)err)
                    .Name("requested_prefab").Value(req.Prefab)
                    .Name("built_prefab").Value(builtPrefab) // confirms elevated/bridge variant swap
                    .Name("created_node").Value(node)
                    .Name("created_segment").Value(segment)
                    .Name("cost").Value(cost)
                    .Name("elevation_m").Value((double)req.Elevation)
                 .EndObject();
                return w.ToString();
            }, TimeoutMs);
        }

        /// <summary>Build three control points (start, midpoint, end) for one
        /// straight segment at the requested elevation above terrain.</summary>
        private static NetTool.ControlPoint[] MakeControlPoints(NetInfo prefab, SpikeReq req)
        {
            var tm = Singleton<TerrainManager>.instance;
            var startXZ = new Vector3(req.StartX, 0f, req.StartZ);
            var endXZ = new Vector3(req.EndX, 0f, req.EndZ);
            Vector3 dir = VectorUtils.NormalizeXZ(endXZ - startXZ);

            var start = new Vector3(req.StartX, tm.SampleDetailHeight(startXZ) + req.Elevation, req.StartZ);
            var end = new Vector3(req.EndX, tm.SampleDetailHeight(endXZ) + req.Elevation, req.EndZ);
            var mid = (start + end) * 0.5f;

            return new[]
            {
                Cp(start, dir, req.Elevation),
                Cp(mid, dir, req.Elevation),
                Cp(end, dir, req.Elevation),
            };
        }

        private static NetTool.ControlPoint Cp(Vector3 pos, Vector3 dir, float elevation)
        {
            return new NetTool.ControlPoint
            {
                m_position = pos,
                m_direction = dir,
                m_node = 0,
                m_segment = 0,
                m_elevation = elevation,
                m_outside = false,
            };
        }

        /// <summary>Best-effort: make NetTool the active tool and inject our
        /// control points + elevation so its native ghost renders. Then the
        /// caller hits POST /screenshot to see if the green/red preview is in
        /// the captured frame. Heavily guarded — must never crash the game.</summary>
        private static string Ghost(NetInfo prefab, SpikeReq req)
        {
            Exception failure = null;
            ToolBase.ToolErrors reported = ToolBase.ToolErrors.Pending;
            CaptureBehaviour.RunOnMain(delegate
            {
                try
                {
                    var tool = ToolsModifierControl.SetTool<NetTool>();
                    tool.m_prefab = prefab;
                    tool.m_mode = NetTool.Mode.Straight;
                    var cps = MakeControlPoints(prefab, req);
                    SetPrivate(tool, "m_elevation", Mathf.RoundToInt(req.Elevation));
                    SetPrivate(tool, "m_controlPoints", cps);
                    SetPrivate(tool, "m_cachedControlPoints", cps);
                    SetPrivate(tool, "m_controlPointCount", 1);
                    SetPrivate(tool, "m_cachedControlPointCount", 1);
                    var errField = typeof(NetTool).GetField("m_buildErrors", BindingFlags.NonPublic | BindingFlags.Instance);
                    if (errField != null) reported = (ToolBase.ToolErrors)errField.GetValue(tool);
                }
                catch (Exception e) { failure = e; }
            }, TimeoutMs);

            var w = new JsonWriter();
            w.BeginObject()
                .Name("ok").Value(failure == null)
                .Name("action").Value("ghost")
                .Name("note").Value("NetTool set active with injected control points; call POST /screenshot to check whether the ghost rendered")
                .Name("build_errors").BeginArray();
            foreach (var e in ErrorNames(reported)) w.Value(e);
            w.EndArray()
                .Name("error").Value(failure != null ? failure.Message : null)
             .EndObject();
            return w.ToString();
        }

        /// <summary>Non-mutating preview: register a renderable manager that
        /// draws the proposed road via CreateNode(test:true, visualize:true)
        /// every rendered frame. test:true builds nothing — there is no segment
        /// to cancel or roll back. The caller then hits POST /screenshot to see
        /// the ghost; "preview_off" stops drawing.</summary>
        private static string Preview(NetInfo prefab, SpikeReq req)
        {
            Exception failure = null;
            CaptureBehaviour.RunOnMain(delegate
            {
                try
                {
                    if (req.Action == "preview_off")
                    {
                        SpikePreviewRenderer.Active = false;
                        return;
                    }
                    var cps = MakeControlPoints(prefab, req);
                    SpikePreviewRenderer.Set(prefab, cps[0], cps[1], cps[2]);
                    SpikePreviewRenderer.Ensure();
                    SpikePreviewRenderer.Active = true;
                }
                catch (Exception e) { failure = e; }
            }, TimeoutMs);

            var w = new JsonWriter();
            w.BeginObject()
                .Name("ok").Value(failure == null)
                .Name("action").Value(req.Action)
                .Name("active").Value(SpikePreviewRenderer.Active)
                .Name("note").Value("non-mutating preview; call POST /screenshot to view, then preview_off to clear")
                .Name("error").Value(failure != null ? failure.Message : null)
             .EndObject();
            return w.ToString();
        }

        private static void SetPrivate(object target, string field, object value)
        {
            var f = target.GetType().GetField(field, BindingFlags.NonPublic | BindingFlags.Instance);
            if (f == null) throw new Exception("field not found: " + field);
            f.SetValue(target, value);
        }

        private static List<string> ErrorNames(ToolBase.ToolErrors err)
        {
            var names = new List<string>();
            if (err == ToolBase.ToolErrors.None) return names;
            foreach (ToolBase.ToolErrors flag in Enum.GetValues(typeof(ToolBase.ToolErrors)))
            {
                if (flag != ToolBase.ToolErrors.None && (err & flag) == flag)
                    names.Add(flag.ToString());
            }
            return names;
        }

        private static string Err(string code, string message)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(false).Name("reason").Value(code).Name("message").Value(message).EndObject();
            return w.ToString();
        }
    }

    /// <summary>THROWAWAY spike: an IRenderableManager that draws a single
    /// proposed road as a ghost each frame, using the game's own
    /// CreateNode(test:true, visualize:true) preview path. Registered once;
    /// gated by Active. Nothing is ever committed.
    /// Renamed SpikePreviewRenderer to avoid conflict with the production
    /// PreviewRenderer in PreviewRenderer.cs.</summary>
    public sealed class SpikePreviewRenderer : IRenderableManager
    {
        public static volatile bool Active;
        private static NetInfo _prefab;
        private static NetTool.ControlPoint _cp0, _cp1, _cp2;
        private static bool _registered;

        public static void Set(NetInfo prefab, NetTool.ControlPoint a, NetTool.ControlPoint b, NetTool.ControlPoint c)
        {
            _prefab = prefab; _cp0 = a; _cp1 = b; _cp2 = c;
        }

        public static void Ensure()
        {
            if (_registered) return;
            RenderManager.RegisterRenderableManager(new SpikePreviewRenderer());
            _registered = true;
        }

        public string GetName() { return "SkylineBenchSpikePreview"; }
        public DrawCallData GetDrawCallData() { return default(DrawCallData); }
        public void CheckReferences() { }
        public void InitRenderData() { }
        public bool CalculateGroupData(int groupX, int groupZ, int layer, ref int vertexCount, ref int triangleCount, ref int objectCount, ref RenderGroup.VertexArrays vertexArrays) { return false; }
        public void PopulateGroupData(int groupX, int groupZ, int layer, ref int vertexIndex, ref int triangleIndex, UnityEngine.Vector3 groupPosition, RenderGroup.MeshData data, ref UnityEngine.Vector3 min, ref UnityEngine.Vector3 max, ref float maxRenderDistance, ref float maxInstanceDistance, ref bool requireSurfaceMaps) { }
        public void BeginRendering(RenderManager.CameraInfo cameraInfo) { }
        public void BeginOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void UndergroundOverlay(RenderManager.CameraInfo cameraInfo) { }
        public void EndOverlay(RenderManager.CameraInfo cameraInfo) { }

        public void EndRendering(RenderManager.CameraInfo cameraInfo)
        {
            if (!Active || _prefab == null) return;
            try
            {
                ushort node, segment; int cost, prod;
                NetTool.CreateNode(_prefab, _cp0, _cp1, _cp2,
                    new FastList<NetTool.NodePosition>(), 1,
                    /*test*/ true, /*visualize*/ true, /*autoFix*/ true, /*needMoney*/ false,
                    /*invert*/ false, /*switchDir*/ false, /*relocateBuildingID*/ 0,
                    out node, out segment, out cost, out prod);
            }
            catch { }
        }
    }
}

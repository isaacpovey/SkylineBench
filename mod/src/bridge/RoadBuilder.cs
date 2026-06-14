using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    /// <summary>Builds and validates roads through the game's own NetTool, so
    /// elevation auto-selects the elevated/bridge prefab variant (with pillars)
    /// and validation uses the native ToolErrors (collisions vs roads AND
    /// buildings, water, slope, height, area). Replaces the hand-rolled
    /// BuildValidator + raw CreateSegment path. Must run on the simulation
    /// thread (build) — callers wrap in SimThread.Run.</summary>
    public static class RoadBuilder
    {
        private const float SnapToleranceM = 8f;
        private const float SnapHeightToleranceM = 4f;
        private const int MaxSegments = 1; // broker pre-splits spans under the segment cap

        public static ActionResultDto Build(BuildRoadReq req) { return Run(req, false); }

        public static ActionResultDto Validate(BuildRoadReq req) { return Run(req, true); }

        private static ActionResultDto Run(BuildRoadReq req, bool test)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);

            var nm = Singleton<NetManager>.instance;
            var tm = Singleton<TerrainManager>.instance;

            var startXZ = new Vector3(req.StartX, 0f, req.StartZ);
            var endXZ = new Vector3(req.EndX, 0f, req.EndZ);
            float lenXZ = VectorUtils.LengthXZ(endXZ - startXZ);
            if (lenXZ < 0.001f) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            Vector3 dir = VectorUtils.NormalizeXZ(endXZ - startXZ);

            float startY = tm.SampleDetailHeight(startXZ) + req.FromElevation;
            float endY = tm.SampleDetailHeight(endXZ) + req.ToElevation;
            var startPos = new Vector3(req.StartX, startY, req.StartZ);
            var endPos = new Vector3(req.EndX, endY, req.EndZ);
            var midPos = (startPos + endPos) * 0.5f;

            var startCp = MakeCp(nm, startPos, dir, req.FromElevation, req.Snap);
            var endCp = MakeCp(nm, endPos, dir, req.ToElevation, req.Snap);
            var midCp = Cp(midPos, dir, (req.FromElevation + req.ToElevation) * 0.5f, 0);

            ushort node, segment; int cost, prod;
            ToolBase.ToolErrors err = NetTool.CreateNode(
                prefab, startCp, midCp, endCp,
                new FastList<NetTool.NodePosition>(), MaxSegments,
                test, /*visualize*/ false, /*autoFix*/ true, /*needMoney*/ false,
                /*invert*/ false, /*switchDir*/ false, /*relocateBuildingID*/ 0,
                out node, out segment, out cost, out prod);

            if (err != ToolBase.ToolErrors.None)
                return ActionResultDto.Fail(RoadErrors.Reason((ulong)err));

            var result = new ActionResultDto { Ok = true };
            if (!test && segment != 0)
            {
                result.CreatedSegments.Add(segment);
                // CreateNode returns only one `out node`, so classify the
                // segment's actual endpoints: a node we snapped a control point
                // to is reported snapped, the rest are newly created.
                var seg = nm.m_segments.m_buffer[segment];
                ClassifyNode(result, seg.m_startNode, startCp.m_node, endCp.m_node);
                ClassifyNode(result, seg.m_endNode, startCp.m_node, endCp.m_node);
            }
            result.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
            return result;
        }

        /// <summary>Record a segment endpoint as snapped (it matches a control
        /// point we snapped onto an existing node) or as newly created.</summary>
        private static void ClassifyNode(ActionResultDto result, ushort nodeId, ushort snapA, ushort snapB)
        {
            if (nodeId == 0) return;
            if (nodeId == snapA || nodeId == snapB)
            {
                if (!result.SnappedNodes.Contains(nodeId)) result.SnappedNodes.Add(nodeId);
            }
            else if (!result.CreatedNodes.Contains(nodeId))
            {
                result.CreatedNodes.Add(nodeId);
            }
        }

        /// <summary>Snap to the nearest existing node within tolerance whose
        /// height matches the requested elevation (so a ramp foot snaps to a
        /// ground node and its top to an elevated one); otherwise leave m_node=0
        /// so NetTool creates a fresh node at m_position.</summary>
        private static NetTool.ControlPoint MakeCp(NetManager nm, Vector3 pos, Vector3 dir, float elevation, bool snap)
        {
            ushort snapTo = snap ? NearestNode(nm, pos) : (ushort)0;
            var cp = Cp(pos, dir, elevation, snapTo);
            if (snapTo != 0) cp.m_position = nm.m_nodes.m_buffer[snapTo].m_position;
            return cp;
        }

        private static ushort NearestNode(NetManager nm, Vector3 p)
        {
            ushort best = 0; float bestD = SnapToleranceM;
            for (uint i = 1; i < nm.m_nodes.m_buffer.Length; i++)
            {
                var n = nm.m_nodes.m_buffer[i];
                if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                if (Mathf.Abs(n.m_position.y - p.y) > SnapHeightToleranceM) continue;
                float d = VectorUtils.LengthXZ(n.m_position - p);
                if (d <= bestD) { bestD = d; best = (ushort)i; }
            }
            return best;
        }

        private static NetTool.ControlPoint Cp(Vector3 pos, Vector3 dir, float elevation, ushort node)
        {
            return new NetTool.ControlPoint
            {
                m_position = pos, m_direction = dir,
                m_node = node, m_segment = 0,
                m_elevation = elevation, m_outside = false,
            };
        }
    }
}

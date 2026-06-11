using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Bridge
{
    public static class GameActions
    {
        private const int TimeoutMs = 8000;
        private const float SnapToleranceM = 8f;
        private const float MaxSegmentLengthM = 200f;

        public static ActionResultDto BuildRoad(BuildRoadReq req)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);
            var startPos = new Vector3(req.StartX, req.StartY, req.StartZ);
            var endPos = new Vector3(req.EndX, req.EndY, req.EndZ);
            float len = VectorUtils.LengthXZ(endPos - startPos);
            if (len < 0.001f) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            if (len > MaxSegmentLengthM) return ActionResultDto.Fail(ErrorCode.SegmentTooLong);

            return SimThread.Run<ActionResultDto>(delegate
            {
                var nm = Singleton<NetManager>.instance;
                var sm = Singleton<SimulationManager>.instance;
                var rand = new Randomizer(sm.m_currentBuildIndex);
                var result = new ActionResultDto { Ok = true };
                ushort startId, endId;
                if (!ResolveNode(nm, startPos, prefab, req.Snap, ref rand, sm, out startId, result)) return ActionResultDto.Fail(ErrorCode.Unknown);
                if (!ResolveNode(nm, endPos, prefab, req.Snap, ref rand, sm, out endId, result)) return ActionResultDto.Fail(ErrorCode.Unknown);
                Vector3 dir = VectorUtils.NormalizeXZ(endPos - startPos);
                ushort segId;
                bool ok = nm.CreateSegment(out segId, ref rand, prefab, startId, endId, dir, -dir, sm.m_currentBuildIndex, sm.m_currentBuildIndex, false);
                if (!ok) return ActionResultDto.Fail(ErrorCode.Collision);
                sm.m_currentBuildIndex += 2u;
                result.CreatedSegments.Add(segId);
                result.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(startPos, endPos, prefab.m_halfWidth);
                return result;
            }, TimeoutMs);
        }

        private static bool ResolveNode(NetManager nm, Vector3 p, NetInfo prefab, bool snap, ref Randomizer rand, SimulationManager sm, out ushort id, ActionResultDto result)
        {
            id = 0;
            if (snap)
            {
                ushort near = NearestNode(nm, p, SnapToleranceM);
                if (near != 0) { id = near; result.SnappedNodes.Add(near); return true; }
            }
            if (!nm.CreateNode(out id, ref rand, prefab, p, sm.m_currentBuildIndex)) return false;
            result.CreatedNodes.Add(id);
            return true;
        }

        private static ushort NearestNode(NetManager nm, Vector3 p, float tol)
        {
            ushort best = 0; float bestD = tol;
            for (uint i = 1; i < nm.m_nodes.m_buffer.Length; i++)
            {
                var n = nm.m_nodes.m_buffer[i];
                if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                float d = VectorUtils.LengthXZ(n.m_position - p);
                if (d <= bestD) { bestD = d; best = (ushort)i; }
            }
            return best;
        }

        public static ActionResultDto Bulldoze(BulldozeReq req)
        {
            return SimThread.Run<ActionResultDto>(delegate
            {
                switch (req.TargetType)
                {
                    case "segment":
                    {
                        var nm = Singleton<NetManager>.instance;
                        var seg = nm.m_segments.m_buffer[req.Id];
                        int fronting = -1;
                        if ((seg.m_flags & NetSegment.Flags.Created) != NetSegment.Flags.None && seg.Info != null)
                        {
                            Vector3 aPos = nm.m_nodes.m_buffer[seg.m_startNode].m_position;
                            Vector3 bPos = nm.m_nodes.m_buffer[seg.m_endNode].m_position;
                            fronting = (int)Frontage.CountZonedBuildingsNear(aPos, bPos, seg.Info.m_halfWidth);
                        }
                        nm.ReleaseSegment((ushort)req.Id, false);
                        var res = new ActionResultDto { Ok = true, ZonedBuildingsFronting = fronting };
                        res.Destroyed.Add(req.Id);
                        return res;
                    }
                    case "node": Singleton<NetManager>.instance.ReleaseNode((ushort)req.Id); break;
                    case "building": Singleton<BuildingManager>.instance.ReleaseBuilding((ushort)req.Id); break;
                    default: return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                }
                var r = new ActionResultDto { Ok = true }; r.Destroyed.Add(req.Id); return r;
            }, TimeoutMs);
        }

        public static ActionResultDto UpgradeRoad(UpgradeRoadReq req)
        {
            var prefab = Prefabs.FindRoad(req.Prefab);
            if (prefab == null) return ActionResultDto.Fail(ErrorCode.InvalidPrefab);
            return SimThread.Run<ActionResultDto>(delegate
            {
                var nm = Singleton<NetManager>.instance;
                var s = nm.m_segments.m_buffer[req.SegmentId];
                if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
                ushort startN = s.m_startNode, endN = s.m_endNode;
                Vector3 aPos = nm.m_nodes.m_buffer[startN].m_position;
                Vector3 bPos = nm.m_nodes.m_buffer[endN].m_position;
                Vector3 sd = s.m_startDirection, ed = s.m_endDirection;
                var sm = Singleton<SimulationManager>.instance;
                var rand = new Randomizer(sm.m_currentBuildIndex);
                nm.ReleaseSegment((ushort)req.SegmentId, true);
                ushort segId;
                bool ok = nm.CreateSegment(out segId, ref rand, prefab, startN, endN, sd, ed, sm.m_currentBuildIndex, sm.m_currentBuildIndex, false);
                if (!ok) return ActionResultDto.Fail(ErrorCode.Collision);
                sm.m_currentBuildIndex += 2u;
                var r = new ActionResultDto { Ok = true };
                r.CreatedSegments.Add(segId);
                r.Destroyed.Add(req.SegmentId);
                r.ZonedBuildingsFronting = (int)Frontage.CountZonedBuildingsNear(aPos, bPos, prefab.m_halfWidth);
                return r;
            }, TimeoutMs);
        }

        public static ActionResultDto SetZone(SetZoneReq req)
        {
            ItemClass.Zone zone = ParseZone(req.ZoneType);
            if (zone == ItemClass.Zone.None) return ActionResultDto.Fail(ErrorCode.InvalidArgs);
            return SimThread.Run<ActionResultDto>(delegate
            {
                ZoneWriter.SetZoneOverRect(req.MinX, req.MinZ, req.MaxX, req.MaxZ, zone);
                return new ActionResultDto { Ok = true };
            }, TimeoutMs);
        }

        private static ItemClass.Zone ParseZone(string z)
        {
            switch (z)
            {
                case "residential": case "residential_low": return ItemClass.Zone.ResidentialLow;
                case "residential_high": return ItemClass.Zone.ResidentialHigh;
                case "commercial": case "commercial_low": return ItemClass.Zone.CommercialLow;
                case "commercial_high": return ItemClass.Zone.CommercialHigh;
                case "industrial": return ItemClass.Zone.Industrial;
                case "office": return ItemClass.Zone.Office;
                default: return ItemClass.Zone.None;
            }
        }

        public static ClockStateDto Clock(ClockReq req)
        {
            var t = ModRuntime.Threading;
            if (t == null) return new ClockStateDto { Ok = false, Paused = false, Tick = 0 };
            switch (req.Op)
            {
                case "pause": t.simulationPaused = true; break;
                case "resume": t.simulationPaused = false; break;
                case "set-speed": t.simulationSpeed = Mathf.Clamp(req.Speed, 1, 3); break;
                case "step": Step(t, req.Ticks); break;
                default: return new ClockStateDto { Ok = false, Paused = t.simulationPaused, Tick = t.simulationTick };
            }
            return new ClockStateDto { Ok = true, Paused = t.simulationPaused, Tick = t.simulationTick };
        }

        private static void Step(ICities.IThreading t, int ticks)
        {
            if (ticks <= 0) return;
            uint target = t.simulationTick + (uint)ticks;
            bool wasPaused = t.simulationPaused;
            t.simulationPaused = false;
            int guard = 0;
            while (t.simulationTick < target && guard < 600000) { System.Threading.Thread.Sleep(1); guard++; }
            if (wasPaused) t.simulationPaused = true;
        }
    }
}

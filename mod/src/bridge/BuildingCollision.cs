using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    /// <summary>Lists the buildings a proposed road leg collides with by mirroring the
    /// query NetTool.CreateNode runs internally: build the same swept quad + [minY,maxY]
    /// band (CollisionCorridor) and call BuildingManager.OverlapQuad with a building
    /// bitmask out-param — the engine sets a bit per colliding building id — then read
    /// the set bits back to ids. Collision parameters come from the prefab's own m_netAI
    /// so the verdict matches the engine. Pillars are not build-time collision-tested, so
    /// they are intentionally ignored. Must run on the simulation thread (BuildingManager
    /// read). Intended to be verified in-game; not unit-tested (the broker mock has no
    /// buildings). The caller must pass the elevation-resolved NetInfo variant (e.g. via
    /// NetAI.GetInfo) so that m_minHeight/m_maxHeight and collision params reflect the
    /// actual elevated/bridge/tunnel prefab rather than the base ground prefab.</summary>
    public static class BuildingCollision
    {
        public static List<uint> Find(NetInfo prefab, Vector3 startPos, Vector3 endPos)
        {
            var hits = Describe(prefab, startPos, endPos);
            var ids = new List<uint>(hits.Count);
            for (int i = 0; i < hits.Count; i++) ids.Add(hits[i].Id);
            return ids;
        }

        /// <summary>Rich collision hits plus a combined lateral offset. Empty list
        /// when nothing overlaps / prefab is invalid.</summary>
        public static List<CollisionHitDto> Describe(NetInfo prefab, Vector3 startPos, Vector3 endPos)
        {
            var result = new List<CollisionHitDto>();
            if (prefab == null || prefab.m_netAI == null) return result;

            float halfWidth = prefab.m_netAI.GetCollisionHalfWidth();
            var input = new CorridorInput
            {
                Start = startPos,
                End = endPos,
                HalfWidth = halfWidth,
                MinHeight = prefab.m_minHeight,
                MaxHeight = prefab.m_maxHeight,
            };
            var corridor = CollisionCorridor.Compute(input);
            var quad = new Quad2 { a = corridor.A, b = corridor.B, c = corridor.C, d = corridor.D };

            var bm = Singleton<BuildingManager>.instance;
            int count = bm.m_buildings.m_buffer.Length;
            var mask = new ulong[(count + 63) / 64];
            // ignoreBuilding/ignoreNode1/ignoreNode2 are 0 intentionally: CreateNode
            // would pass real ids here, but passing 0 keeps this a superset query —
            // feedback may over-list, never under-list.
            bm.OverlapQuad(
                quad, corridor.MinY, corridor.MaxY,
                prefab.m_netAI.GetCollisionType(), prefab.m_netAI.GetCollisionLayers(),
                /*ignoreBuilding*/ (ushort)0, /*ignoreNode1*/ (ushort)0, /*ignoreNode2*/ (ushort)0,
                mask);

            for (uint id = 1; id < (uint)count; id++)
            {
                if ((mask[id >> 6] & (1UL << (int)(id & 0x3f))) == 0UL) continue;
                var b = bm.m_buildings.m_buffer[id];
                var info = b.Info;
                string category = GameReads.Category(info);
                float w = info != null ? info.m_cellWidth * 8f : 0f;
                float l = info != null ? info.m_cellLength * 8f : 0f;
                var obstacle = new Obstacle
                {
                    Position = b.m_position,
                    FootprintWidth = w,
                    FootprintLength = l,
                };
                Vector2 offset = CollisionCorridor.LateralOffset(input, obstacle);
                result.Add(new CollisionHitDto
                {
                    Id = id,
                    Kind = "building",
                    Category = category,
                    X = b.m_position.x, Y = b.m_position.y, Z = b.m_position.z,
                    FootprintWidth = w, FootprintLength = l,
                    CanBulldoze = CollisionCorridor.IsZonedCategory(category),
                    OffsetX = offset.x, OffsetZ = offset.y,
                });
            }
            return result;
        }

        public static SuggestedOffsetDto CombinedOffset(NetInfo prefab, Vector3 startPos, Vector3 endPos, List<CollisionHitDto> hits)
        {
            if (hits == null || hits.Count == 0) return null;
            float halfWidth = (prefab != null && prefab.m_netAI != null)
                ? prefab.m_netAI.GetCollisionHalfWidth() : 8f;
            var input = new CorridorInput
            {
                Start = startPos, End = endPos, HalfWidth = halfWidth,
                MinHeight = 0f, MaxHeight = 0f,
            };
            var obstacles = new Obstacle[hits.Count];
            for (int i = 0; i < hits.Count; i++)
            {
                obstacles[i] = new Obstacle
                {
                    Position = new Vector3(hits[i].X, hits[i].Y, hits[i].Z),
                    FootprintWidth = hits[i].FootprintWidth,
                    FootprintLength = hits[i].FootprintLength,
                };
            }
            OffsetAdvice advice = CollisionCorridor.CombinedOffset(input, obstacles);
            return new SuggestedOffsetDto { X = advice.X, Z = advice.Z, ClearsAll = advice.ClearsAll };
        }
    }
}

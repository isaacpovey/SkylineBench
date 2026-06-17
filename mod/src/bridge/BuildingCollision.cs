using System.Collections.Generic;
using ColossalFramework;
using ColossalFramework.Math;
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Lists the buildings a proposed road leg collides with by mirroring the
    /// query NetTool.CreateNode runs internally: build the same swept quad + [minY,maxY]
    /// band (CollisionCorridor) and call BuildingManager.OverlapQuad with a building
    /// bitmask out-param — the engine sets a bit per colliding building id — then read
    /// the set bits back to ids. Collision parameters come from the prefab's own m_netAI
    /// so the verdict matches the engine. Pillars are not build-time collision-tested, so
    /// they are intentionally ignored. Must run on the simulation thread (BuildingManager
    /// read). Verified in-game, not unit-tested (the broker mock has no buildings).</summary>
    public static class BuildingCollision
    {
        public static List<uint> Find(NetInfo prefab, Vector3 startPos, Vector3 endPos)
        {
            var result = new List<uint>();
            if (prefab == null || prefab.m_netAI == null) return result;

            var corridor = CollisionCorridor.Compute(new CorridorInput
            {
                Start = startPos,
                End = endPos,
                HalfWidth = prefab.m_netAI.GetCollisionHalfWidth(),
                MinHeight = prefab.m_minHeight,
                MaxHeight = prefab.m_maxHeight,
            });
            var quad = new Quad2 { a = corridor.A, b = corridor.B, c = corridor.C, d = corridor.D };

            var bm = Singleton<BuildingManager>.instance;
            int count = bm.m_buildings.m_buffer.Length;
            var mask = new ulong[(count + 63) / 64];
            bm.OverlapQuad(
                quad, corridor.MinY, corridor.MaxY,
                prefab.m_netAI.GetCollisionType(), prefab.m_netAI.GetCollisionLayers(),
                /*ignoreBuilding*/ (ushort)0, /*ignoreNode1*/ (ushort)0, /*ignoreNode2*/ (ushort)0,
                mask);

            for (uint id = 1; id < count; id++)
            {
                if ((mask[id >> 6] & (1UL << (int)(id & 0x3f))) != 0UL) result.Add(id);
            }
            return result;
        }
    }
}

using ColossalFramework;
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Counts zoned (RCIO) buildings that front a road span. CS1 zone blocks
    /// extend 4 cells (32 m) from the road edge, so a building whose centre is
    /// within halfWidth + 36 m of the span's centreline is treated as fronting
    /// it. Geometric approximation: a parallel street closer than the corridor
    /// width can produce small over-counts — acceptable for a neutral
    /// informational field. Must run on the simulation thread.
    /// </summary>
    public static class Frontage
    {
        private const float ZoneDepthM = 32f;
        private const float MarginM = 4f;

        public static uint CountZonedBuildingsNear(Vector3 a, Vector3 b, float roadHalfWidth)
        {
            float corridor = roadHalfWidth + ZoneDepthM + MarginM;
            var bm = Singleton<BuildingManager>.instance;
            uint count = 0;
            for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
            {
                var bld = bm.m_buildings.m_buffer[i];
                if ((bld.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                if (!IsZoned(bld.Info)) continue;
                if (DistanceToSegmentXZ(bld.m_position, a, b) <= corridor) count++;
            }
            return count;
        }

        private static bool IsZoned(BuildingInfo info)
        {
            if (info == null || info.m_class == null) return false;
            switch (info.m_class.m_service)
            {
                case ItemClass.Service.Residential:
                case ItemClass.Service.Commercial:
                case ItemClass.Service.Industrial:
                case ItemClass.Service.Office:
                    return true;
                default:
                    return false;
            }
        }

        private static float DistanceToSegmentXZ(Vector3 p, Vector3 a, Vector3 b)
        {
            var ap = new Vector2(p.x - a.x, p.z - a.z);
            var ab = new Vector2(b.x - a.x, b.z - a.z);
            float len2 = ab.sqrMagnitude;
            float t = len2 > 0f ? Mathf.Clamp01(Vector2.Dot(ap, ab) / len2) : 0f;
            var closest = new Vector2(a.x + ab.x * t, a.z + ab.y * t);
            return Vector2.Distance(new Vector2(p.x, p.z), closest);
        }
    }
}

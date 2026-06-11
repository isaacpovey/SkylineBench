using ColossalFramework;
using UnityEngine;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Pre-creation placement checks for road builds. NetManager.CreateSegment
    /// is the low-level constructor and skips all NetTool validation, so
    /// without these checks builds the game UI refuses (through buildings, too
    /// steep, outside owned tiles) silently succeed. Explicit checks rather
    /// than NetTool's test mode keep vanilla rules we don't want (money,
    /// road-spacing) out of the benchmark. Must run on the simulation thread.
    /// </summary>
    public static class BuildValidator
    {
        private const int MaxReportedCollisions = 20;

        /// <summary>null when the placement is valid; otherwise a failure DTO
        /// with the normalized reason (plus colliding building ids).</summary>
        public static ActionResultDto Check(NetInfo prefab, Vector3 a, Vector3 b)
        {
            var am = Singleton<GameAreaManager>.instance;
            if (am.PointOutOfArea(a) || am.PointOutOfArea(b))
                return ActionResultDto.Fail(ErrorCode.OutOfArea);

            float lenXZ = ColossalFramework.Math.VectorUtils.LengthXZ(b - a);
            if (lenXZ > 0.001f && Mathf.Abs(b.y - a.y) / lenXZ > prefab.m_maxSlope)
                return ActionResultDto.Fail(ErrorCode.SlopeTooSteep);

            var colliding = CollidingBuildings(a, b, prefab.m_halfWidth);
            if (colliding.Count > 0)
            {
                var fail = ActionResultDto.Fail(ErrorCode.ObjectCollision);
                fail.CollidingBuildings = colliding;
                return fail;
            }
            return null;
        }

        private static System.Collections.Generic.List<uint> CollidingBuildings(Vector3 a, Vector3 b, float roadHalfWidth)
        {
            var hits = new System.Collections.Generic.List<uint>();
            var bm = Singleton<BuildingManager>.instance;
            for (uint i = 0; i < bm.m_buildings.m_buffer.Length && hits.Count < MaxReportedCollisions; i++)
            {
                var bld = bm.m_buildings.m_buffer[i];
                if ((bld.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                if (Intersects(a, b, roadHalfWidth, ref bld)) hits.Add(i);
            }
            return hits;
        }

        /// <summary>2-D capsule (road span widened by half-width) vs the
        /// building's rotated footprint rectangle.</summary>
        private static bool Intersects(Vector3 a, Vector3 b, float roadHalfWidth, ref Building bld)
        {
            var info = bld.Info;
            float halfW = (info != null ? info.m_cellWidth : 1) * 4f;
            float halfL = (info != null ? info.m_cellLength : 1) * 4f;
            float cos = Mathf.Cos(-bld.m_angle), sin = Mathf.Sin(-bld.m_angle);
            Vector2 la = ToLocal(a, bld.m_position, cos, sin);
            Vector2 lb = ToLocal(b, bld.m_position, cos, sin);
            return SegmentToRectDistance(la, lb, halfW, halfL) <= roadHalfWidth;
        }

        private static Vector2 ToLocal(Vector3 world, Vector3 centre, float cos, float sin)
        {
            float dx = world.x - centre.x, dz = world.z - centre.z;
            return new Vector2(dx * cos - dz * sin, dx * sin + dz * cos);
        }

        /// <summary>Distance from segment [a,b] to the axis-aligned rectangle
        /// |x| ≤ halfW, |y| ≤ halfL centred at the origin (0 when intersecting).</summary>
        private static float SegmentToRectDistance(Vector2 a, Vector2 b, float halfW, float halfL)
        {
            if (PointInRect(a, halfW, halfL) || PointInRect(b, halfW, halfL)) return 0f;
            Vector2[] corners =
            {
                new Vector2(-halfW, -halfL), new Vector2(halfW, -halfL),
                new Vector2(halfW, halfL), new Vector2(-halfW, halfL)
            };
            float best = float.MaxValue;
            for (int i = 0; i < 4; i++)
            {
                Vector2 c0 = corners[i], c1 = corners[(i + 1) % 4];
                if (SegmentsIntersect(a, b, c0, c1)) return 0f;
                best = Mathf.Min(best, PointToSegmentDistance(c0, a, b));
                best = Mathf.Min(best, PointToSegmentDistance(a, c0, c1));
                best = Mathf.Min(best, PointToSegmentDistance(b, c0, c1));
            }
            return best;
        }

        private static bool PointInRect(Vector2 p, float halfW, float halfL)
        {
            return Mathf.Abs(p.x) <= halfW && Mathf.Abs(p.y) <= halfL;
        }

        private static bool SegmentsIntersect(Vector2 p1, Vector2 p2, Vector2 q1, Vector2 q2)
        {
            float d1 = Cross(q2 - q1, p1 - q1), d2 = Cross(q2 - q1, p2 - q1);
            float d3 = Cross(p2 - p1, q1 - p1), d4 = Cross(p2 - p1, q2 - p1);
            return ((d1 > 0f) != (d2 > 0f)) && ((d3 > 0f) != (d4 > 0f));
        }

        private static float Cross(Vector2 u, Vector2 v) { return u.x * v.y - u.y * v.x; }

        private static float PointToSegmentDistance(Vector2 p, Vector2 a, Vector2 b)
        {
            var ab = b - a;
            float len2 = ab.sqrMagnitude;
            float t = len2 > 0f ? Mathf.Clamp01(Vector2.Dot(p - a, ab) / len2) : 0f;
            return Vector2.Distance(p, a + ab * t);
        }
    }
}

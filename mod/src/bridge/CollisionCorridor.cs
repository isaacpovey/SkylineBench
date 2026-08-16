using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Pure geometry for the swept collision corridor of a straight road
    /// leg: the XZ rectangle the road occupies plus the [MinY,MaxY] vertical band the
    /// engine tests buildings against. Mirrors NetTool.CreateNode's collision query
    /// (swept quad + segY+m_minHeight..segY+m_maxHeight). A plain rectangle is an
    /// intentional superset of CreateNode's bezier-shaped quad — it never reports fewer
    /// buildings than the engine. No Colossal types, so it is unit-testable. The broker
    /// pre-splits spans and the builder uses MaxSegments=1, so each leg is one straight
    /// segment. Intended to be verified in-game.</summary>
    public struct CorridorInput
    {
        public Vector3 Start;
        public Vector3 End;
        public float HalfWidth;
        public float MinHeight;   // NetInfo.m_minHeight (collision band relative to road surface)
        public float MaxHeight;   // NetInfo.m_maxHeight
    }

    public struct Corridor
    {
        public Vector2 A, B, C, D; // XZ rectangle corners
        public float MinY, MaxY;
    }

    /// <summary>A colliding footprint used to suggest a lateral reroute. Position is
    /// the building centre; footprints are metres (cell size × 8).</summary>
    public struct Obstacle
    {
        public Vector3 Position;
        public float FootprintWidth;
        public float FootprintLength;
    }

    /// <summary>A single XZ shift of the whole span that tries to clear obstacles.
    /// ClearsAll is false when buildings pinch the corridor from both sides, so no
    /// one lateral shift works — bulldoze zoned hits or raise elevation instead.</summary>
    public struct OffsetAdvice
    {
        public float X, Z;
        public bool ClearsAll;
    }

    public static class CollisionCorridor
    {
        /// <summary>Extra metres beyond the circumradius so a suggested shift actually
        /// clears instead of grazing the footprint.</summary>
        public const float ClearanceMarginM = 2f;

        public static Corridor Compute(CorridorInput input)
        {
            var s = new Vector2(input.Start.x, input.Start.z);
            var e = new Vector2(input.End.x, input.End.z);
            Vector2 along = e - s;
            float len = along.magnitude;
            Vector2 dir = len > 1e-4f ? along / len : new Vector2(1f, 0f);
            Vector2 perp = new Vector2(-dir.y, dir.x);
            // Pad the ends by a half-width so a building sitting exactly at an endpoint
            // is still caught (superset safety).
            Vector2 s0 = s - dir * input.HalfWidth;
            Vector2 e0 = e + dir * input.HalfWidth;
            Vector2 side = perp * input.HalfWidth;
            return new Corridor
            {
                A = s0 - side, B = e0 - side, C = e0 + side, D = s0 + side,
                MinY = Mathf.Min(input.Start.y, input.End.y) + input.MinHeight,
                MaxY = Mathf.Max(input.Start.y, input.End.y) + input.MaxHeight,
            };
        }

        /// <summary>Zoned RCIO categories are safe to bulldoze for road space;
        /// service/other must be routed around.</summary>
        public static bool IsZonedCategory(string category)
        {
            return category == "residential" || category == "commercial"
                || category == "industrial" || category == "office";
        }

        /// <summary>Lateral XZ shift of the whole span that moves the road away
        /// from this one obstacle. Zero if the centreline already clears it.</summary>
        public static Vector2 LateralOffset(CorridorInput road, Obstacle obstacle)
        {
            Vector2 perp;
            float signed, overlap;
            Analyze(road, obstacle, out perp, out signed, out overlap);
            if (overlap <= 0f) return Vector2.zero;
            float away = signed > 0f ? -1f : 1f;
            return perp * (away * overlap);
        }

        /// <summary>One span-wide shift that clears every obstacle if they all sit
        /// on the same side of the centreline. If they pinch from both sides,
        /// ClearsAll is false and the vector is the larger of the two demands.</summary>
        public static OffsetAdvice CombinedOffset(CorridorInput road, Obstacle[] obstacles)
        {
            Vector2 perp = Vector2.zero;
            float leftNeed = 0f, rightNeed = 0f;
            bool any = false;
            if (obstacles != null)
            {
                for (int i = 0; i < obstacles.Length; i++)
                {
                    Vector2 p;
                    float signed, overlap;
                    Analyze(road, obstacles[i], out p, out signed, out overlap);
                    if (!any) { perp = p; any = true; }
                    if (overlap <= 0f) continue;
                    if (signed > 0f) rightNeed = Mathf.Max(rightNeed, overlap);
                    else leftNeed = Mathf.Max(leftNeed, overlap);
                }
            }
            var advice = new OffsetAdvice { ClearsAll = true };
            if (leftNeed > 0f && rightNeed > 0f)
            {
                advice.ClearsAll = false;
                if (leftNeed >= rightNeed) { advice.X = perp.x * leftNeed; advice.Z = perp.y * leftNeed; }
                else { advice.X = -perp.x * rightNeed; advice.Z = -perp.y * rightNeed; }
            }
            else if (leftNeed > 0f) { advice.X = perp.x * leftNeed; advice.Z = perp.y * leftNeed; }
            else if (rightNeed > 0f) { advice.X = -perp.x * rightNeed; advice.Z = -perp.y * rightNeed; }
            return advice;
        }

        static void Analyze(CorridorInput road, Obstacle o, out Vector2 perp, out float signed, out float overlap)
        {
            var s = new Vector2(road.Start.x, road.Start.z);
            var e = new Vector2(road.End.x, road.End.z);
            Vector2 along = e - s;
            float len2 = along.sqrMagnitude;
            Vector2 dir = len2 > 1e-8f ? along / Mathf.Sqrt(len2) : new Vector2(1f, 0f);
            perp = new Vector2(-dir.y, dir.x);
            var p = new Vector2(o.Position.x, o.Position.z);
            float t = len2 > 1e-8f ? Mathf.Clamp01(Vector2.Dot(p - s, along) / len2) : 0f;
            Vector2 closest = s + along * t;
            Vector2 fromRoad = p - closest;
            float dist = fromRoad.magnitude;
            signed = Vector2.Dot(fromRoad, perp);
            float radius = 0.5f * Mathf.Sqrt(o.FootprintWidth * o.FootprintWidth + o.FootprintLength * o.FootprintLength);
            overlap = (road.HalfWidth + radius + ClearanceMarginM) - dist;
        }
    }
}

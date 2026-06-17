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
    /// segment.</summary>
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

    public static class CollisionCorridor
    {
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
    }
}

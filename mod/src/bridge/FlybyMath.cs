using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>Catmull-Rom sampling across a list of control points. `u` runs
    /// 0..1 over the whole path; endpoints are clamped (duplicated) so the curve
    /// passes through the first and last control points.</summary>
    public static class FlybyMath
    {
        public static Vector2 Sample(Vector2[] pts, float u)
        {
            if (pts.Length == 0) return Vector2.zero;
            if (pts.Length == 1) return pts[0];
            u = Mathf.Clamp01(u);
            int segments = pts.Length - 1;
            float scaled = u * segments;
            int i = Mathf.Min((int)scaled, segments - 1);
            float t = scaled - i;
            Vector2 p0 = pts[Mathf.Max(i - 1, 0)];
            Vector2 p1 = pts[i];
            Vector2 p2 = pts[i + 1];
            Vector2 p3 = pts[Mathf.Min(i + 2, pts.Length - 1)];
            return CatmullRom(p0, p1, p2, p3, t);
        }

        static Vector2 CatmullRom(Vector2 p0, Vector2 p1, Vector2 p2, Vector2 p3, float t)
        {
            float t2 = t * t;
            float t3 = t2 * t;
            return 0.5f * (
                (2f * p1) +
                (-p0 + p2) * t +
                (2f * p0 - 5f * p1 + 4f * p2 - p3) * t2 +
                (-p0 + 3f * p1 - 3f * p2 + p3) * t3);
        }
    }
}

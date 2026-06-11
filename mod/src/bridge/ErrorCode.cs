using ICities;

namespace SkylineBench.Bridge
{
    /// <summary>Normalized action failure reasons (spec §5). The HTTP layer returns these
    /// at HTTP 200 with {ok:false,reason}.</summary>
    public static class ErrorCode
    {
        public const string Collision = "COLLISION";
        public const string InsufficientFunds = "INSUFFICIENT_FUNDS";
        public const string OutOfBounds = "OUT_OF_BOUNDS";
        public const string InvalidPrefab = "INVALID_PREFAB";
        public const string SegmentTooLong = "SEGMENT_TOO_LONG";
        public const string InvalidArgs = "INVALID_ARGS";
        public const string Unknown = "UNKNOWN";
        public const string ObjectCollision = "OBJECT_COLLISION";
        public const string SlopeTooSteep = "SLOPE_TOO_STEEP";
        public const string OutOfArea = "OUT_OF_AREA";
        public const string TooManyConnections = "TOO_MANY_CONNECTIONS";
        public const string NetBufferFull = "NET_BUFFER_FULL";
    }

    public struct RoadInfo { public string Name; public long ConstructionCost; }

    public static class Prefabs
    {
        /// <summary>Find a NetInfo road prefab by exact name (e.g. "Basic Road"). null if absent.</summary>
        public static NetInfo FindRoad(string name)
        {
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                if (p != null && p.name == name) return p;
            }
            return null;
        }

        /// <summary>Road-service prefabs with their NetInfo construction cost.</summary>
        public static System.Collections.Generic.List<RoadInfo> Roads()
        {
            var list = new System.Collections.Generic.List<RoadInfo>();
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                if (p != null && p.name != null && p.m_class != null && p.m_class.m_service == ItemClass.Service.Road)
                {
                    // Vanilla road AIs derive from PlayerNetAI; for other modded AIs the
                    // cost is unknown, so emit 0 (entry still present) rather than skipping it.
                    var ai = p.m_netAI as PlayerNetAI;
                    list.Add(new RoadInfo { Name = p.name, ConstructionCost = ai != null ? ai.m_constructionCost : 0 });
                }
            }
            return list;
        }
    }
}

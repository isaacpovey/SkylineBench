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
    }

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

        /// <summary>Names of road-service prefabs (excludes rail/metro/pedestrian/canal/etc).</summary>
        public static System.Collections.Generic.List<string> RoadNames()
        {
            var list = new System.Collections.Generic.List<string>();
            int count = PrefabCollection<NetInfo>.PrefabCount();
            for (uint i = 0; i < count; i++)
            {
                var p = PrefabCollection<NetInfo>.GetPrefab(i);
                if (p != null && p.name != null && p.m_class != null && p.m_class.m_service == ItemClass.Service.Road)
                    list.Add(p.name);
            }
            return list;
        }
    }
}

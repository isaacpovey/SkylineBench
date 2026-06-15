using ICities;

namespace SkylineBench.Bridge
{
    public struct RoadInfo
    {
        public string Name;
        public long ConstructionCost;
        // Connectivity characteristics so the agent can tell a limited-access
        // highway (no local/side-road or building connections — converting a
        // connector to it strands the buildings and service depots it served)
        // from a zonable surface road.
        public bool AllowsZoning;
        public bool LimitedAccess;
        public bool OneWay;
        public int Lanes;
        public float HalfWidth;
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
                    var roadAi = p.m_netAI as RoadBaseAI;
                    var zoneAi = p.m_netAI as RoadAI;
                    int lanes = 0;
                    if (p.m_lanes != null)
                        foreach (var lane in p.m_lanes)
                            if (lane != null && (lane.m_laneType & NetInfo.LaneType.Vehicle) != NetInfo.LaneType.None)
                                lanes++;
                    list.Add(new RoadInfo
                    {
                        Name = p.name,
                        ConstructionCost = ai != null ? ai.m_constructionCost : 0,
                        // Limited-access (highway) and non-zonable roads cannot host buildings
                        // or local/side-road connections; m_enableZoning is false for them.
                        AllowsZoning = zoneAi != null && zoneAi.m_enableZoning,
                        LimitedAccess = roadAi != null && roadAi.m_highwayRules,
                        OneWay = p.m_hasForwardVehicleLanes ^ p.m_hasBackwardVehicleLanes,
                        Lanes = lanes,
                        HalfWidth = p.m_halfWidth,
                    });
                }
            }
            return list;
        }
    }
}

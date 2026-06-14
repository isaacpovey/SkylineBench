using ICities;

namespace SkylineBench.Bridge
{
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

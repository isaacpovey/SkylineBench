using SkylineBench.Dto;

namespace SkylineBench.Json
{
    /// <summary>Pure DTO → JSON, matching the broker's contract.rs wire shapes exactly.</summary>
    public static class Serialize
    {
        public static string Network(NetworkDto net)
        {
            var w = new JsonWriter();
            w.BeginObject();
            w.Name("nodes").BeginArray();
            foreach (var n in net.Nodes)
                w.BeginObject().Name("id").Value((long)n.Id).Name("x").Value(n.X).Name("y").Value(n.Y).Name("z").Value(n.Z).EndObject();
            w.EndArray();
            w.Name("segments").BeginArray();
            foreach (var s in net.Segments)
                w.BeginObject().Name("id").Value((long)s.Id).Name("start_node").Value((long)s.StartNode).Name("end_node").Value((long)s.EndNode)
                    .Name("prefab").Value(s.Prefab).Name("lanes").Value((long)s.Lanes).Name("length").Value(s.Length).EndObject();
            w.EndArray();
            w.EndObject();
            return w.ToString();
        }

        public static string Buildings(BuildingsDto b)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("buildings").BeginArray();
            foreach (var x in b.Buildings)
                w.BeginObject().Name("id").Value((long)x.Id).Name("prefab").Value(x.Prefab).Name("category").Value(x.Category)
                    .Name("x").Value(x.X).Name("y").Value(x.Y).Name("z").Value(x.Z)
                    .Name("footprint_width").Value(x.FootprintWidth).Name("footprint_length").Value(x.FootprintLength)
                    .Name("level").Value((long)x.Level).EndObject();
            w.EndArray().EndObject();
            return w.ToString();
        }

        public static string Zones(ZonesDto z)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("cells").BeginArray();
            foreach (var c in z.Cells)
                w.BeginObject().Name("x").Value(c.X).Name("z").Value(c.Z).Name("zone_type").Value(c.ZoneType).EndObject();
            w.EndArray().EndObject();
            return w.ToString();
        }

        public static string Metrics(MetricsDto m)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("tick").Value((long)m.Tick);
            w.Name("traffic").BeginObject().Name("flow_percent").Value(m.FlowPercent).Name("active_vehicles").Value((long)m.ActiveVehicles)
                .Name("segment_loads").BeginArray();
            foreach (var sl in m.SegmentLoads)
                w.BeginObject().Name("segment_id").Value((long)sl.SegmentId).Name("density").Value(sl.Density).EndObject();
            w.EndArray().EndObject();
            w.Name("economy").BeginObject().Name("balance").Value(m.Balance).Name("weekly_income").Value(m.WeeklyIncome)
                .Name("weekly_expenses").Value(m.WeeklyExpenses).Name("funds").Value(m.Funds).EndObject();
            w.Name("population").BeginObject().Name("total").Value((long)m.Population).Name("residential_demand").Value((long)m.ResidentialDemand)
                .Name("commercial_demand").Value((long)m.CommercialDemand).Name("workplace_demand").Value((long)m.WorkplaceDemand)
                .Name("employed").Value((long)m.Employed).EndObject();
            w.Name("services").BeginObject().Name("happiness").Value((long)m.Happiness).EndObject();
            w.EndObject();
            return w.ToString();
        }

        public static string Action(ActionResultDto r)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(r.Ok);
            if (r.Ok)
            {
                WriteUintArray(w, "created_nodes", r.CreatedNodes);
                WriteUintArray(w, "created_segments", r.CreatedSegments);
                WriteUintArray(w, "snapped_nodes", r.SnappedNodes);
                WriteUintArray(w, "destroyed", r.Destroyed);
            }
            else { w.Name("reason").Value(r.Reason); }
            w.EndObject();
            return w.ToString();
        }

        public static string Clock(ClockStateDto c)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(c.Ok).Name("paused").Value(c.Paused).Name("tick").Value((long)c.Tick).EndObject();
            return w.ToString();
        }

        public static string Load(LoadResultDto l)
        {
            var w = new JsonWriter();
            w.BeginObject().Name("ok").Value(l.Ok).Name("city_loaded").Value(l.CityLoaded).EndObject();
            return w.ToString();
        }

        private static void WriteUintArray(JsonWriter w, string name, System.Collections.Generic.List<uint> xs)
        {
            w.Name(name).BeginArray();
            foreach (var x in xs) w.Value((long)x);
            w.EndArray();
        }
    }
}

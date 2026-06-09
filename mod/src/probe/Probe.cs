using System;
using System.Reflection;
using ICities;
using UnityEngine;
using SkylineBench.Bridge;
using SkylineBench.Json;

namespace SkylineBench
{
    public static class Probe
    {
        public static string BuildDump()
        {
            var w = new JsonWriter();
            w.BeginObject();

            w.Name("http_listener_works").Value(true);

            w.Name("road_prefabs").BeginArray();
            try
            {
                int count = PrefabCollection<NetInfo>.PrefabCount();
                for (uint i = 0; i < count; i++)
                {
                    var p = PrefabCollection<NetInfo>.GetPrefab(i);
                    if (p != null && p.name != null) w.Value(p.name);
                }
            }
            catch (Exception e) { w.Value("ERROR: " + e.Message); }
            w.EndArray();

            w.Name("fields").BeginObject();
            ReportField(w, "VehicleManager.m_vehicleCount", typeof(VehicleManager), "m_vehicleCount");
            ReportField(w, "NetSegment.m_trafficDensity", typeof(NetSegment), "m_trafficDensity");
            ReportField(w, "BuildingManager.m_buildings", typeof(BuildingManager), "m_buildings");
            ReportField(w, "EconomyManager", typeof(EconomyManager), null);
            ReportField(w, "ZoneManager.m_actualResidentialDemand", typeof(ZoneManager), "m_actualResidentialDemand");
            ReportField(w, "ZoneManager.m_actualCommercialDemand", typeof(ZoneManager), "m_actualCommercialDemand");
            ReportField(w, "ZoneManager.m_actualWorkplaceDemand", typeof(ZoneManager), "m_actualWorkplaceDemand");
            w.EndObject();

            w.Name("member_dump").BeginObject();
            DumpMembers(w, "EconomyManager", typeof(EconomyManager));
            DumpMembers(w, "ZoneManager", typeof(ZoneManager));
            DumpMembers(w, "BuildingManager", typeof(BuildingManager));
            DumpMembers(w, "VehicleManager", typeof(VehicleManager));
            DumpMembers(w, "ZoneBlock", typeof(ZoneBlock));
            DumpMembers(w, "LoadingManager", typeof(LoadingManager));
            w.EndObject();

            var t = ModRuntime.Threading;
            w.Name("clock").BeginObject()
                .Name("paused").Value(t != null && t.simulationPaused)
                .Name("tick").Value((long)(t != null ? t.simulationTick : 0u))
                .Name("speed").Value((long)(t != null ? t.simulationSpeed : 0))
             .EndObject();

            w.EndObject();
            string json = w.ToString();
            Http.Log.Info("PROBE DUMP: " + json);
            return json;
        }

        private static void ReportField(JsonWriter w, string label, Type type, string field)
        {
            try
            {
                if (field == null) { w.Name(label).Value(type != null ? "type_exists" : "missing"); return; }
                var f = type.GetField(field, BindingFlags.Public | BindingFlags.Instance | BindingFlags.Static);
                w.Name(label).Value(f != null ? ("exists: " + f.FieldType.Name) : "MISSING");
            }
            catch (Exception e) { w.Name(label).Value("ERROR: " + e.Message); }
        }

        private static void DumpMembers(JsonWriter w, string label, Type type)
        {
            w.Name(label).BeginArray();
            try
            {
                foreach (var f in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
                    w.Value(f.Name + " : " + f.FieldType.Name);
                foreach (var p in type.GetProperties(BindingFlags.Public | BindingFlags.Instance))
                    w.Value("prop " + p.Name + " : " + p.PropertyType.Name);
            }
            catch (Exception e) { w.Value("ERROR: " + e.Message); }
            w.EndArray();
        }
    }
}

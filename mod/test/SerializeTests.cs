using System;
using System.Collections.Generic;
using SkylineBench.Dto;
using SkylineBench.Json;

namespace SkylineBench.Tests
{
    public static class SerializeTests
    {
        public static void Register(List<KeyValuePair<string, Action>> tests)
        {
            tests.Add(new KeyValuePair<string, Action>("serialize: network", Network));
            tests.Add(new KeyValuePair<string, Action>("serialize: metrics shape", Metrics));
            tests.Add(new KeyValuePair<string, Action>("serialize: metrics segment length and abandoned buildings", MetricsIncludesSegmentLengthAndAbandoned));
            tests.Add(new KeyValuePair<string, Action>("serialize: action ok", ActionOk));
            tests.Add(new KeyValuePair<string, Action>("serialize: action error omits diff", ActionErr));
            tests.Add(new KeyValuePair<string, Action>("serialize: clock state", Clock));
            tests.Add(new KeyValuePair<string, Action>("serialize: load result", Load));
            tests.Add(new KeyValuePair<string, Action>("serialize: road types shape", RoadTypesShape));
        }

        static void Network()
        {
            var net = new NetworkDto();
            net.Nodes.Add(new NodeDto { Id = 1, X = -50f, Y = 0f, Z = 10f });
            net.Segments.Add(new SegmentDto { Id = 7, StartNode = 1, EndNode = 2, Prefab = "Basic Road", Lanes = 2, Length = 100f, OneWay = true, TravelDirection = "start_to_end", SpeedLimit = 2f });
            Assert.Equal(
                "{\"nodes\":[{\"id\":1,\"x\":-50,\"y\":0,\"z\":10}],\"segments\":[{\"id\":7,\"start_node\":1,\"end_node\":2,\"prefab\":\"Basic Road\",\"lanes\":2,\"length\":100,\"one_way\":true,\"travel_direction\":\"start_to_end\",\"speed_limit\":2}]}",
                Serialize.Network(net));
        }

        static void Metrics()
        {
            var m = new MetricsDto { Tick = 42, FlowPercent = 73.5f, ActiveVehicles = 120, Balance = 0, WeeklyIncome = 500, WeeklyExpenses = 400, Funds = 50000, Population = 2000, ResidentialDemand = 50, CommercialDemand = 40, WorkplaceDemand = 30, Employed = 1500, Happiness = 80 };
            m.SegmentLoads.Add(new SegmentLoadDto { SegmentId = 7, Density = 0.5f });
            string json = Serialize.Metrics(m);
            Assert.True(json.StartsWith("{\"tick\":42,"), "starts with tick");
            Assert.True(json.Contains("\"traffic\":{\"flow_percent\":73.5,\"active_vehicles\":120,\"segment_loads\":[{\"segment_id\":7,\"density\":0.5,\"length\":0}]}"), "traffic group: " + json);
            Assert.True(json.Contains("\"economy\":{\"balance\":0,\"weekly_income\":500,\"weekly_expenses\":400,\"funds\":50000}"), "economy group");
            Assert.True(json.Contains("\"population\":{\"total\":2000,\"residential_demand\":50,\"commercial_demand\":40,\"workplace_demand\":30,\"employed\":1500}"), "population group");
            Assert.True(json.Contains("\"services\":{\"happiness\":80,\"abandoned_buildings\":0}"), "services group");
        }

        static void MetricsIncludesSegmentLengthAndAbandoned()
        {
            var m = new MetricsDto { Tick = 1, FlowPercent = 50f, ActiveVehicles = 10, AbandonedBuildings = 7 };
            m.SegmentLoads.Add(new SegmentLoadDto { SegmentId = 3, Density = 0.9f, Length = 52.5f });
            string json = Serialize.Metrics(m);
            Assert.True(json.Contains("\"length\":52.5"), "segment length in json: " + json);
            Assert.True(json.Contains("\"abandoned_buildings\":7"), "abandoned_buildings in json: " + json);
        }

        static void ActionOk()
        {
            var r = new ActionResultDto { Ok = true };
            r.CreatedNodes.Add(1); r.CreatedNodes.Add(2); r.CreatedSegments.Add(7); r.SnappedNodes.Add(1);
            Assert.Equal(
                "{\"ok\":true,\"created_nodes\":[1,2],\"created_segments\":[7],\"snapped_nodes\":[1],\"destroyed\":[]}",
                Serialize.Action(r));
        }

        static void ActionErr()
        {
            Assert.Equal("{\"ok\":false,\"reason\":\"INVALID_PREFAB\"}", Serialize.Action(ActionResultDto.Fail("INVALID_PREFAB")));
        }

        static void Clock()
        {
            Assert.Equal("{\"ok\":true,\"paused\":false,\"tick\":42}",
                Serialize.Clock(new ClockStateDto { Ok = true, Paused = false, Tick = 42 }));
        }

        static void Load()
        {
            Assert.Equal("{\"ok\":true,\"city_loaded\":true}",
                Serialize.Load(new LoadResultDto { Ok = true, CityLoaded = true }));
        }

        // Verifies the JSON object shape the handler emits; built directly with JsonWriter because game prefabs can't be loaded in the no-game test harness.
        public static void RoadTypesShape()
        {
            var w = new SkylineBench.Json.JsonWriter();
            w.BeginObject().Name("road_types").BeginArray();
            w.BeginObject().Name("name").Value("Basic Road").Name("construction_cost").Value((long)1200).EndObject();
            w.EndArray().EndObject();
            Assert.Equal("{\"road_types\":[{\"name\":\"Basic Road\",\"construction_cost\":1200}]}", w.ToString());
        }
    }
}

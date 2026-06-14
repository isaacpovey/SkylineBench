namespace SkylineBench.Json
{
    public struct BuildRoadReq { public float StartX, StartY, StartZ, EndX, EndY, EndZ; public string Prefab; public bool Snap; }
    public struct BulldozeReq { public string TargetType; public uint Id; }
    public struct UpgradeRoadReq { public uint SegmentId; public string Prefab; }
    public struct SetZoneReq { public float MinX, MinZ, MaxX, MaxZ; public string ZoneType; }
    public struct ClockReq { public string Op; public int Ticks; public int Speed; }
    public struct LoadSaveReq { public string SaveName; }
    public struct ScreenshotReq { public float X, Z, Size, Yaw, Pitch; public string InfoView; }
    public struct KeyframeReq { public float X, Z, Yaw, Pitch, Size; }
    public struct FlybyReq { public KeyframeReq[] Keyframes; public float DurationS; public int CaptureFps; public string OutDir; }

    /// <summary>Pure: JsonValue (parsed request body) → typed action arg structs.
    /// Field names match the broker's bridge_client JSON bodies.</summary>
    public static class RequestParse
    {
        public static BuildRoadReq BuildRoad(JsonValue v)
        {
            var s = v["start"]; var e = v["end"];
            return new BuildRoadReq
            {
                StartX = (float)s["x"].AsDouble(), StartY = (float)s["y"].AsDouble(), StartZ = (float)s["z"].AsDouble(),
                EndX = (float)e["x"].AsDouble(), EndY = (float)e["y"].AsDouble(), EndZ = (float)e["z"].AsDouble(),
                Prefab = v["prefab"].AsString(),
                Snap = !v["snap_to_existing_nodes"].IsNull && v["snap_to_existing_nodes"].AsBool()
            };
        }

        public static BulldozeReq Bulldoze(JsonValue v)
        {
            return new BulldozeReq { TargetType = v["target_type"].AsString(), Id = (uint)v["id"].AsDouble() };
        }

        public static UpgradeRoadReq UpgradeRoad(JsonValue v)
        {
            return new UpgradeRoadReq { SegmentId = (uint)v["segment_id"].AsDouble(), Prefab = v["prefab"].AsString() };
        }

        public static SetZoneReq SetZone(JsonValue v)
        {
            var r = v["rect"];
            return new SetZoneReq
            {
                MinX = (float)r["min_x"].AsDouble(), MinZ = (float)r["min_z"].AsDouble(),
                MaxX = (float)r["max_x"].AsDouble(), MaxZ = (float)r["max_z"].AsDouble(),
                ZoneType = v["zone_type"].AsString()
            };
        }

        public static ClockReq Clock(JsonValue v)
        {
            return new ClockReq
            {
                Op = v["op"].AsString(),
                Ticks = v["ticks"].IsNull ? 0 : (int)v["ticks"].AsDouble(),
                Speed = v["speed"].IsNull ? 0 : (int)v["speed"].AsDouble()
            };
        }

        public static LoadSaveReq LoadSave(JsonValue v) { return new LoadSaveReq { SaveName = v["save_name"].AsString() }; }

        public static ScreenshotReq Screenshot(JsonValue v)
        {
            return new ScreenshotReq
            {
                X = (float)v["x"].AsDouble(),
                Z = (float)v["z"].AsDouble(),
                Size = v["size"].IsNull ? 1000f : (float)v["size"].AsDouble(),
                Yaw = v["yaw"].IsNull ? 0f : (float)v["yaw"].AsDouble(),
                Pitch = v["pitch"].IsNull ? 90f : (float)v["pitch"].AsDouble(),
                InfoView = v["info_view"].IsNull ? "none" : v["info_view"].AsString(),
            };
        }

        public static FlybyReq Flyby(JsonValue v)
        {
            var arr = v["keyframes"];
            int n = arr.Count;
            var kfs = new KeyframeReq[n];
            for (int i = 0; i < n; i++)
            {
                var k = arr[i];
                kfs[i] = new KeyframeReq
                {
                    X = (float)k["x"].AsDouble(),
                    Z = (float)k["z"].AsDouble(),
                    Yaw = (float)k["yaw"].AsDouble(),
                    Pitch = (float)k["pitch"].AsDouble(),
                    Size = (float)k["size"].AsDouble(),
                };
            }
            return new FlybyReq
            {
                Keyframes = kfs,
                DurationS = (float)v["duration_s"].AsDouble(),
                CaptureFps = (int)v["capture_fps"].AsDouble(),
                OutDir = v["out_dir"].AsString(),
            };
        }
    }
}

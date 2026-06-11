using System.Collections.Generic;

namespace SkylineBench.Dto
{
    public struct NodeDto { public uint Id; public float X; public float Y; public float Z; }
    public struct SegmentDto { public uint Id; public uint StartNode; public uint EndNode; public string Prefab; public byte Lanes; public float Length; public bool OneWay; public string TravelDirection; public float SpeedLimit; }
    public sealed class NetworkDto { public List<NodeDto> Nodes = new List<NodeDto>(); public List<SegmentDto> Segments = new List<SegmentDto>(); }

    public struct BuildingDto { public uint Id; public string Prefab; public string Category; public float X; public float Y; public float Z; public float FootprintWidth; public float FootprintLength; public byte Level; }
    public sealed class BuildingsDto { public List<BuildingDto> Buildings = new List<BuildingDto>(); }

    public struct ZoneCellDto { public float X; public float Z; public string ZoneType; }
    public sealed class ZonesDto { public List<ZoneCellDto> Cells = new List<ZoneCellDto>(); }

    public struct SegmentLoadDto { public uint SegmentId; public float Density; public float Length; }
    public sealed class MetricsDto
    {
        public ulong Tick;
        public float FlowPercent; public uint ActiveVehicles; public List<SegmentLoadDto> SegmentLoads = new List<SegmentLoadDto>();
        public long Balance; public long WeeklyIncome; public long WeeklyExpenses; public long Funds;
        public uint Population; public byte ResidentialDemand; public byte CommercialDemand; public byte WorkplaceDemand; public uint Employed;
        public byte Happiness;
        public uint AbandonedBuildings;
    }

    /// <summary>Result of a mutation. Ok==true ⇒ diff fields meaningful; else Reason set (a normalized code).</summary>
    public sealed class ActionResultDto
    {
        public bool Ok;
        public List<uint> CreatedNodes = new List<uint>();
        public List<uint> CreatedSegments = new List<uint>();
        public List<uint> SnappedNodes = new List<uint>();
        public List<uint> Destroyed = new List<uint>();
        public string Reason; // null when Ok
        public static ActionResultDto Fail(string reason) { return new ActionResultDto { Ok = false, Reason = reason }; }
    }

    public sealed class ClockStateDto { public bool Ok; public bool Paused; public ulong Tick; }
    public sealed class LoadResultDto { public bool Ok; public bool CityLoaded; }
}

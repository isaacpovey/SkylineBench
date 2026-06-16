using System.Collections.Generic;

namespace SkylineBench.Dto
{
    public struct NodeDto { public uint Id; public float X; public float Y; public float Z; }
    public struct SegmentDto { public uint Id; public uint StartNode; public uint EndNode; public string Prefab; public byte Lanes; public float Length; public bool OneWay; public string TravelDirection; public float SpeedLimit; }
    public sealed class NetworkDto { public List<NodeDto> Nodes = new List<NodeDto>(); public List<SegmentDto> Segments = new List<SegmentDto>(); }

    public struct BuildingDto { public uint Id; public string Prefab; public string Category; public float X; public float Y; public float Z; public float FootprintWidth; public float FootprintLength; public byte Level; public bool Abandoned; }
    public sealed class BuildingsDto { public List<BuildingDto> Buildings = new List<BuildingDto>(); }

    public struct ZoneCellDto { public float X; public float Z; public string ZoneType; }
    public sealed class ZonesDto { public List<ZoneCellDto> Cells = new List<ZoneCellDto>(); }

    public sealed class ProblemBuildingDto { public uint Id; public float X; public float Z; public string Category; public List<string> Problems = new List<string>(); }
    public sealed class ProblemsDto { public List<ProblemBuildingDto> Buildings = new List<ProblemBuildingDto>(); }

    public struct SegmentLoadDto { public uint SegmentId; public float Density; public float Length; }
    public sealed class MetricsDto
    {
        public ulong Tick;
        public float FlowPercent; public uint ActiveVehicles; public List<SegmentLoadDto> SegmentLoads = new List<SegmentLoadDto>();
        public long Balance; public long WeeklyIncome; public long WeeklyExpenses; public long Funds;
        public uint Population; public byte ResidentialDemand; public byte CommercialDemand; public byte WorkplaceDemand;
        public byte Happiness;
        public uint AbandonedBuildings;
        // Per-building problem flags — leading (non-lagging) signals that a
        // change cut buildings off from the road network or a utility. A spike
        // in RoadNotConnected/Garbage right after an upgrade is the death-spiral
        // precursor that AbandonedBuildings only reflects many days later.
        public uint RoadNotConnected;
        public uint NoElectricity;
        public uint NoWater;
        public uint NoSewage;
        public uint GarbagePiling;
        public uint NoFuel;
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
        public int ZonedBuildingsFronting = -1; // -1 = not computed / not applicable
        public List<uint> CollidingBuildings = new List<uint>();
        public static ActionResultDto Fail(string reason) { return new ActionResultDto { Ok = false, Reason = reason }; }
    }

    public sealed class ClockStateDto { public bool Ok; public bool Paused; public ulong Tick; public bool ForcedPaused; }
    // Identity of a savegame asset, mirroring the CS1 API: Name = Package.Asset.name
    // (save file name), CityName = SaveGameMetaData.cityName, FullName = Package.Asset.fullName
    // (package-qualified id).
    public sealed class SaveInfoDto { public string Name; public string CityName; public string FullName; }

    public sealed class LoadResultDto
    {
        public bool Ok;
        public bool CityLoaded;
        // Identity of the asset the loader resolved (null when Ok==false).
        public SaveInfoDto Resolved;
        // Save names the game exposes; populated only on a no-match miss, so a
        // failed load tells the operator what to pin instead of guessing.
        public List<SaveInfoDto> Available = new List<SaveInfoDto>();
    }
}

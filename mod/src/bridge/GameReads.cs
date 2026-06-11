using ColossalFramework;
using UnityEngine;
using SkylineBench.Dto;

namespace SkylineBench.Bridge
{
    public static class GameReads
    {
        private const int TimeoutMs = 5000;

        public static NetworkDto Network()
        {
            return SimThread.Run<NetworkDto>(delegate
            {
                var dto = new NetworkDto();
                var nm = Singleton<NetManager>.instance;
                var roadNodeIds = new System.Collections.Generic.HashSet<uint>();
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    var info = s.Info;
                    // Roads only: water pipes and power lines are also NetSegments
                    // and previously polluted the network dump and rendered map.
                    if (info == null || info.m_class == null || info.m_class.m_service != ItemClass.Service.Road) continue;
                    bool hasFwd = info.m_hasForwardVehicleLanes;
                    bool hasBwd = info.m_hasBackwardVehicleLanes;
                    bool inverted = (s.m_flags & NetSegment.Flags.Invert) != NetSegment.Flags.None;
                    dto.Segments.Add(new SegmentDto
                    {
                        Id = i, StartNode = s.m_startNode, EndNode = s.m_endNode,
                        Prefab = info.name != null ? info.name : "",
                        Lanes = (byte)(info.m_lanes != null ? info.m_lanes.Length : 0),
                        Length = s.m_averageLength,
                        OneWay = Direction.IsOneWay(hasFwd, hasBwd),
                        TravelDirection = Direction.Travel(hasFwd, hasBwd, inverted),
                        SpeedLimit = info.m_averageVehicleLaneSpeed
                    });
                    roadNodeIds.Add(s.m_startNode);
                    roadNodeIds.Add(s.m_endNode);
                }
                for (uint i = 0; i < nm.m_nodes.m_buffer.Length; i++)
                {
                    var n = nm.m_nodes.m_buffer[i];
                    if ((n.m_flags & NetNode.Flags.Created) == NetNode.Flags.None) continue;
                    if (!roadNodeIds.Contains(i)) continue;
                    dto.Nodes.Add(new NodeDto { Id = i, X = n.m_position.x, Y = n.m_position.y, Z = n.m_position.z });
                }
                return dto;
            }, TimeoutMs);
        }

        public static BuildingsDto Buildings()
        {
            return SimThread.Run<BuildingsDto>(delegate
            {
                var dto = new BuildingsDto();
                var bm = Singleton<BuildingManager>.instance;
                for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
                {
                    var b = bm.m_buildings.m_buffer[i];
                    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                    var info = b.Info;
                    dto.Buildings.Add(new BuildingDto
                    {
                        Id = i, Prefab = info != null ? info.name : "", Category = Category(info),
                        X = b.m_position.x, Y = b.m_position.y, Z = b.m_position.z,
                        FootprintWidth = info != null ? info.m_cellWidth * 8f : 0f,
                        FootprintLength = info != null ? info.m_cellLength * 8f : 0f,
                        Level = (byte)b.m_level
                    });
                }
                return dto;
            }, TimeoutMs);
        }

        private static string Category(BuildingInfo info)
        {
            if (info == null || info.m_class == null) return "other";
            switch (info.m_class.m_service)
            {
                case ItemClass.Service.Residential: return "residential";
                case ItemClass.Service.Commercial: return "commercial";
                case ItemClass.Service.Industrial: return "industrial";
                case ItemClass.Service.Office: return "office";
                default: return "service";
            }
        }

        public static MetricsDto Metrics()
        {
            return SimThread.Run<MetricsDto>(delegate
            {
                var dto = new MetricsDto();
                dto.Tick = Singleton<SimulationManager>.instance.m_currentTickIndex;
                var vm = Singleton<VehicleManager>.instance;
                dto.ActiveVehicles = vm.cityVehicleCount;
                // m_lastTrafficFlow is already the city traffic-flow percentage
                // (0..100): the game rolls it up every 256 sim frames as
                // min(100, m_totalTrafficFlow*100 / m_maxTrafficFlow) and then
                // resets the accumulators (verified against Assembly-CSharp IL).
                // Dividing it by the (mid-refill) accumulator was the bug.
                dto.FlowPercent = vm.m_lastTrafficFlow;
                var nm = Singleton<NetManager>.instance;
                for (uint i = 0; i < nm.m_segments.m_buffer.Length; i++)
                {
                    var s = nm.m_segments.m_buffer[i];
                    if ((s.m_flags & NetSegment.Flags.Created) == NetSegment.Flags.None) continue;
                    var sInfo = s.Info;
                    if (sInfo == null || sInfo.m_class == null || sInfo.m_class.m_service != ItemClass.Service.Road) continue;
                    // m_trafficDensity is a byte the game rolls up to a max of
                    // 100, not 255 — dividing by 255 pinned every saturated
                    // segment at 0.39 and destroyed congestion ranking.
                    dto.SegmentLoads.Add(new SegmentLoadDto { SegmentId = i, Density = Mathf.Min(1f, s.m_trafficDensity / 100f), Length = s.m_averageLength });
                }
                var em = Singleton<EconomyManager>.instance;
                dto.Funds = em.LastCashAmount;
                long income, expenses;
                em.GetIncomeAndExpenses(ItemClass.Service.None, ItemClass.SubService.None, ItemClass.Level.None, out income, out expenses);
                dto.WeeklyIncome = income;
                dto.WeeklyExpenses = expenses + em.GetLoanExpenses() + em.GetPolicyExpenses();
                dto.Balance = income - dto.WeeklyExpenses;
                var zm = Singleton<ZoneManager>.instance;
                dto.ResidentialDemand = (byte)Mathf.Clamp(zm.m_actualResidentialDemand, 0, 100);
                dto.CommercialDemand = (byte)Mathf.Clamp(zm.m_actualCommercialDemand, 0, 100);
                dto.WorkplaceDemand = (byte)Mathf.Clamp(zm.m_actualWorkplaceDemand, 0, 100);
                var dm = Singleton<DistrictManager>.instance;
                dto.Population = dm.m_districts.m_buffer[0].m_populationData.m_finalCount;
                // Employment isn't cleanly exposed by a single manager field; left at 0.
                dto.Employed = 0;
                dto.Happiness = (byte)Mathf.Clamp((int)dm.m_districts.m_buffer[0].m_finalHappiness, 0, 100);
                var bm = Singleton<BuildingManager>.instance;
                uint abandoned = 0;
                for (uint i = 0; i < bm.m_buildings.m_buffer.Length; i++)
                {
                    var b = bm.m_buildings.m_buffer[i];
                    if ((b.m_flags & Building.Flags.Created) == Building.Flags.None) continue;
                    if ((b.m_flags & Building.Flags.Abandoned) != Building.Flags.None) abandoned++;
                }
                dto.AbandonedBuildings = abandoned;
                return dto;
            }, TimeoutMs);
        }

        public static ZonesDto Zones()
        {
            return SimThread.Run<ZonesDto>(delegate
            {
                var dto = new ZonesDto();
                var zm = Singleton<ZoneManager>.instance;
                for (int b = 0; b < zm.m_blocks.m_buffer.Length; b++)
                {
                    var block = zm.m_blocks.m_buffer[b];
                    if ((block.m_flags & ZoneBlock.FLAG_CREATED) == 0u) continue;
                    DecodeBlock(ref block, dto);
                }
                return dto;
            }, TimeoutMs);
        }

        private static void DecodeBlock(ref ZoneBlock block, ZonesDto dto)
        {
            int rows = block.RowCount;
            Vector3 pos = block.m_position;
            float a = block.m_angle;
            Vector3 right = new Vector3(Mathf.Cos(a), 0f, Mathf.Sin(a));
            Vector3 forward = new Vector3(-Mathf.Sin(a), 0f, Mathf.Cos(a));
            // A block holds up to 16 rows (m_zone1 rows 0-7, m_zone2 rows 8-15).
            for (int row = 0; row < rows && row < 16; row++)
                for (int col = 0; col < 4; col++)
                {
                    ItemClass.Zone z = block.GetZone(col, row);
                    string zt = ZoneTypeName(z);
                    if (zt == null) continue;
                    Vector3 cell = pos + right * ((col - 1.5f) * 8f) + forward * ((row + 0.5f) * 8f);
                    dto.Cells.Add(new ZoneCellDto { X = cell.x, Z = cell.z, ZoneType = zt });
                }
        }

        private static string ZoneTypeName(ItemClass.Zone z)
        {
            switch (z)
            {
                case ItemClass.Zone.ResidentialLow: return "residential_low";
                case ItemClass.Zone.ResidentialHigh: return "residential_high";
                case ItemClass.Zone.CommercialLow: return "commercial_low";
                case ItemClass.Zone.CommercialHigh: return "commercial_high";
                case ItemClass.Zone.Industrial: return "industrial";
                case ItemClass.Zone.Office: return "office";
                default: return null;
            }
        }
    }
}

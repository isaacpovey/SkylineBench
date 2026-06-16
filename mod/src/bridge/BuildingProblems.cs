using System.Collections.Generic;

namespace SkylineBench.Bridge
{
    /// <summary>Single source of truth mapping a building's problem flags to the
    /// normalised problem-name vocabulary, shared by the /metrics counts and the
    /// /problems read so the two cannot drift. Must run on the simulation thread
    /// (reads live Building state).</summary>
    public static class BuildingProblems
    {
        public static List<string> Names(ref Building b)
        {
            var names = new List<string>();
            if ((b.m_flags & Building.Flags.Abandoned) != Building.Flags.None) names.Add("abandoned");
            // Building problem flags live in ProblemStruct.m_Problems1 (this game
            // version split the old flat Notification.Problem enum in two).
            var p = b.m_problems.m_Problems1;
            if (Has(p, Notification.Problem1.RoadNotConnected)) names.Add("road_not_connected");
            if (Has(p, Notification.Problem1.Electricity) || Has(p, Notification.Problem1.ElectricityNotConnected)) names.Add("no_electricity");
            if (Has(p, Notification.Problem1.Water) || Has(p, Notification.Problem1.WaterNotConnected)) names.Add("no_water");
            if (Has(p, Notification.Problem1.Sewage)) names.Add("no_sewage");
            if (Has(p, Notification.Problem1.Garbage)) names.Add("garbage_piling");
            if (Has(p, Notification.Problem1.NoFuel)) names.Add("no_fuel");
            return names;
        }

        private static bool Has(Notification.Problem1 flags, Notification.Problem1 flag)
        {
            return (flags & flag) != Notification.Problem1.None;
        }
    }
}

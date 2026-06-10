namespace SkylineBench.Bridge
{
    /// <summary>Travel direction of a road segment, derived from the prefab's
    /// vehicle-lane flags and the segment's Invert flag. A one-way prefab's lanes
    /// all run "forward" (start→end); Invert means the segment was drawn opposite
    /// to the prefab orientation, flipping the effective direction. Pure (no game
    /// references) so it is unit-testable in the no-game harness.</summary>
    public static class Direction
    {
        public const string Both = "both";
        public const string StartToEnd = "start_to_end";
        public const string EndToStart = "end_to_start";

        public static bool IsOneWay(bool hasForwardLanes, bool hasBackwardLanes)
        {
            return hasForwardLanes != hasBackwardLanes;
        }

        public static string Travel(bool hasForwardLanes, bool hasBackwardLanes, bool inverted)
        {
            if (!IsOneWay(hasForwardLanes, hasBackwardLanes)) return Both;
            return (hasForwardLanes != inverted) ? StartToEnd : EndToStart;
        }
    }
}

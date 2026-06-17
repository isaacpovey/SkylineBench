namespace SkylineBench.Bridge
{
    /// <summary>Pure map from ToolBase.ToolErrors bit flags to a normalized
    /// ErrorCode string. Takes the raw ulong so it has no game dependency and
    /// is unit-testable. Returns null when no error bits are set. Priority
    /// order: report the most actionable cause first.</summary>
    public static class RoadErrors
    {
        public static string Reason(ulong bits)
        {
            if (bits == 0UL) return null;
            if ((bits & 0x10UL) != 0) return ErrorCode.ObjectCollision;        // ObjectCollision
            if ((bits & 0x200UL) != 0) return ErrorCode.SlopeTooSteep;         // SlopeTooSteep
            if ((bits & 0x800UL) != 0) return ErrorCode.HeightTooHigh;         // HeightTooHigh
            if ((bits & 0x2000UL) != 0) return ErrorCode.CannotBuildOnWater;   // CannotBuildOnWater
            if ((bits & 0x20UL) != 0) return ErrorCode.OutOfArea;              // OutOfArea
            if ((bits & 0x40000UL) != 0) return ErrorCode.TooManyConnections;  // TooManyConnections
            if ((bits & 0x100UL) != 0) return ErrorCode.TooShort;              // TooShort
            if ((bits & 0x80UL) != 0) return ErrorCode.InvalidShape;           // InvalidShape
            return ErrorCode.Unknown;
        }
    }
}

namespace SkylineBench.Bridge
{
    /// <summary>Normalized action failure reasons (spec §5). The HTTP layer returns these
    /// at HTTP 200 with {ok:false,reason}.</summary>
    public static class ErrorCode
    {
        public const string OutOfBounds = "OUT_OF_BOUNDS";
        public const string InvalidPrefab = "INVALID_PREFAB";
        public const string SegmentTooLong = "SEGMENT_TOO_LONG";
        public const string InvalidArgs = "INVALID_ARGS";
        public const string Unknown = "UNKNOWN";
        public const string ObjectCollision = "OBJECT_COLLISION";
        public const string SlopeTooSteep = "SLOPE_TOO_STEEP";
        public const string OutOfArea = "OUT_OF_AREA";
        public const string TooManyConnections = "TOO_MANY_CONNECTIONS";
        public const string NetBufferFull = "NET_BUFFER_FULL";
        public const string CannotBuildOnWater = "CANNOT_BUILD_ON_WATER";
        public const string HeightTooHigh = "HEIGHT_TOO_HIGH";
    }
}

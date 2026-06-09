using ColossalFramework;
using UnityEngine;

namespace SkylineBench.Bridge
{
    /// <summary>
    /// Writes zone types into ZoneBlock cells. Uses the confirmed game API
    /// <c>ZoneBlock.SetZone(int x, int z, ItemClass.Zone)</c> followed by
    /// <c>ZoneBlock.RefreshZoning(ushort blockID)</c> (both verified via monodis on
    /// Assembly-CSharp.dll). SetZone encodes the cell as a 4-bit nibble in
    /// m_zone1 (x in 0..1) / m_zone2 (x in 2..3) and sets FLAG_TYPECHANGED; RefreshZoning
    /// recomputes the block's visuals/zoning. SetZone's (x, z) are the column/row, the
    /// same indices GameReads.DecodeBlock passes to GetZone(col, row), so read and write
    /// agree on geometry. Must be invoked on the sim thread.
    /// </summary>
    public static class ZoneWriter
    {
        private const float CellSizeM = 8f;
        private const int ColsPerBlock = 4;
        private const int MaxRowsPerBlock = 16;

        public static void SetZoneOverRect(float minX, float minZ, float maxX, float maxZ, ItemClass.Zone zone)
        {
            var zm = Singleton<ZoneManager>.instance;
            var buffer = zm.m_blocks.m_buffer;
            for (int b = 0; b < buffer.Length; b++)
            {
                if ((buffer[b].m_flags & ZoneBlock.FLAG_CREATED) == 0u) continue;
                if (ApplyToBlock(ref buffer[b], minX, minZ, maxX, maxZ, zone))
                    buffer[b].RefreshZoning((ushort)b);
            }
        }

        private static bool ApplyToBlock(ref ZoneBlock block, float minX, float minZ, float maxX, float maxZ, ItemClass.Zone zone)
        {
            int rows = block.RowCount;
            int rowLimit = rows < MaxRowsPerBlock ? rows : MaxRowsPerBlock;
            Vector3 pos = block.m_position;
            float a = block.m_angle;
            Vector3 right = new Vector3(Mathf.Cos(a), 0f, Mathf.Sin(a));
            Vector3 forward = new Vector3(-Mathf.Sin(a), 0f, Mathf.Cos(a));
            bool changed = false;
            for (int row = 0; row < rowLimit; row++)
                for (int col = 0; col < ColsPerBlock; col++)
                {
                    Vector3 cell = pos + right * ((col - 1.5f) * CellSizeM) + forward * ((row + 0.5f) * CellSizeM);
                    if (cell.x < minX || cell.x > maxX || cell.z < minZ || cell.z > maxZ) continue;
                    if (block.SetZone(col, row, zone)) changed = true;
                }
            return changed;
        }
    }
}

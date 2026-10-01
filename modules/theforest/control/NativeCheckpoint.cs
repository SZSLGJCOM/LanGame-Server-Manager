using System;
using System.IO;
using System.Runtime.InteropServices;

// Non-generic System.Action belongs to a different assembly in Unity's old
// framework. This callback never crosses the game boundary, so own its type.
internal delegate void ForestSaveOperation();

internal sealed class ForestCheckpointFailure : IOException
{
    internal readonly string Code;
    internal ForestCheckpointFailure(string code) { Code = code; }
}

internal static class ForestNativeCheckpoint
{
    // This game's Mono FileStream.SafeFileHandle creates another owning wrapper
    // without transferring ownership. Native SaveGame triggers GC, so querying
    // through that getter can close a still-owned/reused handle. Own one actual
    // Win32 metadata handle and close it only with its matching Win32 function.
    private sealed class NativeFile : SafeHandle
    {
        internal NativeFile(IntPtr value) : base(IntPtr.Zero, true) { SetHandle(value); }
        public override bool IsInvalid { get { return handle == IntPtr.Zero || handle == new IntPtr(-1); } }
        protected override bool ReleaseHandle() { return CloseHandle(handle); }
    }

    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool GetFileInformationByHandle(NativeFile handle, out FileIdentity identity);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern IntPtr CreateFileW(string name, uint access, uint share, IntPtr security, uint disposition, uint flags, IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool CloseHandle(IntPtr handle);

    [StructLayout(LayoutKind.Sequential)]
    private struct FileIdentity
    {
        internal uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh, WriteLow, WriteHigh;
        internal uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
        internal bool SameFile(FileIdentity other) { return Volume == other.Volume && IndexHigh == other.IndexHigh && IndexLow == other.IndexLow; }
        internal long Length { get { return checked(((long)SizeHigh << 32) | SizeLow); } }
    }

    private static NativeFile Open(string path, bool missingAllowed)
    {
        // FILE_READ_ATTRIBUTES; FILE_SHARE_READ|WRITE|DELETE; OPEN_EXISTING.
        IntPtr handle = CreateFileW(path, 0x80, 7, IntPtr.Zero, 3, 0x80, IntPtr.Zero);
        if (handle == IntPtr.Zero || handle == new IntPtr(-1))
        {
            int error = Marshal.GetLastWin32Error();
            if (missingAllowed && (error == 2 || error == 3)) return null;
            throw new ForestCheckpointFailure("checkpoint_open");
        }
        return new NativeFile(handle);
    }

    internal static long Save(string path, ForestSaveOperation nativeSave)
    {
        // Build 3488796 renames an existing checkpoint to `prev`, then writes
        // its replacement. Keep the original object pinned across native GC and
        // rename; an unchanged file and reused file IDs cannot prove a save.
        using (NativeFile before = Open(path, true))
        {
            FileIdentity previous = new FileIdentity();
            if (before != null && !GetFileInformationByHandle(before, out previous))
                throw new ForestCheckpointFailure("checkpoint_before_query");
            nativeSave();
            using (NativeFile after = Open(path, false))
            {
                FileIdentity current;
                if (!GetFileInformationByHandle(after, out current)) throw new ForestCheckpointFailure("checkpoint_after_query");
                if (current.Length <= 0) throw new ForestCheckpointFailure("checkpoint_empty");
                if (before != null && previous.SameFile(current)) throw new ForestCheckpointFailure("checkpoint_unchanged");
                return current.Length;
            }
        }
    }
}

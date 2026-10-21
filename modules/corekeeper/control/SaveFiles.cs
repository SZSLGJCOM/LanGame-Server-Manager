using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace LanGame.SaveControl
{
    // Local IPC uses the same pinned-ancestor/OPEN_REPARSE_POINT boundary as
    // Valheim control. The peer cannot choose a leaf, native object or save path.
    internal sealed class SaveFiles : IDisposable
    {
        private const uint ShareRead = 1, ShareWrite = 2, ShareDelete = 4, OpenReparse = 0x200000;
        private readonly List<SafeFileHandle> pins = new List<SafeFileHandle>();
        private readonly string root;
        [StructLayout(LayoutKind.Sequential)]
        private struct Info
        {
            internal uint Attributes;
            internal System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
            internal uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
        }
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFile(string path, uint access, uint sharing,
            IntPtr security, uint creation, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out Info info);

        internal SaveFiles(string path)
        {
            if (path == null || path.Length < 4 || !Char.IsLetter(path[0]) || path[1] != ':' ||
                (path[2] != '\\' && path[2] != '/') || path.IndexOf(':', 2) >= 0)
                throw new SaveFailure("invalid_control_root");
            root = Path.GetFullPath(path).TrimEnd('\\', '/');
            try
            {
                List<string> ancestors = new List<string>();
                for (DirectoryInfo item = new DirectoryInfo(root); item != null; item = item.Parent) ancestors.Add(item.FullName);
                ancestors.Reverse();
                foreach (string ancestor in ancestors)
                {
                    SafeFileHandle pin = CreateFile(ancestor, 1, ShareRead | ShareWrite,
                        IntPtr.Zero, 3, OpenReparse | 0x2000000, IntPtr.Zero);
                    try { Inspect(pin, true); pins.Add(pin); }
                    catch { pin.Dispose(); throw; }
                }
            }
            catch { Dispose(); throw; }
        }
        private static Info Inspect(SafeFileHandle handle, bool directory)
        {
            Info info;
            if (handle == null || handle.IsInvalid || !GetFileInformationByHandle(handle, out info) ||
                (info.Attributes & 0x400) != 0 || ((info.Attributes & 0x10) != 0) != directory)
                throw new SaveFailure("unsafe_control_path");
            return info;
        }
        internal static FileStream OpenPinnedFile(string path, long limit)
        {
            SafeFileHandle handle = CreateFile(path, 0x80000000, ShareRead, IntPtr.Zero, 3, OpenReparse, IntPtr.Zero);
            try
            {
                Info info = Inspect(handle, false);
                if ((((ulong)info.SizeHigh << 32) | info.SizeLow) > (ulong)limit) throw new SaveFailure("file_limit");
                return new FileStream(handle, FileAccess.Read);
            }
            catch { handle.Dispose(); throw; }
        }
        internal string Read(string leaf, bool optional)
        {
            if (leaf != "session.txt" && leaf != "request.txt" && leaf != "response.txt") throw new SaveFailure("invalid_leaf");
            SafeFileHandle handle = CreateFile(Path.Combine(root, leaf), 0x80000000,
                optional ? ShareRead | ShareWrite | ShareDelete : ShareRead, IntPtr.Zero, 3, OpenReparse, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error(); handle.Dispose();
                if (optional && (error == 2 || error == 3 || (leaf == "request.txt" && (error == 32 || error == 33)))) return null;
                throw new SaveFailure("control_unavailable");
            }
            using (handle)
            {
                Info info = Inspect(handle, false);
                if (info.SizeHigh != 0 || info.SizeLow > 2048) throw new SaveFailure("frame_limit");
                using (FileStream file = new FileStream(handle, FileAccess.Read))
                using (MemoryStream data = new MemoryStream())
                {
                    byte[] buffer = new byte[2049];
                    int count;
                    while ((count = file.Read(buffer, 0, buffer.Length)) != 0)
                    {
                        data.Write(buffer, 0, count);
                        if (data.Length > 2048) throw new SaveFailure("frame_limit");
                    }
                    return new UTF8Encoding(false, true).GetString(data.ToArray());
                }
            }
        }
        internal void Publish(string leaf, string value)
        {
            if (leaf != "response.txt" || value.Length > 2048) throw new SaveFailure("invalid_response");
            string destination = Path.Combine(root, leaf), temporary = Path.Combine(root, "response_" + Guid.NewGuid().ToString("N") + ".tmp");
            bool owned = false;
            try
            {
                byte[] bytes = new UTF8Encoding(false, true).GetBytes(value);
                using (FileStream file = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                {
                    owned = true; file.Write(bytes, 0, bytes.Length); file.Flush();
                }
                bool present;
                using (SafeFileHandle existing = CreateFile(destination, 0x80, ShareRead | ShareWrite | ShareDelete,
                    IntPtr.Zero, 3, OpenReparse, IntPtr.Zero))
                {
                    if (existing.IsInvalid)
                    {
                        int error = Marshal.GetLastWin32Error();
                        if (error != 2 && error != 3) throw new SaveFailure("unsafe_response");
                        present = false;
                    }
                    else { Inspect(existing, false); present = true; }
                }
                if (present) File.Replace(temporary, destination, null); else File.Move(temporary, destination);
                owned = false;
            }
            finally { if (owned) File.Delete(temporary); }
        }
        public void Dispose()
        {
            for (int i = pins.Count - 1; i >= 0; i--) pins[i].Dispose();
            pins.Clear();
        }
    }
}

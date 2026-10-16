using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using Microsoft.Win32.SafeHandles;

namespace Lgsm.Valheim.Control
{
    // Windows-only boundary: pin each existing ancestor without DELETE sharing
    // and open leaves with OPEN_REPARSE_POINT before inspecting their handles.
    internal sealed class ControlFiles : IDisposable
    {
        private const uint OpenExisting = 3;
        private const uint OpenReparse = 0x00200000;
        private const uint BackupSemantics = 0x02000000;
        private const uint AttributeReparse = 0x400;
        private const uint AttributeDirectory = 0x10;
        private const uint ShareRead = 1, ShareWrite = 2, ShareDelete = 4;
        private readonly List<SafeFileHandle> pins = new List<SafeFileHandle>();
        private readonly string root;
        private bool disposed;
        internal string LastRequestVersion { get; private set; }

        [StructLayout(LayoutKind.Sequential)]
        private struct FileInformation
        {
            internal uint Attributes;
            internal System.Runtime.InteropServices.ComTypes.FILETIME Creation;
            internal System.Runtime.InteropServices.ComTypes.FILETIME Access;
            internal System.Runtime.InteropServices.ComTypes.FILETIME Write;
            internal uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
        }
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFile(string path, uint access, uint sharing,
            IntPtr security, uint creation, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInformation info);

        internal ControlFiles(string path)
        {
            if (!LocalAbsolute(path))
                throw new ControlFailure("invalid_control_root");
            root = Path.GetFullPath(path).TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
            if (root.Length < 4 || root.StartsWith(@"\\", StringComparison.Ordinal) ||
                root.IndexOf(':', 2) >= 0) throw new ControlFailure("invalid_control_root");
            try
            {
                List<string> ancestors = new List<string>();
                for (DirectoryInfo current = new DirectoryInfo(root); current != null; current = current.Parent)
                    ancestors.Add(current.FullName);
                ancestors.Reverse();
                foreach (string ancestor in ancestors)
                {
                    SafeFileHandle handle = CreateFile(ancestor, 1, ShareRead | ShareWrite,
                        IntPtr.Zero, OpenExisting, OpenReparse | BackupSemantics, IntPtr.Zero);
                    try
                    {
                        Inspect(handle, true);
                        pins.Add(handle);
                    }
                    catch { handle.Dispose(); throw; }
                }
            }
            catch { Dispose(); throw; }
        }

        internal static bool LocalAbsolute(string path)
        {
            return path != null && path.Length >= 3 && Char.IsLetter(path[0]) && path[1] == ':' &&
                (path[2] == '\\' || path[2] == '/') && path.IndexOf(':', 2) < 0;
        }

        internal static SafeFileHandle PinBoundedFile(string path, int maximumBytes)
        {
            if (!LocalAbsolute(path)) throw new ControlFailure("unsafe_native_access_path");
            SafeFileHandle handle = CreateFile(path, 0x80000000, ShareRead,
                IntPtr.Zero, OpenExisting, OpenReparse, IntPtr.Zero);
            try
            {
                FileInformation info = Inspect(handle, false);
                ulong size = ((ulong)info.SizeHigh << 32) | info.SizeLow;
                if (size > (ulong)maximumBytes) throw new ControlFailure("native_access_file_limit");
                return handle;
            }
            catch { handle.Dispose(); throw; }
        }

        private static FileInformation Inspect(SafeFileHandle handle, bool directory)
        {
            FileInformation info;
            if (handle == null || handle.IsInvalid || !GetFileInformationByHandle(handle, out info) ||
                (info.Attributes & AttributeReparse) != 0 ||
                ((info.Attributes & AttributeDirectory) != 0) != directory)
                throw new ControlFailure("unsafe_control_path");
            return info;
        }

        internal byte[] ReadRequest()
        {
            return ReadLeaf("request.json", ControlProtocol.MaxRequestBytes, true);
        }

        internal string ReadSessionToken()
        {
            return ControlProtocol.DecodeSession(ReadLeaf("session.json", ControlProtocol.MaxSessionBytes, false));
        }

        private byte[] ReadLeaf(string name, int maximum, bool request)
        {
            if (disposed) throw new ObjectDisposedException("ControlFiles");
            string path = Path.Combine(root, name);
            SafeFileHandle handle = CreateFile(path, 0x80000000, request ? ShareRead | ShareWrite | ShareDelete : ShareRead,
                IntPtr.Zero, OpenExisting, OpenReparse, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error();
                handle.Dispose();
                if (request && (error == 2 || error == 3 || error == 32 || error == 33)) return null;
                throw new ControlFailure(request ? "request_unavailable" : "session_unavailable");
            }
            using (handle)
            {
                FileInformation info = Inspect(handle, false);
                ulong size = ((ulong)info.SizeHigh << 32) | info.SizeLow;
                if (size > (ulong)maximum) throw new ControlFailure(request ? "request_limit" : "invalid_session");
                using (FileStream stream = new FileStream(handle, FileAccess.Read))
                using (MemoryStream body = new MemoryStream())
                {
                    byte[] buffer = new byte[4096];
                    while (body.Length <= maximum)
                    {
                        int amount = Math.Min(buffer.Length, maximum + 1 - (int)body.Length);
                        int count = stream.Read(buffer, 0, amount);
                        if (count == 0)
                        {
                            if (request) LastRequestVersion = info.Volume.ToString("X8") + info.IndexHigh.ToString("X8") +
                                    info.IndexLow.ToString("X8") + info.Write.dwHighDateTime.ToString("X8") +
                                    info.Write.dwLowDateTime.ToString("X8");
                            return body.ToArray();
                        }
                        body.Write(buffer, 0, count);
                    }
                    throw new ControlFailure(request ? "request_limit" : "invalid_session");
                }
            }
        }

        internal void Publish(ControlResponse response)
        {
            if (disposed) throw new ObjectDisposedException("ControlFiles");
            byte[] body = ControlProtocol.Encode(response);
            string destination = Path.Combine(root, "response.json");
            string temporary = Path.Combine(root, "response_" + Guid.NewGuid().ToString("N") + ".tmp");
            bool owned = false;
            try
            {
                using (FileStream stream = new FileStream(temporary, FileMode.CreateNew, FileAccess.Write, FileShare.None))
                {
                    owned = true;
                    stream.Write(body, 0, body.Length);
                    stream.Flush();
                }
                SafeFileHandle existing = CreateFile(destination, 0x80, ShareRead | ShareWrite | ShareDelete,
                    IntPtr.Zero, OpenExisting, OpenReparse, IntPtr.Zero);
                bool present;
                using (existing)
                {
                    if (existing.IsInvalid)
                    {
                        int error = Marshal.GetLastWin32Error();
                        if (error != 2 && error != 3) throw new ControlFailure("response_unavailable");
                        present = false;
                    }
                    else { Inspect(existing, false); present = true; }
                }
                if (present) File.Replace(temporary, destination, null);
                else File.Move(temporary, destination);
                owned = false;
            }
            finally
            {
                if (owned) File.Delete(temporary);
            }
        }

        public void Dispose()
        {
            if (disposed) return;
            disposed = true;
            for (int i = pins.Count - 1; i >= 0; i--) pins[i].Dispose();
            pins.Clear();
        }
    }
}

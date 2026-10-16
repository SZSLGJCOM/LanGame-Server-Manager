using System;
using System.Collections.Generic;
using System.IO;
using Microsoft.Win32.SafeHandles;

namespace Lgsm.Valheim.Control
{
    // Admission covers only reads initiated by this controller. It cannot
    // bound native game RPCs or other plugins' independent list reads.
    internal sealed class NativeAccessPins : IDisposable
    {
        internal const int MaximumFileBytes = 512 * 1024;
        private readonly Dictionary<string, ControlFiles> directories =
            new Dictionary<string, ControlFiles>(StringComparer.OrdinalIgnoreCase);
        private readonly List<SafeFileHandle> files = new List<SafeFileHandle>();

        internal NativeAccessPins(string[] paths)
        {
            try
            {
                foreach (string path in paths)
                {
                    if (!ControlFiles.LocalAbsolute(path)) throw new ControlFailure("unsafe_native_access_path");
                    string full = Path.GetFullPath(path);
                    string directory = Path.GetDirectoryName(full);
                    if (!directories.ContainsKey(directory)) directories.Add(directory, new ControlFiles(directory));
                    files.Add(ControlFiles.PinBoundedFile(full, MaximumFileBytes));
                }
            }
            catch { Dispose(); throw; }
        }

        public void Dispose()
        {
            for (int i = files.Count - 1; i >= 0; i--) files[i].Dispose();
            files.Clear();
            foreach (ControlFiles directory in directories.Values) directory.Dispose();
            directories.Clear();
        }
    }
}

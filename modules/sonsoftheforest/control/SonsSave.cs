using System;
using System.Collections;
using System.Diagnostics;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using HarmonyLib;
using LanGame.SaveControl;
using Microsoft.Win32.SafeHandles;
using SonsSdk;
using SonsSdk.Attributes;

namespace LanGame.Sons
{
    public sealed class SaveControl : SonsMod, IOnUpdateReceiver
    {
        private SaveLoop control;
        private StartupSaveReadiness readiness;
        private Stopwatch readinessClock;
        private NativeBackupPatch backups;
        private NativeSaveCompletion completion;
        // Dedicated servers skip the title-scene SDK initialization coroutine.
        // Loader initialization runs in both hosts; game reflection is deferred
        // to readiness observation and requests on the main-thread update.
        protected override void OnInitializeMod()
        {
            string root = Environment.GetEnvironmentVariable("LGSM_SONS_SAVE_ROOT");
            if (String.IsNullOrEmpty(root)) return;
            string image = Process.GetCurrentProcess().MainModule.FileName;
            if (!String.Equals(Path.GetFileName(image), "SonsOfTheForestDS.exe", StringComparison.OrdinalIgnoreCase))
                throw new SaveFailure("wrong_host");
            string game = Path.GetDirectoryName(image);
            NativeBinding.VerifyFile(Path.Combine(game, "GameAssembly.dll"), "deadbef72344c5d19e2572e607f0c1ecbd0b85200ec9b98d5939753cde5929a2");
            NativeBinding.VerifyFile(Path.Combine(game, "SonsOfTheForestDS_Data/il2cpp_data/Metadata/global-metadata.dat"),
                "d9df15ce6470003dcffe1149a2d11625fa6b3feb7db509bcd65e3b268bfe448c");
            backups = NativeBackupPatch.Install(delegate(string marker) { Log(marker); });
            try
            {
                completion = NativeSaveCompletion.Install();
                control = new SaveLoop(root, "sonsoftheforest", new SaveAdapter(PrepareCompleted), delegate(string marker) { Log(marker); });
                Log("LGSM_SONS_SAVE_COMPLETION_READY");
            }
            catch
            {
                if (completion != null) completion.Dispose();
                completion = null; backups.Dispose(); backups = null; throw;
            }
            readiness = new StartupSaveReadiness(Prepare, delegate(string marker) { Log(marker); });
            readinessClock = Stopwatch.StartNew();
            Log("LanGame native save control initialized.");
        }
        private static Func<bool> Prepare()
        {
            Type autosave = NativeBinding.Type("Sons.Multiplayer", "Sons.Gameplay.DedicatedServerAutosave");
            Type unityObject = NativeBinding.Type("UnityEngine.CoreModule", "UnityEngine.Object");
            MethodInfo find = null;
            foreach (MethodInfo method in unityObject.GetMethods(BindingFlags.Public | BindingFlags.Static))
                if (method.Name == "FindObjectsOfType" && method.IsGenericMethodDefinition &&
                    method.GetGenericArguments().Length == 1 && method.GetParameters().Length == 0)
                { if (find != null) throw new SaveFailure("api_unavailable"); find = method; }
            if (find == null) throw new SaveFailure("api_unavailable");
            IEnumerable objects = find.MakeGenericMethod(autosave).Invoke(null, null) as IEnumerable;
            if (objects == null) throw new SaveFailure("api_unavailable");
            object owner = null;
            foreach (object item in objects)
            {
                if (owner != null || item == null || item.GetType() != autosave) throw new SaveFailure("world_ambiguous");
                owner = item;
            }
            if (owner == null) throw new SaveFailure("world_not_ready");
            MethodInfo ready = NativeBinding.Method(autosave, "IsReadyToSave", BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Instance, typeof(bool));
            if (!(bool)ready.Invoke(owner, null)) throw new SaveFailure("world_not_ready");
            MethodInfo save = NativeBinding.Method(autosave, "SaveImmediate", BindingFlags.Public | BindingFlags.Static, typeof(void));
            return delegate {
                if (!(bool)ready.Invoke(owner, null)) throw new SaveFailure("world_not_ready");
                save.Invoke(null, null);
                return true; // Native void return is a request receipt only.
            };
        }
        private Func<SaveReceipt> PrepareCompleted()
        {
            Func<bool> invoke = Prepare();
            return delegate {
                return NativeSaveCompletion.Invoke(completion.Begin(), delegate {
                    return invoke() ? SaveReceipt.Completed : SaveReceipt.Rejected;
                });
            };
        }
        public void OnUpdate()
        {
            if (control == null) return;
            try
            {
                readiness.Tick(readinessClock.ElapsedMilliseconds);
                control.Update();
            }
            catch
            {
                Log("LanGame save control disabled: invalid IPC state.");
                control.Dispose(); control = null;
            }
        }
        protected override void OnDeinitializeMod()
        {
            if (control != null) control.Dispose();
            control = null; readiness = null;
            if (completion != null) completion.Dispose();
            completion = null;
            if (backups != null) backups.Dispose();
            backups = null;
            if (readinessClock != null) readinessClock.Stop();
        }
    }

    // Patch this admitted game's backup boundary, so native automatic and exit
    // saves receive the same correction as manual saves. Serialization and the
    // primary SaveData.zip replacement remain entirely inside the native game.
    internal sealed class NativeBackupPatch : IDisposable
    {
        private const string Owner = "LanGame.Sons.Save.Backup";
        private static readonly object Gate = new object();
        private static NativeBackupPatch active;
        private readonly HarmonyLib.Harmony harmony = new HarmonyLib.Harmony(Owner);
        private readonly Action<string> log;
        private MethodInfo target;

        private NativeBackupPatch(Action<string> log) { this.log = log; }

        internal static NativeBackupPatch Install(Action<string> log)
        {
            lock (Gate)
            {
                if (active != null) throw new SaveFailure("backup_patch_conflict");
                NativeBackupPatch patch = new NativeBackupPatch(log);
                Type manager = NativeBinding.Type("Sons.Save", "Sons.Save.SaveGameManager");
                patch.target = NativeBinding.Method(manager, "BackupPreviousSave", BindingFlags.Public | BindingFlags.Static,
                    typeof(void), typeof(string), typeof(bool), typeof(bool));
                Patches previous = HarmonyLib.Harmony.GetPatchInfo(patch.target);
                if (previous != null && previous.Owners.Count != 0) throw new SaveFailure("backup_patch_conflict");
                MethodInfo prefix = typeof(NativeBackupPatch).GetMethod("Prefix", BindingFlags.NonPublic | BindingFlags.Static);
                active = patch;
                try
                {
                    patch.harmony.Patch(patch.target, new HarmonyMethod(prefix));
                    Patches installed = HarmonyLib.Harmony.GetPatchInfo(patch.target);
                    if (installed == null || installed.Prefixes.Count != 1 || installed.Prefixes[0].PatchMethod != prefix)
                        throw new SaveFailure("backup_patch_unavailable");
                    log("LGSM_SONS_BACKUP_PATCH_READY");
                    return patch;
                }
                catch
                {
                    patch.harmony.Unpatch(patch.target, HarmonyPatchType.All, Owner);
                    active = null;
                    throw;
                }
            }
        }

        private static bool Prefix(string __0, bool __1, bool __2)
        {
            // Corruption recovery has a different native prefix and lifecycle.
            // Preserve that operation as well as native empty/invalid-path logs.
            if (__1 || String.IsNullOrEmpty(__0) || !Directory.Exists(__0)) return true;
            lock (Gate)
            {
                if (active == null) return true;
                try
                {
                    if (Directory.GetFileSystemEntries(__0).Length == 0) return true;
                    BackupArchive.Create(__0, DateTime.Now, __2);
                    active.log("LGSM_SONS_BACKUP_PUBLISHED");
                }
                catch (Exception error)
                {
                    // The admitted native method catches backup exceptions and
                    // then continues its primary save. Preserve that behavior,
                    // exposing the failed recovery point without replaying save.
                    active.log("LGSM_SONS_BACKUP_FAILED issue=backup_io_failure hresult=" +
                        error.HResult.ToString("x8", CultureInfo.InvariantCulture));
                }
                return false;
            }
        }

        public void Dispose()
        {
            lock (Gate)
            {
                if (active != this) return;
                harmony.Unpatch(target, HarmonyPatchType.All, Owner);
                active = null;
                log("LGSM_SONS_BACKUP_PATCH_REMOVED");
            }
        }
    }

    internal static class BackupArchive
    {
        // Native recovery selects filenames by the slot's "_backup" prefix,
        // sorts them as strings, then selects the last. Keep that namespace,
        // real local time, ZIP suffix, and the enclosing slot directory entry.
        // The final four hex characters are an ordinal, not a future timestamp.
        internal static string Create(string slot, DateTime now, bool rotate)
        {
            string root = Path.GetFullPath(slot).TrimEnd('\\', '/');
            DirectoryInfo directory = new DirectoryInfo(root);
            if (directory.Parent == null) throw new IOException("Backup directory has no parent.");
            string parent = directory.Parent.FullName;
            string prefix = directory.Name + "_backup";
            string stem = prefix + "_" + now.ToString("yyyyMMddHHmmss", CultureInfo.InvariantCulture);
            string temporary = Path.Combine(parent, ".lgsm-backup-" + Guid.NewGuid().ToString("N") + ".partial");
            bool owned = false;
            byte[] digest;
            long length;
            using (SaveFiles parentPin = new SaveFiles(parent))
            {
                try
                {
                    using (FileStream output = new FileStream(temporary, FileMode.CreateNew, FileAccess.ReadWrite, FileShare.None))
                    {
                        owned = true;
                        using (ZipArchive archive = new ZipArchive(output, ZipArchiveMode.Create, true))
                            CopyDirectory(archive, root, directory.Name + "/");
                        output.Flush(true);
                        length = output.Length;
                        output.Position = 0;
                        using (SHA256 sha = SHA256.Create()) digest = sha.ComputeHash(output);
                    }
                    // Same-directory, no-overwrite publication: an existing ZIP
                    // is never truncated, and discovery never sees a partial ZIP.
                    string destination = Publish(temporary, parent, stem);
                    owned = false;
                    // A writable mod could replace the closed temporary or the
                    // published leaf. Verify the bytes produced by our original
                    // exclusive stream, and retain a no-write/no-delete handle
                    // throughout rotation before removing any older recovery.
                    using (FileStream published = OpenVerifiedBackup(destination, length, digest))
                        if (rotate) Rotate(parent, prefix, destination);
                    return destination;
                }
                catch (Exception error)
                {
                    if (owned)
                    {
                        try { DeletePinnedFile(temporary); }
                        catch (Exception cleanup) { throw new AggregateException("Backup creation and temporary cleanup failed.", error, cleanup); }
                    }
                    throw;
                }
            }
        }

        internal static FileStream OpenVerifiedBackup(string path, long length, byte[] digest)
        {
            FileStream file = SaveFiles.OpenPinnedFile(WindowsLeaf(path), Int64.MaxValue);
            try
            {
                using (SHA256 sha = SHA256.Create())
                    if (file.Length != length || Convert.ToBase64String(sha.ComputeHash(file)) != Convert.ToBase64String(digest))
                        throw new IOException("Published backup no longer matches the completed archive.");
                return file;
            }
            catch { file.Dispose(); throw; }
        }

        private static string WindowsLeaf(string path)
        {
            string full = Path.GetFullPath(path);
            return full.Length >= 260 && !full.StartsWith("\\\\?\\", StringComparison.Ordinal) ? "\\\\?\\" + full : full;
        }

        internal static string Publish(string temporary, string parent, string stem)
        {
            int next = 0;
            foreach (string path in Directory.GetFiles(parent))
            {
                string name = Path.GetFileName(path);
                if (name.Length == stem.Length + 8 && name.StartsWith(stem, StringComparison.Ordinal) &&
                    name.EndsWith(".zip", StringComparison.Ordinal) && Digits(name, stem.Length, 4, true))
                    next = Math.Max(next, Int32.Parse(name.Substring(stem.Length, 4), NumberStyles.HexNumber, CultureInfo.InvariantCulture) + 1);
            }
            // This scan chooses a candidate, not permission to overwrite. Only
            // the atomic no-overwrite move establishes ownership. A confirmed
            // name collision allocates another name for this same completed ZIP;
            // no native save or serialization is repeated. Other I/O errors stop.
            for (int attempt = 0; attempt < 4096 && next <= 0xffff; attempt++, next++)
            {
                string destination = Path.Combine(parent, stem + next.ToString("x4", CultureInfo.InvariantCulture) + ".zip");
                try { File.Move(temporary, destination); return destination; }
                catch (IOException error)
                {
                    int code = error.HResult & 0xffff;
                    if (code != 80 && code != 183) throw;
                }
            }
            throw new IOException("Backup ordinal allocation exhausted its bounded name space.");
        }

        private static void CopyDirectory(ZipArchive archive, string path, string entryPath)
        {
            using (SaveFiles directoryPin = new SaveFiles(path))
            {
                string[] entries = Directory.GetFileSystemEntries(path);
                if (entries.Length == 0) archive.CreateEntry(entryPath);
                foreach (string entry in entries)
                {
                    string leaf = Path.GetFileName(entry);
                    if ((File.GetAttributes(entry) & FileAttributes.Directory) != 0)
                        CopyDirectory(archive, entry, entryPath + leaf + "/");
                    else
                    {
                        using (FileStream input = SaveFiles.OpenPinnedFile(WindowsLeaf(entry), Int64.MaxValue))
                        {
                            ZipArchiveEntry item = archive.CreateEntry(entryPath + leaf, CompressionLevel.Optimal);
                            using (Stream output = item.Open()) input.CopyTo(output);
                        }
                    }
                }
            }
        }

        private static bool Digits(string name, int start, int count, bool hex)
        {
            for (int i = start; i < start + count; i++)
                if (!((name[i] >= '0' && name[i] <= '9') || (hex && name[i] >= 'a' && name[i] <= 'f'))) return false;
            return true;
        }

        private static bool KnownBackup(string name, string prefix)
        {
            string stem = prefix + "_";
            if (!name.StartsWith(stem, StringComparison.Ordinal) || !name.EndsWith(".zip", StringComparison.Ordinal)) return false;
            int length = name.Length - stem.Length - 4;
            if (length == 14) return Digits(name, stem.Length, 14, false);
            return length == 18 && Digits(name, stem.Length, 14, false) && Digits(name, stem.Length + 14, 4, true);
        }

        private static void Rotate(string parent, string prefix, string published)
        {
            System.Collections.Generic.List<string> files = new System.Collections.Generic.List<string>();
            foreach (string path in Directory.GetFiles(parent))
                if (!String.Equals(path, published, StringComparison.OrdinalIgnoreCase) && KnownBackup(Path.GetFileName(path), prefix)) files.Add(path);
            files.Sort(StringComparer.CurrentCulture); // The admitted native Array.Sort<string> order.
            // Always retain the just-published recovery point, including after
            // a system clock rollback; it contains the primary about to change.
            for (int i = 0; i < files.Count - 2; i++) DeletePinnedFile(files[i]);
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct AttributeTag { internal uint Attributes, Tag; }
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern SafeFileHandle CreateFile(string path, uint access, uint sharing, IntPtr security, uint creation, uint flags, IntPtr template);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool GetFileInformationByHandleEx(SafeFileHandle handle, int kind, out AttributeTag info, uint size);
        [DllImport("kernel32.dll", SetLastError = true)]
        private static extern bool SetFileInformationByHandle(SafeFileHandle handle, int kind, ref int value, uint size);

        private static void DeletePinnedFile(string path)
        {
            // Inspect and delete the same leaf handle. Never follow a reparse
            // point or delete a replacement leaf after a path-only inspection.
            using (SafeFileHandle handle = CreateFile(WindowsLeaf(path), 0x10080, 3, IntPtr.Zero, 3, 0x200000, IntPtr.Zero))
            {
                AttributeTag info;
                if (handle.IsInvalid || !GetFileInformationByHandleEx(handle, 9, out info, 8) || (info.Attributes & 0x410) != 0)
                    throw new IOException("Backup cleanup could not pin an ordinary file.");
                int remove = 1;
                if (!SetFileInformationByHandle(handle, 4, ref remove, 4)) throw new IOException("Backup cleanup failed.");
            }
        }
    }

    // Startup evidence only: validate the exact production Prepare path without
    // ever invoking the returned native save delegate. The owner thread stops
    // scanning after the first ready result, a terminal API failure or timeout.
    internal sealed class StartupSaveReadiness
    {
        internal const string ReadyMarker = "LGSM_SONS_SAVE_READY";
        internal const long BudgetMilliseconds = 600000;
        private readonly Func<Func<bool>> prepare;
        private readonly Action<string> log;
        private long next;
        private bool done;

        internal StartupSaveReadiness(Func<Func<bool>> prepare, Action<string> log)
        {
            this.prepare = prepare;
            this.log = log;
        }

        internal void Tick(long elapsed)
        {
            if (done || elapsed < next) return;
            if (elapsed >= BudgetMilliseconds)
            {
                done = true;
                log("LGSM_SONS_SAVE_UNAVAILABLE issue=startup_readiness_timeout");
                return;
            }
            next = elapsed + 1000;
            string marker;
            try
            {
                if (prepare() == null) throw new SaveFailure("api_unavailable");
                marker = ReadyMarker;
            }
            catch (SaveFailure error)
            {
                if (error.Code == "world_not_ready") return;
                marker = "LGSM_SONS_SAVE_UNAVAILABLE issue=" + error.Code;
            }
            catch (Exception)
            {
                marker = "LGSM_SONS_SAVE_UNAVAILABLE issue=api_unavailable";
            }
            done = true;
            log(marker);
        }
    }
}

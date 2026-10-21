using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Security.Cryptography;
using System.Threading;
using HarmonyLib;
using LanGame.SaveControl;

namespace LanGame.Sons
{
    // Real filesystem/ZIP and Harmony lifecycle tests. Sons.Save is the only
    // boundary double; these tests do not load Unity or claim native acceptance.
    internal static class SonsBackupTests
    {
        private static readonly DateTime SameSecond = new DateTime(2026, 10, 20, 1, 2, 3, 456);
        private static int passed;

        private static void Assert(bool value, string message)
        {
            if (!value) throw new Exception(message);
        }

        private static string Primary(string slot) { return Path.Combine(slot, "SaveData.zip"); }
        private static string[] Backups(string slot) { return Directory.GetFiles(Path.GetDirectoryName(slot), Path.GetFileName(slot) + "_backup*.zip"); }
        private static void Check(bool value, string message) { Assert(value, message); passed++; }
        private static string ReadPrimary(string archivePath, string slot)
        {
            using (ZipArchive zip = ZipFile.OpenRead(archivePath))
            {
                ZipArchiveEntry item = zip.GetEntry(Path.GetFileName(slot) + "/SaveData.zip");
                Assert(item != null, "Native recovery's enclosing slot entry is missing");
                using (StreamReader reader = new StreamReader(item.Open())) return reader.ReadToEnd();
            }
        }

        private static void FilesystemCases(string root)
        {
            string slot = Path.Combine(root, "fs", "0000000001");
            Directory.CreateDirectory(slot);
            DateTime time = SameSecond;
            File.WriteAllText(Primary(slot), "before-first-save");
            string first = BackupArchive.Create(slot, time, true);
            File.WriteAllText(Primary(slot), "before-second-save");
            string second = BackupArchive.Create(slot, time, true);
            Check(first != second && ReadPrimary(first, slot) == "before-first-save" && ReadPrimary(second, slot) == "before-second-save",
                "Same-second backup creation collided, overwrote a backup, or copied the wrong primary");

            File.WriteAllText(Primary(slot), "must-remain-primary");
            string allocation = Path.Combine(root, "allocation");
            Directory.CreateDirectory(allocation);
            string stem = "0000000009_backup_20261020010203";
            string existing = Path.Combine(allocation, stem + "0000.zip"), temporary = Path.Combine(allocation, "candidate.partial");
            File.WriteAllText(existing, "existing recovery point");
            File.WriteAllText(temporary, "one completed archive");
            string allocated = BackupArchive.Publish(temporary, allocation, stem);
            Check(allocated == Path.Combine(allocation, stem + "0001.zip") && File.ReadAllText(existing) == "existing recovery point" &&
                File.ReadAllText(allocated) == "one completed archive" && !File.Exists(temporary) && Backups(slot).Length == 2 &&
                ReadPrimary(second, slot) == "before-second-save" &&
                File.ReadAllText(Primary(slot)) == "must-remain-primary" && Directory.GetFiles(Path.GetDirectoryName(slot), "*.partial").Length == 0,
                "Ordinal allocation changed an existing backup, primary, or left its completed temporary");

            string exhausted = Path.Combine(allocation, stem + "ffff.zip");
            File.WriteAllText(exhausted, "last ordinal remains intact");
            File.WriteAllText(temporary, "unpublished completed archive");
            bool bounded = false;
            try { BackupArchive.Publish(temporary, allocation, stem); }
            catch (IOException) { bounded = true; }
            Check(bounded && File.ReadAllText(exhausted) == "last ordinal remains intact" &&
                File.ReadAllText(temporary) == "unpublished completed archive", "Ordinal allocation wrapped or overwrote an existing archive");
            bool missing = false;
            try { BackupArchive.Publish(Path.Combine(allocation, "absent.partial"), allocation, stem + "another"); }
            catch (IOException error) { missing = (error.HResult & 0xffff) == 2; }
            Check(missing && Directory.GetFiles(allocation).Length == 4, "A non-collision I/O failure was hidden or changed existing files");

            string native = Path.Combine(Path.GetDirectoryName(slot), "0000000001_backup_20261020010203.zip");
            File.Copy(first, native);
            string[] candidates = Backups(slot);
            foreach (string culture in new[] { "", "zh-CN", "en-US" })
            {
                CultureInfo old = Thread.CurrentThread.CurrentCulture;
                try
                {
                    Thread.CurrentThread.CurrentCulture = CultureInfo.GetCultureInfo(culture);
                    Array.Sort(candidates);
                    Check(candidates[candidates.Length - 1] == second, "Native string sorting did not select the latest same-second backup");
                    string ordinalStem = "0000000001_backup_20261020010203";
                    string[] hexadecimal = new[] { "0009", "000a", "000f", "0010", "0fff", "1000", "a000", "ffff" };
                    string[] mixed = new string[hexadecimal.Length + 1];
                    mixed[0] = ordinalStem + ".zip";
                    for (int i = 0; i < hexadecimal.Length; i++) mixed[i + 1] = ordinalStem + hexadecimal[i] + ".zip";
                    Array.Sort(mixed);
                    Check(mixed[0] == ordinalStem + ".zip" && mixed[mixed.Length - 1] == ordinalStem + "ffff.zip",
                        "Native sorting failed across hexadecimal ordinal transitions");
                }
                finally { Thread.CurrentThread.CurrentCulture = old; }
            }

            string foreign = Path.Combine(Path.GetDirectoryName(slot), "0000000001_backup_external.zip");
            File.WriteAllText(foreign, "external backup remains owned by its creator");
            string third = BackupArchive.Create(slot, time.AddTicks(1), true);
            Check(!File.Exists(native) && File.Exists(first) && File.Exists(second) && File.Exists(third) &&
                File.ReadAllText(foreign) == "external backup remains owned by its creator",
                "Retention did not keep three recognized backups or touched an unknown external file");

            bool failed = false;
            using (FileStream held = new FileStream(Primary(slot), FileMode.Open, FileAccess.Read, FileShare.None))
            {
                try { BackupArchive.Create(slot, time.AddTicks(2), true); }
                catch (SaveFailure) { failed = true; }
            }
            Check(failed && File.Exists(first) && File.Exists(second) && File.Exists(third) && Backups(slot).Length == 4 &&
                Directory.GetFiles(Path.GetDirectoryName(slot), "*.partial").Length == 0,
                "Failed backup creation rotated away a recovery point or leaked its partial archive");

            failed = false;
            using (FileStream held = new FileStream(first, FileMode.Open, FileAccess.Read, FileShare.Read))
            {
                try { BackupArchive.Create(slot, time.AddTicks(3), true); }
                catch (IOException) { failed = true; }
            }
            Check(failed && Backups(slot).Length == 5 && File.Exists(first) &&
                Directory.GetFiles(Path.GetDirectoryName(slot), "*.partial").Length == 0,
                "Retention failure hid the newly completed recovery point or removed an existing one");

            string rollback = BackupArchive.Create(slot, time.AddDays(-1), true);
            Check(File.Exists(rollback) && ReadPrimary(rollback, slot) == "must-remain-primary" && Backups(slot).Length == 4,
                "Clock rollback caused retention to delete the primary's newly published recovery point");

            byte[] correct = File.ReadAllBytes(rollback), digest;
            using (SHA256 sha = SHA256.Create()) digest = sha.ComputeHash(correct);
            using (FileStream pinned = BackupArchive.OpenVerifiedBackup(rollback, correct.Length, digest))
            {
                bool blockedWrite = false, blockedDelete = false;
                try { File.WriteAllBytes(rollback, correct); } catch (IOException) { blockedWrite = true; }
                try { File.Delete(rollback); } catch (IOException) { blockedDelete = true; }
                Check(blockedWrite && blockedDelete, "Verified publication was writable or deletable during retention");
            }
            correct[correct.Length - 1] ^= 1;
            File.WriteAllBytes(rollback, correct);
            bool changed = false;
            try { using (FileStream pin = BackupArchive.OpenVerifiedBackup(rollback, correct.Length, digest)) { } }
            catch (IOException) { changed = true; }
            Check(changed && Backups(slot).Length == 4, "Same-length replacement crossed the completed-archive identity check");

            string longDirectory = Path.Combine(root, "long-leaf");
            Directory.CreateDirectory(longDirectory);
            string longLeaf = Path.Combine(longDirectory, new string('x', Math.Max(100, 265 - longDirectory.Length)) + ".zip");
            byte[] longBytes = new byte[] { 1, 3, 5, 7 }, longDigest;
            File.WriteAllBytes(longLeaf, longBytes);
            using (SHA256 sha = SHA256.Create()) longDigest = sha.ComputeHash(longBytes);
            using (FileStream pinned = BackupArchive.OpenVerifiedBackup(longLeaf, longBytes.Length, longDigest))
                Check(longLeaf.Length > 260 && pinned.Length == longBytes.Length, "A local Win32 leaf over MAX_PATH lost its verified pin");
        }

        private static void ConcurrentPublication(string root)
        {
            string slot = Path.Combine(root, "parallel", "0000000003");
            Directory.CreateDirectory(slot);
            File.WriteAllText(Primary(slot), "same native primary");
            Exception failure = null;
            Thread[] threads = new Thread[8];
            using (ManualResetEvent start = new ManualResetEvent(false))
            {
                for (int i = 0; i < threads.Length; i++)
                {
                    threads[i] = new Thread(delegate() {
                        start.WaitOne();
                        try { BackupArchive.Create(slot, SameSecond, false); }
                        catch (Exception error) { Interlocked.CompareExchange(ref failure, error, null); }
                    });
                    threads[i].Start();
                }
                start.Set();
                foreach (Thread thread in threads) Assert(thread.Join(10000), "Concurrent archive publication did not finish");
            }
            Check(failure == null && Backups(slot).Length == threads.Length && Directory.GetFiles(Path.GetDirectoryName(slot), "*.partial").Length == 0,
                "Atomic same-second allocation collided under independent concurrent publishers");
            foreach (string backup in Backups(slot)) Assert(ReadPrimary(backup, slot) == "same native primary", "A concurrent archive is incomplete");
        }

        private static bool ForeignPrefix() { return true; }

        private static void HarmonyCases(string root)
        {
            Type manager = Type.GetType("Sons.Save.SaveGameManager, Sons.Save", true);
            MethodInfo native = manager.GetMethod("BackupPreviousSave");
            FieldInfo calls = manager.GetField("Calls");
            HarmonyLib.Harmony foreign = new HarmonyLib.Harmony("LanGame.Sons.Tests.Foreign");
            foreign.Patch(native, new HarmonyMethod(typeof(SonsBackupTests).GetMethod("ForeignPrefix", BindingFlags.NonPublic | BindingFlags.Static)));
            bool rejected = false;
            try { NativeBackupPatch.Install(delegate { }); }
            catch (SaveFailure error) { rejected = error.Code == "backup_patch_conflict"; }
            finally { foreign.Unpatch(native, HarmonyPatchType.All, foreign.Id); }
            Check(rejected && HarmonyLib.Harmony.GetPatchInfo(native).Owners.Count == 0, "An unknown patch was adopted or removed");

            string slot = Path.Combine(root, "hook", "0000000002");
            Directory.CreateDirectory(slot);
            File.WriteAllText(Primary(slot), "current native primary");
            List<string> logs = new List<string>();
            using (NativeBackupPatch patch = NativeBackupPatch.Install(logs.Add))
            {
                native.Invoke(null, new object[] { slot, false, true });
                native.Invoke(null, new object[] { slot, false, true });
                Check((int)calls.GetValue(null) == 0 && Backups(slot).Length == 2 &&
                    logs.FindAll(delegate(string item) { return item == "LGSM_SONS_BACKUP_PUBLISHED"; }).Count == 2,
                    "The exact native backup boundary was not intercepted once for each call");

                native.Invoke(null, new object[] { slot, true, true });
                native.Invoke(null, new object[] { Path.Combine(root, "not-created"), false, true });
                Check((int)calls.GetValue(null) == 2, "Corruption recovery or absent-path native behavior changed");

                using (FileStream held = new FileStream(Primary(slot), FileMode.Open, FileAccess.Read, FileShare.None))
                    native.Invoke(null, new object[] { slot, false, true });
                Check((int)calls.GetValue(null) == 2 && Backups(slot).Length == 2 &&
                    logs.Exists(delegate(string item) { return item.StartsWith("LGSM_SONS_BACKUP_FAILED issue=backup_io_failure hresult=", StringComparison.Ordinal); }),
                    "A backup failure escaped into the primary save, replayed native saving, or disappeared");

                Exception threadError = null;
                Thread[] threads = new Thread[3];
                for (int i = 0; i < threads.Length; i++)
                {
                    threads[i] = new Thread(delegate() {
                        try { native.Invoke(null, new object[] { slot, false, true }); }
                        catch (Exception error) { Interlocked.CompareExchange(ref threadError, error, null); }
                    });
                    threads[i].Start();
                }
                foreach (Thread thread in threads) Assert(thread.Join(10000), "Backup callback did not finish");
                Check(threadError == null && (int)calls.GetValue(null) == 2 && Backups(slot).Length == 3 &&
                    logs.FindAll(delegate(string item) { return item == "LGSM_SONS_BACKUP_PUBLISHED"; }).Count == 5,
                    "Concurrent native callers bypassed serialization, collided, or broke retention");
            }
            native.Invoke(null, new object[] { slot, false, true });
            Check((int)calls.GetValue(null) == 3 && HarmonyLib.Harmony.GetPatchInfo(native).Owners.Count == 0,
                "The mod's backup patch survived its owning lifecycle");
        }

        private static void Main(string[] args)
        {
            string parent = Path.GetFullPath(args[0]);
            string root = Path.Combine(parent, "sons-backup-test-" + Guid.NewGuid().ToString("N"));
            Assert(!Directory.Exists(root), "Test fixture must be exclusively new");
            Directory.CreateDirectory(root);
            try
            {
                FilesystemCases(root);
                ConcurrentPublication(root);
                HarmonyCases(root);
                Console.WriteLine("Sons backup regression tests: " + passed + " passed; native game not started");
            }
            finally
            {
                Assert(Path.GetDirectoryName(root) == parent, "Test cleanup escaped its reserved parent");
                Directory.Delete(root, true);
            }
        }
    }
}

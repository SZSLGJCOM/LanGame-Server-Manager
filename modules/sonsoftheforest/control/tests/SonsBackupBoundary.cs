using System.Runtime.CompilerServices;

// Test-only replacement for the external IL2CPP method. Never compiled into or
// installed with the game plugin; the backup/ZIP/retention code stays real.
namespace Sons.Save
{
    public sealed class SaveGameManager
    {
        public static int Calls;
        public static int SaveMode;
        public static string Temporary;
        public static bool FailRegisterAfter, FailUnregisterAfter;
        public static readonly System.Collections.Generic.List<Il2CppSystem.Action> BeforeCallbacks = new System.Collections.Generic.List<Il2CppSystem.Action>();
        public static readonly System.Collections.Generic.List<Il2CppSystem.Action<bool>> AfterCallbacks = new System.Collections.Generic.List<Il2CppSystem.Action<bool>>();
        public static void RegisterBeforeSaveCallback(Il2CppSystem.Action callback) { BeforeCallbacks.Add(callback); }
        public static void UnregisterBeforeSaveCallback(Il2CppSystem.Action callback)
        {
            if (!callback.NativeCompatible) throw new System.InvalidOperationException("native reflected target/method mismatch");
            BeforeCallbacks.Remove(callback);
        }
        public static void RegisterAfterSaveCallback(Il2CppSystem.Action<bool> callback)
        {
            if (FailRegisterAfter) throw new System.IO.IOException("native registry unavailable");
            AfterCallbacks.Add(callback);
        }
        public static void UnregisterAfterSaveCallback(Il2CppSystem.Action<bool> callback)
        {
            if (!callback.NativeCompatible) throw new System.InvalidOperationException("native reflected target/method mismatch");
            if (FailUnregisterAfter) throw new System.IO.IOException("native registry cleanup failed");
            AfterCallbacks.Remove(callback);
        }
        public static void BeforeSave() { foreach (var callback in BeforeCallbacks.ToArray()) callback.Invoke(); }
        public static void AfterSave(bool playerOnly) { foreach (var callback in AfterCallbacks.ToArray()) callback.Invoke(playerOnly); }
        public static void SaveToSlot(string dir)
        {
            BeforeSave();
            if (SaveMode == 5)
            {
                // Fixed loader's outer native->managed trampoline records and
                // swallows the original write exception before returning.
                try { WriteJsonZip(dir, null, null, false); }
                catch (System.IO.IOException nativeError)
                {
                    // Runtime RaiseExceptionIfNecessary turns the native
                    // exception and its native stack into a CLR Il2CppException.
                    try { throw new Il2CppInterop.Runtime.Il2CppException(nativeError.ToString()); }
                    catch (Il2CppInterop.Runtime.Il2CppException) { return; }
                }
            }
            WriteJsonZip(dir, null, null, false);
            AfterSave(false);
        }

        [MethodImpl(MethodImplOptions.NoInlining)]
        public static void BackupPreviousSave(string backupFolderPath, bool markCorrupted, bool deleteOldBackups)
        {
            Calls++;
        }

        [MethodImpl(MethodImplOptions.NoInlining)]
        public static void WriteJsonZip(string dir, SaveGameManager instance, string gameName, bool savePlayerOnly)
        {
            if (SaveMode == 1) return;
            string destination = System.IO.Path.Combine(dir, SaveMode == 3 ? "Other.zip" : "SaveData.zip");
            if (SaveMode == 2)
            {
                Il2CppSystem.IO.File.Copy(Temporary + ".missing", destination, true);
                return;
            }
            Il2CppSystem.IO.File.Copy(Temporary, destination, true);
            if (SaveMode == 4) Il2CppSystem.IO.File.Copy(Temporary, destination, true);
        }
    }
}

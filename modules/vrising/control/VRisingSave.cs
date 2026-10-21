using System;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using BepInEx;
using BepInEx.Unity.IL2CPP;
using HarmonyLib;
using LanGame.SaveControl;

namespace LanGame.VRising
{
    [BepInPlugin("local.langame.vrising.save", "LanGame Save Control", "0.1.0")]
    public sealed class SaveControl : BasePlugin
    {
        private static SaveControl current;
        private SaveLoop control;
        private NativeSaveCompletion completion;
        private static object bootstrap;
        private HarmonyLib.Harmony updateHook;
        public override void Load()
        {
            string root = Environment.GetEnvironmentVariable("LGSM_VRISING_SAVE_ROOT");
            if (String.IsNullOrEmpty(root)) return;
            string image = Process.GetCurrentProcess().MainModule.FileName;
            if (!String.Equals(Path.GetFileName(image), "VRisingServer.exe", StringComparison.OrdinalIgnoreCase))
                throw new SaveFailure("wrong_host");
            string game = Path.GetDirectoryName(image);
            NativeBinding.VerifyFile(Path.Combine(game, "GameAssembly.dll"), "ff366577fb0c707c42b8bde242304653aaa37f6ff0665ab9520150ceb692aed1");
            NativeBinding.VerifyFile(Path.Combine(game, "VRisingServer_Data/il2cpp_data/Metadata/global-metadata.dat"),
                "58d3cea9b9b546ad3d734ba2a7296ac3c9820491eec7bb108a03cfbcb1065f85");
            Type type = NativeBinding.Type("ProjectM", "ProjectM.ServerBootstrapSystem");
            MethodInfo update = NativeBinding.Method(type, "OnUpdate", BindingFlags.NonPublic | BindingFlags.Public | BindingFlags.Instance, typeof(void));
            try
            {
                completion = new NativeSaveCompletion(delegate(string line) { Log.LogError(line); });
                control = new SaveLoop(root, "vrising", new SaveAdapter(Prepare, completion.Observe, completion.Abandon, completion.Issue));
                current = this;
                updateHook = new HarmonyLib.Harmony("local.langame.vrising.save");
                updateHook.Patch(update,
                    postfix: new HarmonyMethod(typeof(SaveControl).GetMethod("AfterUpdate", BindingFlags.NonPublic | BindingFlags.Static)));
            }
            catch { if (control != null) control.Dispose(); if (updateHook != null) updateHook.UnpatchSelf(); current = null; throw; }
        }
        private static void AfterUpdate(object __instance)
        {
            if (current == null || current.control == null) return;
            bootstrap = __instance;
            try { current.control.Update(); }
            catch
            {
                current.Log.LogError("LanGame save control disabled: invalid IPC state.");
                current.control.Dispose(); current.control = null;
            }
            finally { bootstrap = null; }
        }
        private static Func<SaveReceipt> Prepare()
        {
            object server = bootstrap;
            Type type = NativeBinding.Type("ProjectM", "ProjectM.ServerBootstrapSystem");
            if (server == null || server.GetType() != type) throw new SaveFailure("world_not_ready");
            PropertyInfo initialized = type.GetProperty("ServerIsInitialized", BindingFlags.Public | BindingFlags.Instance);
            PropertyInfo worldProperty = type.GetProperty("World", BindingFlags.Public | BindingFlags.Instance);
            if (initialized == null || initialized.PropertyType != typeof(bool) || worldProperty == null ||
                !(bool)initialized.GetValue(server, null)) throw new SaveFailure("world_not_ready");
            object world = worldProperty.GetValue(server, null);
            if (world == null) throw new SaveFailure("world_not_ready");
            PropertyInfo worldCreated = world.GetType().GetProperty("IsCreated", BindingFlags.Public | BindingFlags.Instance);
            PropertyInfo worldSequence = world.GetType().GetProperty("SequenceNumber", BindingFlags.Public | BindingFlags.Instance);
            if (worldCreated == null || worldCreated.PropertyType != typeof(bool) ||
                worldSequence == null || worldSequence.PropertyType != typeof(ulong) || !(bool)worldCreated.GetValue(world, null))
                throw new SaveFailure("world_not_ready");
            ulong sequence = (ulong)worldSequence.GetValue(world, null);
            Type runtimeType = NativeBinding.Type("ProjectM.Shared", "ProjectM.ServerRuntimeSettings");
            MethodInfo settingsMethod = NativeBinding.Method(type, "GetServerRuntimeSettings", BindingFlags.Public | BindingFlags.Instance, runtimeType);
            object settings = settingsMethod.Invoke(server, null);
            Type saveType = NativeBinding.Type("ProjectM", "ProjectM.TriggerPersistenceSaveSystem");
            MethodInfo getSystem = null;
            foreach (MethodInfo method in world.GetType().GetMethods(BindingFlags.Public | BindingFlags.Instance))
                if (method.Name == "GetExistingSystemManaged" && method.IsGenericMethodDefinition &&
                    method.GetGenericArguments().Length == 1 && method.GetParameters().Length == 0)
                { if (getSystem != null) throw new SaveFailure("api_unavailable"); getSystem = method; }
            if (getSystem == null) throw new SaveFailure("api_unavailable");
            object saves = getSystem.MakeGenericMethod(saveType).Invoke(world, null);
            if (saves == null) throw new SaveFailure("world_not_ready");
            Type serializerType = NativeBinding.Type("ProjectM", "ProjectM.SerializePersistenceSystemV2");
            object serializer = getSystem.MakeGenericMethod(serializerType).Invoke(world, null);
            if (serializer == null) throw new SaveFailure("world_not_ready");
            PropertyInfo saving = serializerType.GetProperty("IsSaving", BindingFlags.Public | BindingFlags.Instance);
            if (saving == null || saving.PropertyType != typeof(bool) || (bool)saving.GetValue(serializer, null))
                throw new SaveFailure("save_busy");
            MethodInfo ready = NativeBinding.Method(saveType, "IsReadyToSave", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance, typeof(bool), runtimeType);
            if (!(bool)ready.Invoke(saves, new[] { settings })) throw new SaveFailure("save_busy");
            MethodInfo save = NativeBinding.Method(saveType, "TriggerAutoSave", BindingFlags.Public | BindingFlags.Instance, typeof(bool), runtimeType);
            return delegate {
                if (NativeSaveCompletion.Pointer(server) != NativeSaveCompletion.Pointer(bootstrap) || !(bool)initialized.GetValue(server, null))
                    throw new SaveFailure("target_changed");
                NativeSaveCompletion observer = current == null ? null : current.completion;
                if (observer == null) throw new SaveFailure("api_unavailable");
                IntPtr worldPointer = NativeSaveCompletion.Pointer(world);
                return observer.Start(saves, serializer,
                    delegate { return (bool)save.Invoke(saves, new[] { settings }); },
                    delegate {
                        object activeWorld = worldProperty.GetValue(server, null);
                        return NativeSaveCompletion.Pointer(server) == NativeSaveCompletion.Pointer(bootstrap) && (bool)initialized.GetValue(server, null) &&
                            NativeSaveCompletion.Pointer(activeWorld) == worldPointer && (bool)worldCreated.GetValue(activeWorld, null) &&
                            (ulong)worldSequence.GetValue(activeWorld, null) == sequence &&
                            NativeSaveCompletion.Pointer(getSystem.MakeGenericMethod(saveType).Invoke(activeWorld, null)) == NativeSaveCompletion.Pointer(saves) &&
                            NativeSaveCompletion.Pointer(getSystem.MakeGenericMethod(serializerType).Invoke(activeWorld, null)) == NativeSaveCompletion.Pointer(serializer);
                    });
            };
        }
        public override bool Unload()
        {
            if (control != null) control.Dispose(); control = null;
            if (completion != null) completion.Abandon(); completion = null;
            if (updateHook != null) updateHook.UnpatchSelf(); updateHook = null;
            current = null; bootstrap = null;
            return true;
        }
    }
}

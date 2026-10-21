using System;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    public sealed class SaveControl : PugMod.IMod
    {
        private SaveLoop control;
        private CoreSaveCompletion completion;
        private CoreSaveHooks earlyHooks;
        public SaveControl()
        {
            if (String.IsNullOrEmpty(Environment.GetEnvironmentVariable("LGSM_COREKEEPER_SAVE_ROOT"))) return;
            Type manager = NativeBinding.Type("Pug.Other", "Manager");
            NativeBinding.VerifyFile(manager.Assembly.Location,
                "042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938");
            // The native SideLoader constructs Mods while Manager.EarlyInit is
            // still running. Waiting for IMod.Init can miss the long-lived loop.
            earlyHooks = new CoreSaveHooks();
        }
        public void EarlyInit() { }
        public void Init()
        {
            string root = Environment.GetEnvironmentVariable("LGSM_COREKEEPER_SAVE_ROOT");
            if (String.IsNullOrEmpty(root)) return;
            Type manager = NativeBinding.Type("Pug.Other", "Manager");
            NativeBinding.VerifyFile(manager.Assembly.Location,
                "042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938");
            if (earlyHooks == null) throw new SaveFailure("api_unavailable");
            completion = new CoreSaveCompletion(earlyHooks); earlyHooks = null;
            try { control = new SaveLoop(root, "corekeeper", new SaveAdapter(completion.Prepare, completion.Observe, completion.Abandon)); }
            catch { completion.Dispose(); completion = null; throw; }
        }
        public void Update()
        {
            if (control == null) return;
            try { control.Update(); }
            catch
            {
                UnityEngine.Debug.LogError("LanGame save control disabled: invalid IPC state.");
                Shutdown();
            }
        }
        public void Shutdown()
        {
            try { if (control != null) control.Dispose(); }
            finally
            {
                control = null;
                try { if (completion != null) completion.Dispose(); }
                finally
                {
                    completion = null;
                    try { if (earlyHooks != null) earlyHooks.Dispose(); }
                    finally { earlyHooks = null; }
                }
            }
        }
        public void ModObjectLoaded(UnityEngine.Object value) { }
        public bool CanBeUnloaded() { return control == null && completion == null && earlyHooks == null; }
    }
}

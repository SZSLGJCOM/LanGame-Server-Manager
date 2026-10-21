using System;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    public sealed class SaveControl : PugMod.IMod
    {
        private SaveLoop control;
        private CoreSaveTarget active;
        private string entitiesHash, componentsHash;
        public void EarlyInit() { }
        public void Init()
        {
            string root = Environment.GetEnvironmentVariable("LGSM_COREKEEPER_SAVE_ROOT");
            if (String.IsNullOrEmpty(root)) return;
            Type manager = NativeBinding.Type("Pug.Other", "Manager");
            string sdkHash;
            try
            {
                NativeBinding.VerifyFile(manager.Assembly.Location,
                    "631b58cfb806e838bc0c770963b3ec13edeba368007e2ad91e280bd7bf8ad2b0");
                entitiesHash = "e4f03d5dcc4547f2730218fd128e918a26d24590f31b827d334c09a0de2e762a";
                componentsHash = "c2822ba48eec76430167963ddfd885eb2d5225cf5193850f1833583a27401d0f";
                sdkHash = "ed2157a69174d6e85331d2744e3e3c9c43fa7e1e69be344aac1158c3f711ec5d";
            }
            catch (SaveFailure failure)
            {
                if (failure.Code != "unsupported_native_build") throw;
                NativeBinding.VerifyFile(manager.Assembly.Location,
                    "042b6db4b62a34c5354b19e478302b723b3c89a4e6c6912ffd0ce91c78311938");
                entitiesHash = "d73687d44b81ceccb1f0416cd373d75e2dc58d8d998ed26310e31f48650a6c1d";
                componentsHash = "564660969f1c3630fbe6e133e118ced32906682bc8b53839a139211cc4fd177f";
                sdkHash = "6f1bd3ea3c46c7e6392c42f27bca6baf1bfb13a4eaa89756941eb51e193e7664";
            }
            NativeBinding.VerifyFile(typeof(PugMod.IMod).Assembly.Location, sdkHash);
            control = new SaveLoop(root, "corekeeper", new SaveAdapter(Prepare, Observe, Abandon));
        }
        private Func<SaveReceipt> Prepare()
        {
            CoreSaveTarget target = new CoreSaveTarget(entitiesHash, componentsHash);
            return delegate {
                if (!target.Submit()) return SaveReceipt.Rejected;
                active = target;
                return SaveReceipt.Pending;
            };
        }
        private SaveReceipt Observe()
        {
            if (active == null) throw new SaveFailure("native_outcome_unknown");
            return active.Advance() ? SaveReceipt.Dispatched : SaveReceipt.Pending;
        }
        private void Abandon() { active = null; }
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
            finally { control = null; active = null; }
        }
        public void ModObjectLoaded(UnityEngine.Object value) { }
        public bool CanBeUnloaded() { return control == null; }
    }
}

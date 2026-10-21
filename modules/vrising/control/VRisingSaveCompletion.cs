using System;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using LanGame.SaveControl;

namespace LanGame.VRising
{
    internal sealed class NativeSaveCompletion
    {
        // The payload builder replaces this marker with its bundled native DLL
        // digest before compiling the managed entry. An unbound source fails.
        private const string NativeHash = "LGSM_VRISING_NATIVE_SHA256";
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int Install();
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong Begin(IntPtr trigger, IntPtr serializer);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int End(ulong ticket, int admitted);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int ObserveNative(ulong ticket);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate int OperationDiagnostic(ulong ticket);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void AbandonNative(ulong ticket);
        [DllImport("kernel32.dll", EntryPoint = "LoadLibraryExW", CharSet = CharSet.Unicode, SetLastError = true)]
        private static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
        [DllImport("kernel32.dll", CharSet = CharSet.Ansi, ExactSpelling = true, SetLastError = true)]
        private static extern IntPtr GetProcAddress(IntPtr module, string name);
        private readonly Begin begin;
        private readonly End end;
        private readonly ObserveNative observe;
        private readonly AbandonNative abandon;
        private readonly OperationDiagnostic operationDiagnostic;
        private readonly Action<string> diagnostic;
        private ulong ticket;
        private Func<bool> context;
        internal NativeSaveCompletion(Action<string> diagnostic)
        {
            this.diagnostic = diagnostic;
            string directory = Path.GetDirectoryName(typeof(NativeSaveCompletion).Assembly.Location);
            string path = Path.Combine(directory, "LgsmVRisingSaveControl.dll");
            if (NativeHash.Length != 64) throw new SaveFailure("api_unavailable");
            // Hold a no-write/no-delete file pin across loader mapping.
            using (FileStream pin = SaveFiles.OpenPinnedFile(path, 4L * 1024 * 1024))
            {
                NativeBinding.VerifyFile(path, NativeHash);
                IntPtr module = LoadLibraryEx(path, IntPtr.Zero, 0x00000100 | 0x00000800);
                if (module == IntPtr.Zero) throw new SaveFailure("api_unavailable");
                begin = (Begin)Resolve(module, "lgsm_vr_save_begin", typeof(Begin));
                end = (End)Resolve(module, "lgsm_vr_save_end", typeof(End));
                observe = (ObserveNative)Resolve(module, "lgsm_vr_save_observe", typeof(ObserveNative));
                abandon = (AbandonNative)Resolve(module, "lgsm_vr_save_abandon", typeof(AbandonNative));
                operationDiagnostic = (OperationDiagnostic)Resolve(module, "lgsm_vr_save_operation_diagnostic", typeof(OperationDiagnostic));
                Install install = (Install)Resolve(module, "lgsm_vr_save_install", typeof(Install));
                if (install() != 1) throw new SaveFailure("unsupported_native_build");
            }
            // The native module pins itself and owns active jump/unwind records
            // until process exit. Loader unload only abandons this IPC owner.
        }
        private static Delegate Resolve(IntPtr module, string name, Type type)
        {
            IntPtr method = GetProcAddress(module, name);
            if (method == IntPtr.Zero) throw new SaveFailure("api_unavailable");
            return Marshal.GetDelegateForFunctionPointer(method, type);
        }
        internal static IntPtr Pointer(object native)
        {
            if (native == null) throw new SaveFailure("world_not_ready");
            PropertyInfo property = native.GetType().GetProperty("Pointer", BindingFlags.Public | BindingFlags.Instance);
            if (property == null || property.PropertyType != typeof(IntPtr)) throw new SaveFailure("api_unavailable");
            IntPtr pointer = (IntPtr)property.GetValue(native, null);
            if (pointer == IntPtr.Zero) throw new SaveFailure("world_not_ready");
            return pointer;
        }
        internal SaveReceipt Start(object trigger, object serializer, Func<bool> invoke, Func<bool> sameWorld)
        {
            if (ticket != 0) throw new SaveFailure("save_busy");
            if (sameWorld == null || !sameWorld()) throw new SaveFailure("target_changed");
            ticket = begin(Pointer(trigger), Pointer(serializer));
            if (ticket == 0) { Report("begin"); throw new SaveFailure("save_busy"); }
            context = sameWorld;
            bool admitted;
            try { admitted = invoke(); }
            catch { Report("invoke"); throw; }
            int bound = end(ticket, admitted ? 1 : 0);
            if (!admitted) { Abandon(); return SaveReceipt.Rejected; }
            if (bound != 0) { Report("bind"); throw new SaveFailure("native_outcome_unknown"); }
            return SaveReceipt.Pending;
        }
        internal SaveReceipt Observe()
        {
            if (ticket == 0 || context == null || !context()) { Report("context"); throw new SaveFailure("native_outcome_unknown"); }
            switch (observe(ticket))
            {
                case 0: return SaveReceipt.Pending;
                case 1: return SaveReceipt.Completed;
                case 2: return SaveReceipt.Failed;
                default: Report("observe"); throw new SaveFailure("native_outcome_unknown");
            }
        }
        private void Report(string stage)
        {
            if (diagnostic == null) return;
            string reason;
            switch (operationDiagnostic(ticket))
            {
                case 0: reason = "none"; break;
                case 1: reason = "trigger_instance"; break;
                case 2: reason = "trigger_reason"; break;
                case 3: reason = "trigger_caller"; break;
                case 4: reason = "trigger_repeated"; break;
                case 5: reason = "trigger_name_encoding"; break;
                case 6: reason = "trigger_name_format"; break;
                case 7: reason = "native_rejected"; break;
                case 8: reason = "trigger_missing"; break;
                case 9: reason = "serialize_lifecycle"; break;
                case 10: reason = "event_shape"; break;
                case 11: reason = "event_identity"; break;
                case 12: reason = "target_identity"; break;
                case 13: reason = "finalizer_repeated"; break;
                case 14: reason = "final_result"; break;
                case 15: reason = "result_address"; break;
                case 16: reason = "operation_identity"; break;
                case 17: reason = "native_code"; break;
                case 18: reason = "owner_thread"; break;
                default: reason = "diagnostic_contract"; break;
            }
            // Closed labels only: never log a filename, native pointer, save
            // payload, IPC token, raw request or game-supplied exception text.
            diagnostic("LanGame VRising save verification failure stage=" + stage + " reason=" + reason);
        }
        internal string Issue() { return "vrising_write_failed"; }
        internal void Abandon()
        {
            ulong previous = ticket; ticket = 0; context = null;
            if (previous != 0) abandon(previous);
        }
    }
}

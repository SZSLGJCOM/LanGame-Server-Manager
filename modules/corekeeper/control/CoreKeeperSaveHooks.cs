using System;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using System.Threading;
using HarmonyLib;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    internal sealed class CoreSaveHooks : IDisposable
    {
        internal const string Owner = "local.langame.corekeeper.save";
        private readonly Harmony harmony = new Harmony(Owner);
        private readonly List<MethodBase> methods = new List<MethodBase>();
        private static object armedFilesystem;
        private static int writerThread;
        private static int installed;
        private const BindingFlags Flags = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;

        internal CoreSaveHooks()
        {
            if (Interlocked.CompareExchange(ref installed, 1, 0) != 0) throw new SaveFailure("api_unavailable");
            try { Install(); }
            catch (Exception original)
            {
                try { harmony.UnpatchSelf(); Interlocked.Exchange(ref installed, 0); }
                catch (Exception cleanup) { throw new AggregateException(original, cleanup); }
                throw;
            }
        }
        private void Install()
        {
            NativeBinding.VerifyFile(typeof(Harmony).Assembly.Location,
                "01fc6fcb6408811b72e25bfc3628cca08b27473758966777254500d07d170b4e");
            Type serializer = NativeBinding.Type("Pug.Other", "SerializeWorldSystem");
            Type manager = NativeBinding.Type("Pug.Other", "SaveManager");
            Type filesystem = NativeBinding.Type("Pug.Other", "Pug.Platform.FilesystemManager");
            Type platform = NativeBinding.Type("Pug.Other", "Pug.Platform.StandaloneFilesystem");
            Patch(Exact(serializer, "ScheduleOutputWorldUpdateJobs", 0x0600427d), "BeforeSerialization", null, null);
            Patch(Exact(serializer, "WriteSerializedDataToFile", 0x0600427f), "BeforeWrite", null, "AfterWrite");
            Patch(Exact(manager, "WriteCompressedWorldData", 0x06007ffa), "BeforePayload", null, null);
            Patch(Exact(filesystem, "WriteHandler", 0x0600929b), null, "WriterTranspiler", null);
            Patch(Exact(platform, "Write", 0x0600927b), null, "PublicationTranspiler", null);
        }
        private static MethodInfo Exact(Type type, string name, int token)
        {
            foreach (MethodInfo method in type.GetMethods(Flags))
                if (method.Name == name && method.MetadataToken == token && method.ReturnType == typeof(void)) return method;
            throw new SaveFailure("api_unavailable");
        }
        private static HarmonyMethod Observer(string name)
        {
            return name == null ? null : new HarmonyMethod(typeof(CoreSaveHooks).GetMethod(name,
                BindingFlags.NonPublic | BindingFlags.Static));
        }
        private void Patch(MethodInfo method, string prefix, string transpiler, string finalizer)
        {
            Exclusive(method);
            methods.Add(method);
            harmony.Patch(method, prefix: Observer(prefix), transpiler: Observer(transpiler), finalizer: Observer(finalizer));
        }
        private static void Exclusive(MethodBase method)
        {
            Patches patches = Harmony.GetPatchInfo(method);
            if (patches != null)
                foreach (string owner in patches.Owners)
                    if (owner != Owner) throw new SaveFailure("api_unavailable");
        }
        internal void Validate() { foreach (MethodBase method in methods) Exclusive(method); }
        internal static void WriterLoopStarted(object filesystem)
        {
            // This runs inside the transformed native loop, not at installation.
            // A loop that was already executing before patching cannot arm it.
            Interlocked.Exchange(ref armedFilesystem, filesystem);
            Interlocked.Exchange(ref writerThread, Thread.CurrentThread.ManagedThreadId);
        }
        internal static bool WriterArmed(object filesystem)
        { return Object.ReferenceEquals(Interlocked.CompareExchange(ref armedFilesystem, null, null), filesystem); }
        internal static bool OnWriterThread()
        { return Thread.CurrentThread.ManagedThreadId == Interlocked.CompareExchange(ref writerThread, 0, 0); }
        private static void BeforeSerialization(object __instance) { CoreSaveCompletion.Started(__instance); }
        private static void BeforeWrite(object __instance) { CoreSaveCompletion.Serializing(__instance); }
        private static Exception AfterWrite(Exception __exception)
        { CoreSaveCompletion.SerializationEnded(); return __exception; }
        private static void BeforePayload(object __instance, byte[] data, int size)
        { CoreSaveCompletion.Payload(__instance, data, size); }

        private static IEnumerable<CodeInstruction> WriterTranspiler(IEnumerable<CodeInstruction> instructions, MethodBase __originalMethod)
        {
            MethodBody body = __originalMethod.GetMethodBody();
            if (__originalMethod.MetadataToken != 0x0600929b || body == null || body.LocalVariables.Count == 0)
                throw new SaveFailure("api_unavailable");
            return RewriteWriter(instructions, body.LocalVariables[0].LocalType);
        }
        internal static IEnumerable<CodeInstruction> RewriteWriter(IEnumerable<CodeInstruction> instructions, Type request)
        {
            List<CodeInstruction> code = new List<CodeInstruction>(instructions);
            int start = -1, end = -1;
            for (int i = 0; i + 1 < code.Count; i++)
            {
                FieldInfo field = code[i + 1].operand as FieldInfo;
                if (code[i].opcode != OpCodes.Ldloc_0 || code[i + 1].opcode != OpCodes.Ldfld ||
                    field == null || field.DeclaringType != request) continue;
                if (field.Name == "flush") { if (start != -1) throw new SaveFailure("api_unavailable"); start = i; }
                if (field.Name == "addToPool") { if (end != -1) throw new SaveFailure("api_unavailable"); end = i; }
            }
            if (start < 0 || end <= start) throw new SaveFailure("api_unavailable");
            List<CodeInstruction> result = new List<CodeInstruction>();
            if (code.Count == 0) throw new SaveFailure("api_unavailable");
            result.Add(MoveBoundary(code[0], new CodeInstruction(OpCodes.Ldarg_0)));
            result.Add(new CodeInstruction(OpCodes.Call, typeof(CoreSaveHooks).GetMethod("WriterLoopStarted",
                BindingFlags.NonPublic | BindingFlags.Static)));
            for (int i = 0; i < code.Count; i++)
            {
                if (i == start)
                {
                    CodeInstruction first = MoveBoundary(code[i], new CodeInstruction(OpCodes.Ldarg_0));
                    result.Add(first); result.Add(new CodeInstruction(OpCodes.Ldloc_0));
                    result.Add(new CodeInstruction(OpCodes.Box, request));
                    result.Add(Call("WriterStarted"));
                }
                if (i == end) result.Add(MoveBoundary(code[i], Call("WriterEnded")));
                result.Add(code[i]);
            }
            return result;
        }
        private static IEnumerable<CodeInstruction> PublicationTranspiler(IEnumerable<CodeInstruction> instructions)
        { return RewritePublication(instructions); }
        internal static IEnumerable<CodeInstruction> RewritePublication(IEnumerable<CodeInstruction> instructions)
        {
            List<CodeInstruction> result = new List<CodeInstruction>();
            int replace = 0, move = 0;
            foreach (CodeInstruction instruction in instructions)
            {
                result.Add(instruction);
                MethodInfo method = instruction.operand as MethodInfo;
                if (instruction.opcode != OpCodes.Call || method == null || method.DeclaringType != typeof(System.IO.File)) continue;
                bool replaced = method.Name == "Replace" && method.GetParameters().Length == 4;
                bool moved = method.Name == "Move" && method.GetParameters().Length == 2;
                if (!replaced && !moved) continue;
                if (replaced) replace++; else move++;
                result.Add(new CodeInstruction(OpCodes.Ldarg_0));
                result.Add(new CodeInstruction(OpCodes.Ldarg_2));
                result.Add(new CodeInstruction(OpCodes.Ldarg_3));
                result.Add(Call("Published"));
            }
            if (replace != 1 || move != 1) throw new SaveFailure("api_unavailable");
            return result;
        }
        private static CodeInstruction Call(string name)
        {
            return new CodeInstruction(OpCodes.Call, typeof(CoreSaveCompletion).GetMethod(name,
                BindingFlags.NonPublic | BindingFlags.Static));
        }
        private static CodeInstruction MoveBoundary(CodeInstruction from, CodeInstruction to)
        {
            to.labels.AddRange(from.labels); from.labels.Clear();
            to.blocks.AddRange(from.blocks); from.blocks.Clear();
            return to;
        }
        public void Dispose() { harmony.UnpatchSelf(); Interlocked.Exchange(ref installed, 0); }
    }
}

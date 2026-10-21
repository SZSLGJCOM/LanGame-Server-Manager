using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Reflection.Emit;
using System.Threading;
using HarmonyLib;
using LanGame.CoreKeeper;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    // Only the Unity boundary is replaced. Tests run the production correlation
    // state machine, real Harmony instruction rewrites and admitted native IL.
    internal sealed class CoreSaveTarget
    {
        internal readonly object World = new object(), Serializer = new object(), Filesystem = new object();
        internal readonly object Platform = new object(), Manager = new object(), File = new object(), Entity = new object();
        internal bool Valid = true, Included = true;
        internal int Submissions;
        internal void Validate() { if (!Valid) throw new SaveFailure("target_changed"); }
        internal object Submit() { Submissions++; return Entity; }
        internal bool Consumes(object serializer, object entity)
        { Validate(); return Included && Object.ReferenceEquals(Serializer, serializer) && Object.ReferenceEquals(Entity, entity); }
        internal bool SameFile(object file) { return Object.ReferenceEquals(file, File); }
        internal void Pump() { Validate(); }
        internal string RelativePath() { return "worlds/0.world.gzip"; }
        internal static FieldInfo Field(Type type, string name) { return type.GetField(name); }
    }
}

internal static class CoreKeeperCompletionTests
{
    private sealed class Request
    {
        public object file;
        public byte[] data;
        public int dataSize;
        public bool raw = true, remove = false, flush = false, addToPool = true;
        public string subFolder = null;
    }
    private static int passed;
    private static void Check(bool value) { if (!value) throw new Exception("assertion failed"); }
    private static void Run(string name, Action test) { test(); passed++; Console.WriteLine("PASS " + name); }
    private static byte[] Data() { return Enumerable.Range(0, 2048).Select(i => (byte)i).ToArray(); }
    private static void Unknown(Action action)
    {
        try { action(); throw new Exception("expected unknown"); }
        catch (SaveFailure failure) { Check(failure.Code == "native_outcome_unknown" || failure.Code == "target_changed"); }
    }
    private static Request Queue(CoreSaveCompletion save, CoreSaveTarget target)
    {
        Check(save.Begin(target) == SaveReceipt.Pending); Check(target.Submissions == 1);
        CoreSaveCompletion.Started(target.Serializer);
        CoreSaveCompletion.Serializing(target.Serializer);
        byte[] data = Data(); CoreSaveCompletion.Payload(target.Manager, data, data.Length);
        CoreSaveCompletion.SerializationEnded();
        return new Request { file = target.File, data = data, dataSize = data.Length };
    }
    private static void Publish(CoreSaveTarget target, Request request)
    {
        CoreSaveCompletion.WriterStarted(target.Filesystem, request);
        CoreSaveCompletion.Published(target.Platform, target.RelativePath(), request.data);
        CoreSaveCompletion.WriterEnded();
    }
    private static void Case(string name, Action<CoreSaveCompletion, CoreSaveTarget, Request> test)
    {
        Run(name, delegate { using (CoreSaveCompletion save = new CoreSaveCompletion(false)) {
            CoreSaveTarget target = new CoreSaveTarget(); Request request = Queue(save, target); test(save, target, request);
        }});
    }
    private static void GateFailure(Action rewrite)
    {
        try { rewrite(); throw new Exception("expected fixed IL rejection"); }
        catch (SaveFailure failure) { Check(failure.Code == "api_unavailable"); }
    }
    private static int Calls(IEnumerable<CodeInstruction> code, string name)
    { return code.Count(i => i.operand is MethodInfo && ((MethodInfo)i.operand).Name == name); }

    public static void Main(string[] args)
    {
        Case("exact native entity/payload/publication closes", (s,t,r) => { Publish(t,r); Check(s.Observe() == SaveReceipt.Completed); });
        Case("writer receipt can arrive on its owned worker", (s,t,r) => {
            Exception failure = null; Thread worker = new Thread(delegate() { try { Publish(t,r); } catch(Exception e) { failure=e; } });
            worker.Start(); worker.Join(); Check(failure == null); Check(s.Observe() == SaveReceipt.Completed);
        });
        Case("payload queue is not completion", (s,t,r) => Check(s.Observe() == SaveReceipt.Pending));
        Case("publication still waits for native request end", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data);
            Check(s.Observe() == SaveReceipt.Pending); CoreSaveCompletion.WriterEnded(); Check(s.Observe() == SaveReceipt.Completed);
        });
        Case("void return and pool return cannot acknowledge failure", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("unowned publication cannot acknowledge", (s,t,r) => {
            CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data); Check(s.Observe() == SaveReceipt.Pending);
        });
        Case("another payload object cannot acknowledge", (s,t,r) => {
            r.data=(byte[])r.data.Clone(); Publish(t,r); Check(s.Observe() == SaveReceipt.Pending);
        });
        Case("another filesystem cannot acknowledge", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(new object(),r); CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data);
            CoreSaveCompletion.WriterEnded(); Check(s.Observe() == SaveReceipt.Pending);
        });
        Case("another file descriptor is rejected", (s,t,r) => { r.file=new object(); Publish(t,r); Unknown(() => s.Observe()); });
        Case("another data size is rejected", (s,t,r) => { r.dataSize--; Publish(t,r); Unknown(() => s.Observe()); });
        Case("transformed raw contract is rejected", (s,t,r) => { r.raw=false; Publish(t,r); Unknown(() => s.Observe()); });
        Case("delete request cannot acknowledge save", (s,t,r) => { r.remove=true; Publish(t,r); Unknown(() => s.Observe()); });
        Case("flush marker cannot acknowledge save", (s,t,r) => { r.flush=true; Publish(t,r); Unknown(() => s.Observe()); });
        Case("redirected native subfolder is rejected", (s,t,r) => { r.subFolder="other"; Publish(t,r); Unknown(() => s.Observe()); });
        Case("another platform cannot acknowledge", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.Published(new object(),t.RelativePath(),r.data);
            CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("another publication path is rejected", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.Published(t.Platform,"worlds/1.world.gzip",r.data);
            CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("different bytes are rejected", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); byte[] changed=(byte[])r.data.Clone(); changed[1024]^=1;
            CoreSaveCompletion.Published(t.Platform,t.RelativePath(),changed); CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("short publication is rejected", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.Published(t.Platform,t.RelativePath(),new byte[1]);
            CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("world changed after publication cannot acknowledge", (s,t,r) => { Publish(t,r); t.Valid=false; Unknown(() => s.Observe()); });
        Case("duplicate serializer is rejected", (s,t,r) => { CoreSaveCompletion.Started(t.Serializer); Publish(t,r); Unknown(() => s.Observe()); });
        Case("duplicate payload is rejected", (s,t,r) => {
            CoreSaveCompletion.Serializing(t.Serializer); CoreSaveCompletion.Payload(t.Manager,r.data,r.data.Length);
            CoreSaveCompletion.SerializationEnded(); Publish(t,r); Unknown(() => s.Observe());
        });
        Case("duplicate writer is rejected", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.WriterStarted(t.Filesystem,r);
            CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data); CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("duplicate publication is rejected", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data);
            CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data); CoreSaveCompletion.WriterEnded(); Unknown(() => s.Observe());
        });
        Case("abandoned worker cannot complete a new generation", (s,t,r) => {
            CoreSaveCompletion.WriterStarted(t.Filesystem,r); s.Abandon();
            CoreSaveTarget next=new CoreSaveTarget(); Check(s.Begin(next)==SaveReceipt.Pending);
            CoreSaveCompletion.Published(t.Platform,t.RelativePath(),r.data); CoreSaveCompletion.WriterEnded();
            Check(s.Observe()==SaveReceipt.Pending); s.Abandon(); Request fresh=Queue(s,new CoreSaveTarget());
            Check(fresh!=null);
        });
        Run("missing owned entity does not borrow another save", delegate {
            using(CoreSaveCompletion s=new CoreSaveCompletion(false)) {
                CoreSaveTarget t=new CoreSaveTarget { Included=false }; Check(s.Begin(t)==SaveReceipt.Pending);
                CoreSaveCompletion.Started(t.Serializer); CoreSaveCompletion.Serializing(t.Serializer);
                byte[] data=Data(); CoreSaveCompletion.Payload(t.Manager,data,data.Length); CoreSaveCompletion.SerializationEnded();
                Publish(t,new Request {file=t.File,data=data,dataSize=data.Length}); Check(s.Observe()==SaveReceipt.Pending);
            }
        });
        Run("wrong owner thread cannot bind a generation", delegate {
            using(CoreSaveCompletion s=new CoreSaveCompletion(false)) {
                CoreSaveTarget t=new CoreSaveTarget(); s.Begin(t);
                Thread worker=new Thread(() => CoreSaveCompletion.Started(t.Serializer)); worker.Start(); worker.Join();
                CoreSaveCompletion.Serializing(t.Serializer); byte[] data=Data(); CoreSaveCompletion.Payload(t.Manager,data,data.Length);
                CoreSaveCompletion.SerializationEnded(); Publish(t,new Request {file=t.File,data=data,dataSize=data.Length});
                Check(s.Observe()==SaveReceipt.Pending);
            }
        });
        Run("another serializer cannot bind the entity", delegate {
            using(CoreSaveCompletion s=new CoreSaveCompletion(false)) {
                CoreSaveTarget t=new CoreSaveTarget(); s.Begin(t); CoreSaveCompletion.Started(new object());
                Check(s.Observe()==SaveReceipt.Pending);
            }
        });
        Run("original exception is returned unchanged", delegate {
            Exception original=new IOException("fixture native failure");
            MethodInfo finalizer=typeof(CoreSaveHooks).GetMethod("AfterWrite",BindingFlags.NonPublic|BindingFlags.Static);
            Check(Object.ReferenceEquals(original,finalizer.Invoke(null,new object[] {original})));
        });
        Run("writer must actually execute the patched loop before admission", delegate {
            object filesystem=new object(); Check(!CoreSaveHooks.WriterArmed(filesystem));
            CoreSaveHooks.WriterLoopStarted(filesystem); Check(CoreSaveHooks.WriterArmed(filesystem));
            Check(!CoreSaveHooks.WriterArmed(new object())); Check(CoreSaveHooks.OnWriterThread());
            bool other=true; Thread worker=new Thread(() => other=CoreSaveHooks.OnWriterThread());
            worker.Start(); worker.Join(); Check(!other);
        });
        NativeInstructions(args[0]);
        Console.WriteLine("PASS "+passed+" Core Keeper completion/IL boundary checks");
    }

    private static void NativeInstructions(string directory)
    {
        AppDomain.CurrentDomain.AssemblyResolve += delegate(object sender, ResolveEventArgs e) {
            string path=Path.Combine(directory,new AssemblyName(e.Name).Name+".dll");
            return System.IO.File.Exists(path)?Assembly.LoadFrom(path):null;
        };
        Assembly native=Assembly.LoadFrom(Path.Combine(directory,"Pug.Other.dll"));
        MethodBase writer=native.ManifestModule.ResolveMethod(0x0600929b);
        MethodBase publisher=native.ManifestModule.ResolveMethod(0x0600927b);
        Run("native filesystem binding uses the real namespace", delegate {
            Check(writer.DeclaringType.FullName=="Pug.Platform.FilesystemManager");
            Check(publisher.DeclaringType.FullName=="Pug.Platform.StandaloneFilesystem");
        });
        ILGenerator writerGenerator=new DynamicMethod("native_writer_read",typeof(void),Type.EmptyTypes,
            typeof(CoreKeeperCompletionTests),true).GetILGenerator();
        ILGenerator publishGenerator=new DynamicMethod("native_publish_read",typeof(void),Type.EmptyTypes,
            typeof(CoreKeeperCompletionTests),true).GetILGenerator();
        List<CodeInstruction> originalWriter=PatchProcessor.GetOriginalInstructions(writer,writerGenerator);
        List<CodeInstruction> originalPublish=PatchProcessor.GetOriginalInstructions(publisher,publishGenerator);
        Type request=writer.GetMethodBody().LocalVariables[0].LocalType;
        Run("actual native writer retains all original calls", delegate {
            List<CodeInstruction> rewritten=CoreSaveHooks.RewriteWriter(originalWriter,request).ToList();
            Check(Calls(rewritten,"WriterStarted")==1 && Calls(rewritten,"WriterEnded")==1);
            Check(rewritten.Where(i=>i.operand is MethodInfo && ((MethodInfo)i.operand).DeclaringType!=typeof(CoreSaveCompletion)
                    && ((MethodInfo)i.operand).DeclaringType!=typeof(CoreSaveHooks))
                .Select(i=>i.operand).SequenceEqual(originalWriter.Where(i=>i.operand is MethodInfo).Select(i=>i.operand)));
        });
        Run("actual native publication adds receipts only after Replace/Move", delegate {
            List<CodeInstruction> rewritten=CoreSaveHooks.RewritePublication(originalPublish).ToList();
            Check(Calls(originalPublish,"Published")==0 && Calls(rewritten,"Published")==2);
            for(int i=0;i<rewritten.Count;i++) if(rewritten[i].operand is MethodInfo && ((MethodInfo)rewritten[i].operand).Name=="Published") {
                MethodInfo preceding=rewritten[i-4].operand as MethodInfo;
                Check(preceding!=null && preceding.DeclaringType==typeof(System.IO.File) &&
                    (preceding.Name=="Replace" || preceding.Name=="Move"));
            }
        });
        Run("changed native writer shape is rejected", delegate {
            GateFailure(()=>CoreSaveHooks.RewriteWriter(originalWriter.Where(i=>!(i.operand is FieldInfo && ((FieldInfo)i.operand).Name=="flush")),request).ToList());
        });
        Run("changed native publication shape is rejected", delegate {
            GateFailure(()=>CoreSaveHooks.RewritePublication(originalPublish.Where(i=>!(i.operand is MethodInfo && ((MethodInfo)i.operand).Name=="Move"))).ToList());
        });
        Run("duplicate native publication shape is rejected", delegate {
            CodeInstruction replace=originalPublish.First(i=>i.operand is MethodInfo && ((MethodInfo)i.operand).Name=="Replace");
            GateFailure(()=>CoreSaveHooks.RewritePublication(originalPublish.Concat(new[] {replace})).ToList());
        });
    }
}

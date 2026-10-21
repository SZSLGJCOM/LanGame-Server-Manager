using System;
using System.Threading;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    internal sealed class CoreSaveCompletion : IDisposable
    {
        private sealed class Capture
        {
            internal long Generation;
            internal CoreSaveTarget Target;
            internal object Entity;
            internal byte[] Payload;
            internal int Size;
            internal string Path;
            internal bool Started, Writing, Published, Completed, Failed;
        }
        private static readonly object Gate = new object();
        private static CoreSaveCompletion current;
        private static bool poisoned;
        [ThreadStatic] private static long serializing;
        [ThreadStatic] private static long writing;
        private readonly int ownerThread = Thread.CurrentThread.ManagedThreadId;
        private static long generation;
        private Capture active;
        private CoreSaveHooks hooks;

        internal CoreSaveCompletion(bool install)
        {
            lock (Gate)
            {
                if (current != null || poisoned) throw new SaveFailure("api_unavailable");
                current = this;
            }
            try { if (install) hooks = new CoreSaveHooks(); }
            catch { lock (Gate) { current = null; poisoned = true; } throw; }
        }
        internal CoreSaveCompletion(CoreSaveHooks installed) : this(false)
        { hooks = installed; }

        internal Func<SaveReceipt> Prepare()
        {
            CheckOwner();
            if (hooks != null) hooks.Validate();
            CoreSaveTarget target = new CoreSaveTarget();
            return delegate { return Begin(target); };
        }
        internal SaveReceipt Begin(CoreSaveTarget target)
        {
            CheckOwner(); target.Validate();
            Capture capture;
            lock (Gate)
            {
                if (active != null) throw new SaveFailure("save_busy");
                capture = new Capture { Generation = ++generation, Target = target };
                active = capture;
                Console.Error.WriteLine("LanGame Core save: owned request admitted.");
            }
            try
            {
                object entity = target.Submit();
                if (entity == null) throw new SaveFailure("native_outcome_unknown");
                lock (Gate) { capture.Entity = entity; }
                return SaveReceipt.Pending;
            }
            catch { Abandon(); throw; }
        }
        internal SaveReceipt Observe()
        {
            CheckOwner();
            if (hooks != null) hooks.Validate();
            Capture capture;
            lock (Gate) { capture = active; }
            if (capture == null) throw new SaveFailure("native_outcome_unknown");
            capture.Target.Validate();
            bool pump;
            lock (Gate) { pump = capture == active && !capture.Completed && !capture.Failed; }
            if (pump) capture.Target.Pump();
            lock (Gate)
            {
                if (capture != active || capture.Failed) throw new SaveFailure("native_outcome_unknown");
                return capture.Completed ? SaveReceipt.Completed : SaveReceipt.Pending;
            }
        }
        internal void Abandon()
        {
            lock (Gate)
            {
                if (active != null) {
                    Console.Error.WriteLine("LanGame Core save: released observer (started={0}, payload={1}, writing={2}, published={3}, completed={4}, failed={5}).",
                        active.Started, active.Payload != null, active.Writing, active.Published, active.Completed, active.Failed);
                    active.Payload = null; active.Target = null; active = null;
                }
            }
        }
        private void CheckOwner()
        {
            if (Thread.CurrentThread.ManagedThreadId != ownerThread || current != this)
                throw new SaveFailure("target_changed");
        }

        // Observer entry points never throw into the game or replace an original
        // exception. A failed observation can only prevent our acknowledgement.
        internal static void Started(object serializer)
        {
            try
            {
                lock (Gate)
                {
                    CoreSaveCompletion owner = current;
                    Capture c = owner == null ? null : owner.active;
                    if (c == null || Thread.CurrentThread.ManagedThreadId != owner.ownerThread ||
                        !c.Target.Consumes(serializer, c.Entity)) return;
                    if (c.Started) c.Failed = true;
                    c.Started = true;
                    Console.Error.WriteLine("LanGame Core save: owned entity entered serialization.");
                }
            }
            catch { Failed(); }
        }
        internal static void Serializing(object serializer)
        {
            try
            {
                lock (Gate)
                {
                    Capture c = current == null ? null : current.active;
                    if (c == null || Thread.CurrentThread.ManagedThreadId != current.ownerThread ||
                        !Object.ReferenceEquals(serializer, c.Target.Serializer) || !c.Started) return;
                    if (serializing != 0 || c.Payload != null) { c.Failed = true; return; }
                    serializing = c.Generation;
                }
            }
            catch { Failed(); }
        }
        internal static void SerializationEnded() { serializing = 0; }
        internal static void Payload(object manager, byte[] data, int size)
        {
            try
            {
                lock (Gate)
                {
                    Capture c = current == null ? null : current.active;
                    if (c == null || serializing != c.Generation || !Object.ReferenceEquals(manager, c.Target.Manager)) return;
                    if (c.Payload != null || data == null || size <= 1024 || size > data.Length || size > 512 * 1024 * 1024)
                    { c.Failed = true; return; }
                    c.Payload = data; c.Size = size;
                    Console.Error.WriteLine("LanGame Core save: owned payload captured.");
                }
            }
            catch { Failed(); }
        }
        internal static void WriterStarted(object filesystem, object request)
        {
            writing = 0;
            try
            {
                lock (Gate)
                {
                    Capture c = current == null ? null : current.active;
                    if (c == null || !Object.ReferenceEquals(filesystem, c.Target.Filesystem) || c.Payload == null) return;
                    if (current.hooks != null && !CoreSaveHooks.OnWriterThread()) { c.Failed = true; return; }
                    Type type = request.GetType();
                    if (!Object.ReferenceEquals(CoreSaveTarget.Field(type, "data").GetValue(request), c.Payload)) return;
                    if (c.Writing || !c.Target.SameFile(CoreSaveTarget.Field(type, "file").GetValue(request)) ||
                        (int)CoreSaveTarget.Field(type, "dataSize").GetValue(request) != c.Size ||
                        !(bool)CoreSaveTarget.Field(type, "raw").GetValue(request) ||
                        (bool)CoreSaveTarget.Field(type, "remove").GetValue(request) ||
                        (bool)CoreSaveTarget.Field(type, "flush").GetValue(request) ||
                        !String.IsNullOrEmpty((string)CoreSaveTarget.Field(type, "subFolder").GetValue(request)))
                    { c.Failed = true; return; }
                    c.Path = c.Target.RelativePath(); c.Writing = true; writing = c.Generation;
                    Console.Error.WriteLine("LanGame Core save: owned writer entered.");
                }
            }
            catch { Failed(); }
        }
        internal static void Published(object platform, string path, byte[] data)
        {
            try
            {
                lock (Gate)
                {
                    Capture c = current == null ? null : current.active;
                    if (c == null || writing != c.Generation || !Object.ReferenceEquals(platform, c.Target.Platform)) return;
                    if (c.Published || !String.Equals(path, c.Path, StringComparison.Ordinal) ||
                        data == null || data.Length != c.Size) { c.Failed = true; return; }
                    for (int i = 0; i < c.Size; i++)
                        if (data[i] != c.Payload[i]) { c.Failed = true; return; }
                    c.Published = true;
                }
            }
            catch { Failed(); }
        }
        internal static void WriterEnded()
        {
            try
            {
                lock (Gate)
                {
                    Capture c = current == null ? null : current.active;
                    if (c != null && writing == c.Generation)
                    {
                        if (!c.Published) c.Failed = true;
                        c.Completed = c.Published && !c.Failed;
                        c.Payload = null; // before the native pool can reuse it
                    }
                }
            }
            catch { Failed(); }
            finally { writing = 0; }
        }
        private static void Failed()
        {
            lock (Gate) { if (current != null && current.active != null) current.active.Failed = true; }
        }
        public void Dispose()
        {
            Abandon();
            lock (Gate) { if (current == this) current = null; }
            try { if (hooks != null) hooks.Dispose(); hooks = null; }
            catch { lock (Gate) { poisoned = true; } throw; }
        }
    }
}

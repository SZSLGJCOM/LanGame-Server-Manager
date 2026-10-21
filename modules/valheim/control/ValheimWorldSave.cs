using System;
using System.Reflection;
using System.Threading;

namespace Lgsm.Valheim.Control
{
    [System.Runtime.Serialization.DataContract]
    internal sealed class WorldSaveReceipt
    {
        [System.Runtime.Serialization.DataMember(Name = "previous_save_number")] public uint Previous;
        [System.Runtime.Serialization.DataMember(Name = "committed_save_number")] public uint Committed;
        [System.Runtime.Serialization.DataMember(Name = "evidence")] public string Evidence = "native_commit";
    }

    internal interface IValheimWorldSave
    {
        void BeginSave();
        WorldSaveReceipt PollSave();
        void EndSaveObservation();
    }

    // This adapter is admitted only after the existing three-assembly profile
    // check. The owner thread initiates native saving; it never waits for the
    // save thread, changes save flags, or selects a path supplied by a request.
    internal sealed class ValheimWorldSave : IDisposable
    {
        private const BindingFlags Public = BindingFlags.Public;
        private const BindingFlags Private = BindingFlags.NonPublic;
        private const BindingFlags Static = BindingFlags.Static;
        private const BindingFlags Instance = BindingFlags.Instance;
        private readonly MethodInfo save, diskSpace, isSaving, dedicated, world;
        private readonly FieldInfo thread, loadError, committed, saving, started;
        private readonly PropertyInfo enabled, gameInstance, zoneInstance, dungeonInstance;
        private readonly MethodInfo zoneSkip, dungeonSkip;
        private readonly Action onStarted;
        private object server, savedWorld;
        private Thread ownedThread;
        private uint previous;
        private int starts;
        private bool observing;

        internal ValheimWorldSave(Type znet, Type saveSystem, Type zone, Type dungeon, Type game)
        {
            save = Method(znet, "Save", Public | Instance, typeof(void), typeof(bool), typeof(bool), typeof(bool));
            diskSpace = Method(znet, "EnoughDiskSpaceAvailable", Public | Instance, typeof(bool),
                typeof(bool).MakeByRefType(), typeof(bool), typeof(Action<bool>));
            isSaving = Method(znet, "IsSaving", Public | Instance, typeof(bool));
            dedicated = Method(znet, "IsDedicated", Public | Instance, typeof(bool));
            world = znet.GetMethod("GetWorldIfIsHost", Public | Static, null, Type.EmptyTypes, null);
            if (world == null || world.ReturnType == typeof(void) || world.ReturnType.IsValueType)
                throw new ControlFailure("unsupported_native_save_contract");
            thread = Field(znet, "m_saveThread", Private | Instance, typeof(Thread));
            loadError = Field(znet, "m_loadError", Public | Static, typeof(bool));
            started = Field(znet, "WorldSaveStarted", Public | Static, typeof(Action));
            committed = Field(saveSystem, "s_saveNumber", Private | Static, typeof(uint));
            saving = Field(saveSystem, "s_saving", Private | Static, typeof(bool));
            enabled = znet.GetProperty("enabled", Public | Instance);
            if (enabled == null || enabled.PropertyType != typeof(bool))
                throw new ControlFailure("unsupported_native_save_contract");
            zoneInstance = Singleton(zone);
            dungeonInstance = Singleton(dungeon);
            gameInstance = Singleton(game);
            zoneSkip = Method(zone, "SkipSaving", Public | Instance, typeof(bool));
            dungeonSkip = Method(dungeon, "SkipSaving", Public | Instance, typeof(bool));
            onStarted = delegate { Interlocked.Increment(ref starts); };
        }

        private static PropertyInfo Singleton(Type type)
        {
            PropertyInfo result = type.GetProperty("instance", Public | Static);
            if (result == null || result.PropertyType != type)
                throw new ControlFailure("unsupported_native_save_contract");
            return result;
        }

        private static MethodInfo Method(Type type, string name, BindingFlags flags, Type result, params Type[] arguments)
        {
            MethodInfo method = type.GetMethod(name, flags, null, arguments, null);
            if (method == null || method.ReturnType != result)
                throw new ControlFailure("unsupported_native_save_contract");
            return method;
        }

        private static FieldInfo Field(Type type, string name, BindingFlags flags, Type expected)
        {
            FieldInfo result = type.GetField(name, flags);
            if (result == null || result.FieldType != expected)
                throw new ControlFailure("unsupported_native_save_contract");
            return result;
        }

        internal void Begin(object currentServer)
        {
            if (observing) throw new ControlFailure("save_busy");
            object zone = zoneInstance.GetValue(null, null);
            object dungeon = dungeonInstance.GetValue(null, null);
            object currentWorld = world.Invoke(null, null);
            if (currentWorld == null || zone == null || dungeon == null || gameInstance.GetValue(null, null) == null ||
                !(bool)enabled.GetValue(currentServer, null) || !(bool)dedicated.Invoke(currentServer, null) ||
                (bool)loadError.GetValue(null) || (bool)zoneSkip.Invoke(zone, null) ||
                (bool)dungeonSkip.Invoke(dungeon, null)) throw new ControlFailure("save_not_ready");
            if ((bool)isSaving.Invoke(currentServer, null) || (bool)saving.GetValue(null))
                throw new ControlFailure("save_busy");
            // Use the native server/autosave disk check, with no exit prompt or
            // decision callback. RPC_Save/HardSaveBlock is a client Nintendo rate
            // limit and dereferences a DistributionPlatform absent on the DS.
            object[] diskArguments = { false, false, null };
            bool enoughDisk = (bool)diskSpace.Invoke(currentServer, diskArguments);
            if ((bool)diskArguments[0]) throw new ControlFailure("unsupported_native_save_contract");
            if (!enoughDisk) throw new ControlFailure("save_disk_space_rejected");
            server = currentServer;
            savedWorld = currentWorld;
            previous = (uint)committed.GetValue(null);
            starts = 0;
            started.SetValue(null, Delegate.Combine((Action)started.GetValue(null), onStarted));
            observing = true;
            try
            {
                // The dedicated native Save checks session/load/zone/dungeon
                // state itself. sync=false keeps its worker asynchronous,
                // saveOtherPlayerProfiles=true notifies connected clients,
                // waitForNextFrame=false creates that worker before returning.
                save.Invoke(server, new object[] { false, true, false });
                ownedThread = (Thread)thread.GetValue(server);
                if (Volatile.Read(ref starts) != 1 || ownedThread == null)
                    throw new ControlFailure(starts == 0 && ownedThread == null
                        ? "save_not_started" : "save_outcome_unknown");
            }
            catch (ControlFailure) { Dispose(); throw; }
            catch (Exception) { Dispose(); throw new ControlFailure("save_outcome_unknown"); }
        }

        internal WorldSaveReceipt Poll(object currentServer)
        {
            if (!observing) throw new ControlFailure("save_outcome_unknown");
            try
            {
                bool subscribed = false;
                Action handlers = (Action)started.GetValue(null);
                if (handlers != null) foreach (Delegate handler in handlers.GetInvocationList())
                    if (handler.Equals(onStarted)) subscribed = true;
                if (!Object.ReferenceEquals(server, currentServer) ||
                    !Object.ReferenceEquals(savedWorld, world.Invoke(null, null)) ||
                    Volatile.Read(ref starts) != 1 || !subscribed)
                    throw new ControlFailure("save_outcome_unknown");
                object active = thread.GetValue(server);
                if (active != null && !Object.ReferenceEquals(active, ownedThread))
                    throw new ControlFailure("save_outcome_unknown");
                // A zero-time join never waits and synchronizes a completed
                // worker's writes; IsAlive alone is not a commit barrier.
                // GetSaveNumber() is deliberately not used: while saving it
                // returns the uncommitted generation, including failed saves.
                if (!ownedThread.Join(0)) return null;
                uint number = (uint)committed.GetValue(null);
                if ((bool)saving.GetValue(null)) throw new ControlFailure("save_outcome_unknown");
                if (number == previous) throw new ControlFailure("save_failed");
                if (number != unchecked(previous + 1)) throw new ControlFailure("save_outcome_unknown");
                WorldSaveReceipt result = new WorldSaveReceipt { Previous = previous, Committed = number };
                Dispose();
                return result;
            }
            catch { Dispose(); throw; }
        }

        public void Dispose()
        {
            if (observing)
                started.SetValue(null, Delegate.Remove((Action)started.GetValue(null), onStarted));
            observing = false;
            ownedThread = null;
            server = savedWorld = null;
        }
    }
}

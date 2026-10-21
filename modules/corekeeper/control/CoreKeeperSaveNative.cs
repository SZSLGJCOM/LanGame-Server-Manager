using System;
using System.Reflection;
using System.Threading;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    internal sealed class CoreSaveTarget
    {
        private const BindingFlags Instance = BindingFlags.Public | BindingFlags.Instance;
        private readonly object ecs, world, saves, serializer, serializationGroup, beginCommands, prepareChunks;
        private readonly PropertyInfo nativeEcs, worldProperty, loaded, closing, pending, unmanaged;
        private readonly MethodInfo queue, updateSerialization, updateCommands, updateChunks;
        private readonly MethodInfo readState;
        private readonly Type stateData, trigger;
        private readonly FieldInfo lastWrite, state;
        private readonly object idle;
        private readonly DateTime previousWrite;
        private readonly int ownerThread = Thread.CurrentThread.ManagedThreadId;

        internal CoreSaveTarget(string entitiesHash, string componentsHash)
        {
            Type manager = NativeBinding.Type("Pug.Other", "Manager");
            nativeEcs = Property(manager, "ecs", BindingFlags.Public | BindingFlags.Static);
            ecs = nativeEcs.GetValue(null, null);
            if (ecs == null) throw new SaveFailure("world_not_ready");
            loaded = Property(ecs.GetType(), "WorldsAreLoaded", Instance);
            closing = Property(ecs.GetType(), "IsClosing", Instance);
            worldProperty = Property(ecs.GetType(), "ServerWorld", Instance);
            world = worldProperty.GetValue(ecs, null);
            if (world == null || !(bool)loaded.GetValue(ecs, null) || (bool)closing.GetValue(ecs, null))
                throw new SaveFailure("world_not_ready");
            NativeBinding.VerifyFile(world.GetType().Assembly.Location, entitiesHash);
            Type objects = NativeBinding.Type("Pug.ECS.Components", "ObjectDataCD");
            NativeBinding.VerifyFile(objects.Assembly.Location, componentsHash);
            RequireSaveableObjects(objects);
            saves = System(world, NativeBinding.Type("Pug.Other", "SaveSystem"));
            serializer = System(world, NativeBinding.Type("Pug.Other", "SerializeWorldSystem"));
            if (saves == null || serializer == null) throw new SaveFailure("world_not_ready");
            pending = Property(saves.GetType(), "SaveIsPending", Instance);
            if ((bool)pending.GetValue(saves, null)) throw new SaveFailure("save_busy");
            queue = NativeBinding.Method(ecs.GetType(), "QueueNewServerSave", Instance, typeof(bool));
            lastWrite = serializer.GetType().GetField("LastWorldWrite", Instance);
            if (lastWrite == null || lastWrite.FieldType != typeof(DateTime)) throw new SaveFailure("api_unavailable");
            previousWrite = (DateTime)lastWrite.GetValue(serializer);
            serializationGroup = System(world, NativeBinding.Type("Pug.Other", "SerializationSystemGroup"));
            beginCommands = System(world, NativeBinding.Type("Unity.Entities", "Unity.Entities.BeginSimulationEntityCommandBufferSystem"));
            if (serializationGroup == null || beginCommands == null) throw new SaveFailure("world_not_ready");
            updateSerialization = NativeBinding.Method(serializationGroup.GetType(), "Update", Instance, typeof(void));
            updateCommands = NativeBinding.Method(beginCommands.GetType(), "Update", Instance, typeof(void));
            Type handle = NativeBinding.Type("Unity.Entities", "Unity.Entities.SystemHandle");
            MethodInfo existing = NativeBinding.Method(world.GetType(), "GetExistingSystem", Instance, handle, typeof(Type));
            prepareChunks = existing.Invoke(world, new object[] { NativeBinding.Type("Pug.Other", "SerializeWorldRunSystem") });
            unmanaged = Property(world.GetType(), "Unmanaged", Instance);
            updateChunks = NativeBinding.Method(prepareChunks.GetType(), "Update", Instance, typeof(void), unmanaged.PropertyType);
            stateData = NativeBinding.Type("Pug.Other", "SerializeWorldDataCD");
            trigger = NativeBinding.Type("Pug.Other", "SerializeWorldSystem+SerializeWorld");
            state = stateData.GetField("State", Instance);
            if (state == null || !state.FieldType.IsEnum) throw new SaveFailure("api_unavailable");
            idle = Enum.Parse(state.FieldType, "Idle");
            Type queryType = NativeBinding.Type("Unity.Entities", "Unity.Entities.EntityQuery");
            MethodInfo singleton = queryType.GetMethod("GetSingleton", Instance, null, Type.EmptyTypes, null);
            if (singleton == null || !singleton.IsGenericMethodDefinition) throw new SaveFailure("api_unavailable");
            readState = singleton.MakeGenericMethod(stateData);
        }

        private void Validate()
        {
            if (Thread.CurrentThread.ManagedThreadId != ownerThread ||
                !Object.ReferenceEquals(ecs, nativeEcs.GetValue(null, null)) ||
                !(bool)loaded.GetValue(ecs, null) || (bool)closing.GetValue(ecs, null) ||
                !Object.ReferenceEquals(world, worldProperty.GetValue(ecs, null)) ||
                !Object.ReferenceEquals(saves, System(world, saves.GetType())) ||
                !Object.ReferenceEquals(serializer, System(world, serializer.GetType())))
                throw new SaveFailure("target_changed");
        }
        internal bool Submit()
        {
            Validate();
            if ((bool)pending.GetValue(saves, null)) throw new SaveFailure("save_busy");
            return (bool)queue.Invoke(ecs, null);
        }
        private void RequireSaveableObjects(Type objects)
        {
            // A metadata-only new world cannot pass the native loader's
            // ObjectDataSerializedCD requirement. Do not publish such a save.
            object query = Query(objects, NativeBinding.Type("Pug.ECS.Components", "DontSerializeCD"));
            try { if (IsEmpty(query)) throw new SaveFailure("world_not_ready"); }
            finally { ((IDisposable)query).Dispose(); }
        }
        private object Query(Type required, Type excluded = null)
        {
            object manager = Property(world.GetType(), "EntityManager", Instance).GetValue(world, null);
            Type component = NativeBinding.Type("Unity.Entities", "Unity.Entities.ComponentType");
            const BindingFlags Static = BindingFlags.Public | BindingFlags.Static;
            MethodInfo read = NativeBinding.Method(component, "ReadOnly", Static, component, typeof(Type));
            MethodInfo exclude = NativeBinding.Method(component, "Exclude", Static, component, typeof(Type));
            Array types = Array.CreateInstance(component, excluded == null ? 1 : 2);
            types.SetValue(read.Invoke(null, new object[] { required }), 0);
            if (excluded != null) types.SetValue(exclude.Invoke(null, new object[] { excluded }), 1);
            Type queryType = NativeBinding.Type("Unity.Entities", "Unity.Entities.EntityQuery");
            MethodInfo create = NativeBinding.Method(manager.GetType(), "CreateEntityQuery", Instance, queryType, component.MakeArrayType());
            return create.Invoke(manager, new object[] { types });
        }
        private static bool IsEmpty(object query)
        {
            return (bool)Property(query.GetType(), "IsEmpty", Instance).GetValue(query, null);
        }
        private bool QueueDrained()
        {
            object data = Query(stateData);
            try
            {
                if (!idle.Equals(state.GetValue(readState.Invoke(data, null)))) return false;
            }
            finally { ((IDisposable)data).Dispose(); }
            object events = Query(trigger);
            try { return IsEmpty(events); }
            finally { ((IDisposable)events).Dispose(); }
        }
        internal bool Advance()
        {
            Validate();
            // This native preparation system normally runs in the simulation
            // group. Empty servers pause that group, so new chunks otherwise
            // never become eligible for serialization. Flush the queued event,
            // prepare its chunks, then use the native SaveDone drain sequence.
            updateCommands.Invoke(beginCommands, null);
            updateChunks.Invoke(prepareChunks, new[] { unmanaged.GetValue(world, null) });
            updateSerialization.Invoke(serializationGroup, null);
            updateCommands.Invoke(beginCommands, null);
            updateSerialization.Invoke(serializationGroup, null);
            Validate();
            // The game sets this after handing bytes to its filesystem queue.
            // It ends our bounded pump; it is never a disk-completion receipt.
            // An earlier automatic save can write while our queued event is
            // still pending. Keep advancing until the native queue is idle.
            return (DateTime)lastWrite.GetValue(serializer) != previousWrite && QueueDrained();
        }
        private static PropertyInfo Property(Type type, string name, BindingFlags flags)
        {
            PropertyInfo property = type.GetProperty(name, flags);
            if (property == null) throw new SaveFailure("api_unavailable");
            return property;
        }
        private static object System(object world, Type system)
        {
            MethodInfo selected = null;
            foreach (MethodInfo method in world.GetType().GetMethods(Instance))
                if (method.Name == "GetExistingSystemManaged" && method.IsGenericMethodDefinition &&
                    method.GetGenericArguments().Length == 1 && method.GetParameters().Length == 0)
                { if (selected != null) throw new SaveFailure("api_unavailable"); selected = method; }
            if (selected == null) throw new SaveFailure("api_unavailable");
            return selected.MakeGenericMethod(system).Invoke(world, null);
        }
    }
}

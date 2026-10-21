using System;
using System.Reflection;
using LanGame.SaveControl;

namespace LanGame.CoreKeeper
{
    // All mutation is confined to creating the game's ordinary SerializeWorld
    // event, once, on IMod.Update. Observers only read existing native objects.
    internal sealed class CoreSaveTarget
    {
        private const BindingFlags Instance = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance;
        internal readonly object World, Serializer, Filesystem, Platform, File;
        private readonly object ecs, saves, manager, components;
        private readonly PropertyInfo worldProperty, loaded;
        private readonly MethodInfo create, matches, exists, worldId;
        private readonly FieldInfo trigger;
        private readonly int id;
        private readonly Type entityType;
        private readonly string relativePath;
        private readonly PropertyInfo nativeSaves, nativeFilesystem;
        private readonly object serializationGroup, beginCommands;
        private readonly MethodInfo updateSerialization, updateCommands;
        internal object Manager { get { return manager; } }

        internal CoreSaveTarget()
        {
            Type nativeManager = NativeBinding.Type("Pug.Other", "Manager");
            ecs = Property(nativeManager, "ecs", BindingFlags.Public | BindingFlags.Static).GetValue(null, null);
            if (ecs == null) throw new SaveFailure("world_not_ready");
            loaded = Property(ecs.GetType(), "WorldsAreLoaded", Instance);
            worldProperty = Property(ecs.GetType(), "ServerWorld", Instance);
            World = worldProperty.GetValue(ecs, null);
            if (World == null || !(bool)loaded.GetValue(ecs, null)) throw new SaveFailure("world_not_ready");
            Type saveType = NativeBinding.Type("Pug.Other", "SaveSystem");
            saves = System(World, saveType);
            if (saves == null) throw new SaveFailure("world_not_ready");
            PropertyInfo pending = Property(saveType, "SaveIsPending", Instance);
            if (pending.PropertyType != typeof(bool) || (bool)pending.GetValue(saves, null)) throw new SaveFailure("save_busy");
            Type serializer = NativeBinding.Type("Pug.Other", "SerializeWorldSystem");
            Serializer = System(World, serializer);
            if (Serializer == null) throw new SaveFailure("world_not_ready");
            trigger = Field(serializer, "triggerQ");
            nativeSaves = Property(nativeManager, "saves", BindingFlags.Public | BindingFlags.Static);
            nativeFilesystem = Property(nativeManager, "filesystemManager", BindingFlags.Public | BindingFlags.Static);
            manager = nativeSaves.GetValue(null, null);
            Filesystem = nativeFilesystem.GetValue(null, null);
            if (manager == null || Filesystem == null) throw new SaveFailure("world_not_ready");
            if (!CoreSaveHooks.WriterArmed(Filesystem)) throw new SaveFailure("api_unavailable");
            Platform = Field(Filesystem.GetType(), "platformImpl").GetValue(Filesystem);
            if (Platform == null || Platform.GetType() != nativeManager.Assembly.GetType("Pug.Platform.StandaloneFilesystem", true))
                throw new SaveFailure("api_unavailable");
            worldId = NativeBinding.Method(manager.GetType(), "GetWorldId", Instance, typeof(int));
            id = (int)worldId.Invoke(manager, null);
            Array files = (Array)Field(manager.GetType(), "worldDataFiles").GetValue(manager);
            if (id < 0 || id >= files.Length) throw new SaveFailure("world_not_ready");
            File = files.GetValue(id);
            if ((int)Field(File.GetType(), "fileTypeId").GetValue(File) != 3) throw new SaveFailure("api_unavailable");
            relativePath = (string)NativeBinding.Method(Filesystem.GetType(), "GetFilePath", Instance,
                typeof(string), File.GetType()).Invoke(Filesystem, new[] { File });
            if (String.IsNullOrEmpty(relativePath)) throw new SaveFailure("api_unavailable");
            object entityManager = Property(World.GetType(), "EntityManager", Instance).GetValue(World, null);
            Type entities = entityManager.GetType();
            NativeBinding.VerifyFile(entities.Assembly.Location,
                "d73687d44b81ceccb1f0416cd373d75e2dc58d8d998ed26310e31f48650a6c1d");
            Type component = entities.Assembly.GetType("Unity.Entities.ComponentType", true);
            entityType = entities.Assembly.GetType("Unity.Entities.Entity", true);
            Type eventType = serializer.GetNestedType("SerializeWorld", BindingFlags.Public | BindingFlags.NonPublic);
            if (eventType == null) throw new SaveFailure("api_unavailable");
            Array types = Array.CreateInstance(component, 1);
            types.SetValue(NativeBinding.Method(component, "op_Implicit", BindingFlags.Public | BindingFlags.Static,
                component, typeof(Type)).Invoke(null, new object[] { eventType }), 0);
            components = types;
            create = NativeBinding.Method(entities, "CreateEntity", Instance, entityType, component.MakeArrayType());
            exists = NativeBinding.Method(entities, "Exists", Instance, typeof(bool), entityType);
            matches = NativeBinding.Method(trigger.FieldType, "Matches", Instance, typeof(bool), entityType);
            // SaveSystem.SaveDone uses this same group/ECB/group sequence to
            // drain serialization without advancing the simulation clock.
            // An empty dedicated server has timescale=0, so merely creating
            // the ordinary save entity can otherwise wait indefinitely.
            serializationGroup = System(World, NativeBinding.Type("Pug.Other", "SerializationSystemGroup"));
            beginCommands = System(World, entities.Assembly.GetType("Unity.Entities.BeginSimulationEntityCommandBufferSystem", true));
            if (serializationGroup == null || beginCommands == null) throw new SaveFailure("world_not_ready");
            updateSerialization = NativeBinding.Method(serializationGroup.GetType(), "Update", Instance, typeof(void));
            updateCommands = NativeBinding.Method(beginCommands.GetType(), "Update", Instance, typeof(void));
        }

        internal void Validate()
        {
            if (!(bool)loaded.GetValue(ecs, null) || !Object.ReferenceEquals(World, worldProperty.GetValue(ecs, null)) ||
                !Object.ReferenceEquals(Serializer, System(World, Serializer.GetType())) ||
                !Object.ReferenceEquals(manager, nativeSaves.GetValue(null, null)) ||
                !Object.ReferenceEquals(Filesystem, nativeFilesystem.GetValue(null, null)) ||
                !Object.ReferenceEquals(Platform, Field(Filesystem.GetType(), "platformImpl").GetValue(Filesystem)) ||
                !File.Equals(((Array)Field(manager.GetType(), "worldDataFiles").GetValue(manager)).GetValue(id)) ||
                (int)worldId.Invoke(manager, null) != id)
                throw new SaveFailure("target_changed");
        }

        internal object Submit()
        {
            Validate();
            object entityManager = Property(World.GetType(), "EntityManager", Instance).GetValue(World, null);
            // QueueNewSave's admitted IL only records this component creation in
            // an ECB. Immediate native creation gives this request a real Entity
            // Index+Version before another update can consume the event.
            return create.Invoke(entityManager, new[] { components });
        }

        internal bool Consumes(object serializer, object entity)
        {
            if (!Object.ReferenceEquals(serializer, Serializer) || entity == null || entity.GetType() != entityType) return false;
            Validate();
            object entityManager = Property(World.GetType(), "EntityManager", Instance).GetValue(World, null);
            return (bool)exists.Invoke(entityManager, new[] { entity }) &&
                (bool)matches.Invoke(trigger.GetValue(Serializer), new[] { entity });
        }

        internal bool SameFile(object value) { return File.Equals(value); }
        internal void Pump()
        {
            Validate();
            updateSerialization.Invoke(serializationGroup, null);
            updateCommands.Invoke(beginCommands, null);
            updateSerialization.Invoke(serializationGroup, null);
            Validate();
        }
        internal string RelativePath() { return relativePath; }
        internal static FieldInfo Field(Type type, string name)
        {
            FieldInfo field = type.GetField(name, Instance);
            if (field == null) throw new SaveFailure("api_unavailable");
            return field;
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

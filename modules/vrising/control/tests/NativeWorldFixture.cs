using System;

// Boundary doubles for the generated IL2CPP objects; no native game is loaded.
namespace ProjectM
{
    public sealed class ServerBootstrapSystem
    {
        public IntPtr Pointer { get { return new IntPtr(1); } }
        public bool ServerIsInitialized { get; set; }
        public World World { get; set; }
        public readonly ServerRuntimeSettings Settings = new ServerRuntimeSettings();
        public ServerRuntimeSettings GetServerRuntimeSettings() { return Settings; }
        public void OnUpdate() { }
    }
    public sealed class World
    {
        public IntPtr Pointer { get { return new IntPtr(2); } }
        public bool IsCreated { get; set; }
        public ulong SequenceNumber { get; set; }
        public readonly TriggerPersistenceSaveSystem Saves = new TriggerPersistenceSaveSystem();
        public readonly SerializePersistenceSystemV2 Serializer = new SerializePersistenceSystemV2();
        public T GetExistingSystemManaged<T>() where T : class
        { return (typeof(T) == typeof(TriggerPersistenceSaveSystem) ? (object)Saves : Serializer) as T; }
    }
    public sealed class TriggerPersistenceSaveSystem
    {
        public IntPtr Pointer { get { return new IntPtr(3); } }
        public bool Ready = true, Accepted = true, ThrowOnSave;
        public int Calls;
        public ServerRuntimeSettings ReceivedSettings;
        public bool IsReadyToSave(ServerRuntimeSettings settings) { return Ready; }
        public bool TriggerAutoSave(ServerRuntimeSettings settings)
        {
            Calls++; ReceivedSettings = settings;
            if (ThrowOnSave) throw new InvalidOperationException("native invocation interrupted");
            return Accepted;
        }
    }
    public sealed class SerializePersistenceSystemV2
    {
        public IntPtr Pointer { get { return new IntPtr(4); } }
        public bool IsSaving { get; set; }
    }
}

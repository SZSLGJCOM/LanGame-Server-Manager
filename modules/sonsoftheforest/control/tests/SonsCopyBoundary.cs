using System.Runtime.CompilerServices;

// Test-only external IL2CPP boundary. The observer and Harmony hooks are real.
namespace Il2CppSystem.IO
{
    public static class File
    {
        [MethodImpl(MethodImplOptions.NoInlining)]
        public static void Copy(string source, string destination, bool overwrite)
        { System.IO.File.Copy(source, destination, overwrite); }
    }
}
namespace Il2CppSystem
{
    public class Object
    {
        public Object(System.IntPtr pointer) { Pointer = pointer; }
        public System.IntPtr Pointer { get; private set; }
        public Type GetIl2CppType() { return new Type(GetType()); }
    }
    public sealed class Type : Object
    {
        internal readonly System.Type Managed;
        public Type(System.Type type) : base(type.TypeHandle.Value) { Managed = type; }
        public string FullName { get { return Managed.FullName; } }
        public Reflection.MethodInfo GetMethod(string name) { return new Reflection.MethodInfo(Managed.GetMethod(name)); }
    }
    public class Delegate
    {
        protected Object target;
        protected Reflection.MethodInfo method;
        protected System.Delegate callback;
        public Object Target { get { return target; } }
        public Reflection.MethodInfo Method { get { return method; } }
        public bool NativeCompatible { get { return method.Managed.DeclaringType.IsInstanceOfType(target); } }
        public static Delegate CreateDelegate(Type type, Object instance, Reflection.MethodInfo method)
        {
            if (!method.Managed.DeclaringType.IsInstanceOfType(instance)) throw new System.ArgumentException("method target mismatch");
            return new Delegate {target = instance, method = method};
        }
        public T Cast<T>() where T : Delegate, new()
        { T value = new T(); value.target = target; value.method = method; value.callback = callback; return value; }
    }
    public sealed class Action : Delegate
    {
        public static implicit operator Action(System.Action callback)
        {
            // The real conversion exposes Action.Invoke with a reference-holder
            // target. Native reflection cannot later unregister that pairing.
            return new Action {callback = callback, target = new Object(new System.IntPtr(1)),
                method = new Reflection.MethodInfo(typeof(Action).GetMethod("Invoke"))};
        }
        public void Invoke() { if (callback != null) ((System.Action)callback)(); else method.Managed.Invoke(target, null); }
    }
    public sealed class Action<T> : Delegate
    {
        public static implicit operator Action<T>(System.Action<T> callback)
        { return new Action<T> {callback = callback, target = new Object(new System.IntPtr(1)),
            method = new Reflection.MethodInfo(typeof(Action<T>).GetMethod("Invoke"))}; }
        public void Invoke(T value) { if (callback != null) ((System.Action<T>)callback)(value); else method.Managed.Invoke(target, new object[] {value}); }
    }
}
namespace Il2CppSystem.Reflection
{
    public class MemberInfo : Il2CppSystem.Object
    {
        internal readonly System.Reflection.MethodInfo Managed;
        internal MemberInfo(System.Reflection.MethodInfo method) : base(method.MethodHandle.Value) { Managed = method; }
        public string Name { get { return Managed.Name; } }
        public Il2CppSystem.Type DeclaringType { get { return new Il2CppSystem.Type(Managed.DeclaringType); } }
    }
    public class MethodBase : MemberInfo
    {
        internal MethodBase(System.Reflection.MethodInfo method) : base(method) { }
        public bool IsStatic { get { return Managed.IsStatic; } }
        public ParameterInfo[] GetParameters()
        { return System.Array.ConvertAll(Managed.GetParameters(), parameter => new ParameterInfo(parameter.ParameterType)); }
    }
    public sealed class MethodInfo : MethodBase
    {
        public MethodInfo(System.Reflection.MethodInfo method) : base(method) { }
        public Il2CppSystem.Type ReturnType { get { return new Il2CppSystem.Type(Managed.ReturnType); } }
    }
    public sealed class ParameterInfo
    {
        private readonly System.Type type;
        public ParameterInfo(System.Type type) { this.type = type; }
        public Il2CppSystem.Type ParameterType { get { return new Il2CppSystem.Type(type); } }
    }
}

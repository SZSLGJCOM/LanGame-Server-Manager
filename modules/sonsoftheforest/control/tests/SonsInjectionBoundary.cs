using System;
using System.Reflection;
[assembly: AssemblyVersion("1.4.6.0")]

// Test-only official injection boundary. Production emits the actual receiver
// and validates native delegates; this boundary never pretends to run IL2CPP.
namespace Il2CppInterop.Runtime
{
    public sealed class Il2CppException : Exception
    {
        public Il2CppException(string nativeText) : base(nativeText) { }
    }
    public static class Il2CppType
    {
        public static Il2CppSystem.Type Of<T>() { return new Il2CppSystem.Type(typeof(T)); }
    }
}
namespace Il2CppInterop.Runtime.Injection
{
    public static class ClassInjector
    {
        private static int pointer;
        public static Type Registered;
        public static int Registrations;
        public static IntPtr DerivedConstructorPointer<T>()
        { return new IntPtr(System.Threading.Interlocked.Increment(ref pointer)); }
        public static void DerivedConstructorBody(object instance)
        { if (instance.GetType() != Registered) throw new InvalidOperationException("unregistered receiver"); }
        public static void RegisterTypeInIl2Cpp(Type type)
        {
            if (Registered != null) throw new InvalidOperationException("duplicate injection");
            if (!type.IsPublic || type.IsGenericType || type.GetFields(BindingFlags.Instance | BindingFlags.DeclaredOnly |
                BindingFlags.Public | BindingFlags.NonPublic).Length != 0) throw new InvalidOperationException("receiver contract");
            Registered = type; Registrations++;
        }
    }
}

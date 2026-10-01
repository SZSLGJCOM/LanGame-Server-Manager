using System;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using System.Threading;

internal static class ForestBclContractTests
{
    // These calls are absent or unsafe in Unity 5.6's bundled Mono BCL.
    // Examine emitted IL, including generated closures, rather than C# spelling.
    internal static void Verify(string file)
    {
        // Loading our BCL-only assembly does not invoke its native-loader entry.
        Assembly bridge = Assembly.LoadFrom(file);
        foreach (AssemblyName reference in bridge.GetReferencedAssemblies())
            if (reference.Name != "mscorlib" && reference.Name != "System")
                throw new Exception("The control bridge must not reference a private game SDK");
        if (HasUnsupportedTypeReference(bridge.ManifestModule) ||
            !HasUnsupportedTypeReference(typeof(ForestBclContractTests).Module))
            throw new Exception("Old-Mono type reference contract or its negative control failed");
        foreach (Type type in bridge.GetTypes())
        {
            foreach (MethodInfo method in type.GetMethods(BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance | BindingFlags.DeclaredOnly))
                if (HasUnsupportedCall(method))
                {
                    Console.Error.WriteLine("Unsupported old-Mono call in " + type.Name + "." + method.Name);
                    throw new Exception("Unsupported old-Mono call");
                }
        }
        if (!HasUnsupportedCall(typeof(ForestBclContractTests).GetMethod("UnsupportedLock", BindingFlags.NonPublic | BindingFlags.Static)) ||
            !HasUnsupportedCall(typeof(ForestBclContractTests).GetMethod("UnsupportedReflection", BindingFlags.NonPublic | BindingFlags.Static)) ||
            !HasUnsupportedCall(typeof(ForestBclContractTests).GetMethod("UnsupportedDelegate", BindingFlags.NonPublic | BindingFlags.Static)) ||
            !HasUnsupportedCall(typeof(ForestBclContractTests).GetMethod("UnsafeFileHandle", BindingFlags.NonPublic | BindingFlags.Static)))
            throw new Exception("BCL compatibility regression detector missed its negative controls");
        Console.WriteLine("The Forest Mono BCL: emitted calls and four negative controls passed");
    }

    private static void UnsupportedLock(object gate) { lock (gate) Monitor.PulseAll(gate); }
    private static bool UnsupportedReflection(MethodInfo value) { return value == null; }
    private static Action UnsupportedDelegate() { return delegate() { }; }
    private static Microsoft.Win32.SafeHandles.SafeFileHandle UnsafeFileHandle(System.IO.FileStream stream) { return stream.SafeFileHandle; }

    private static bool HasUnsupportedTypeReference(Module module)
    {
        // TypeRef rows are contiguous metadata tokens. Bound the scan for this
        // small DLL; this also finds unused field/signature/typeof references.
        for (int token = 0x01000001; token < 0x01010000; token++)
        {
            Type type;
            try { type = module.ResolveType(token); }
            catch (ArgumentOutOfRangeException) { return false; }
            if (type.FullName == "System.Action" && type.Assembly.GetName().Name == "mscorlib") return true;
        }
        throw new Exception("Control bridge type reference count exceeded its bound");
    }

    private static bool HasUnsupportedCall(MethodInfo method)
    {
        MethodBody body = method.GetMethodBody();
        if (body == null) return false;
        Dictionary<ushort, OpCode> codes = new Dictionary<ushort, OpCode>();
        foreach (FieldInfo field in typeof(OpCodes).GetFields(BindingFlags.Public | BindingFlags.Static))
            if (field.FieldType == typeof(OpCode)) { OpCode code = (OpCode)field.GetValue(null); codes[(ushort)code.Value] = code; }
        byte[] bytes = body.GetILAsByteArray();
        for (int at = 0; at < bytes.Length; )
        {
            ushort value = bytes[at++];
            if (value == 0xfe) value = (ushort)(0xfe00 | bytes[at++]);
            OpCode code = codes[value];
            if (code.OperandType == OperandType.InlineMethod)
            {
                MethodBase call = method.Module.ResolveMethod(BitConverter.ToInt32(bytes, at));
                string owner = call.DeclaringType.FullName;
                if (owner == "System.Action" || (owner == "System.Threading.Monitor" && call.Name == "Enter" && call.GetParameters().Length != 1) ||
                    (owner == "System.IO.FileStream" && call.Name == "get_SafeFileHandle") ||
                    ((owner.StartsWith("System.Reflection.", StringComparison.Ordinal) || owner == "System.Type") && call.Name.StartsWith("op_", StringComparison.Ordinal))) return true;
            }
            switch (code.OperandType)
            {
                case OperandType.InlineNone: break;
                case OperandType.ShortInlineBrTarget: case OperandType.ShortInlineI: case OperandType.ShortInlineVar: at++; break;
                case OperandType.InlineVar: at += 2; break;
                case OperandType.InlineI8: case OperandType.InlineR: at += 8; break;
                case OperandType.InlineSwitch: at += 4 + 4 * BitConverter.ToInt32(bytes, at); break;
                default: at += 4; break;
            }
        }
        return false;
    }
}

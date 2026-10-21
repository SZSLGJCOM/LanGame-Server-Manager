using System;
using System.Collections;
using System.Collections.Generic;
using System.Reflection;
using System.Reflection.Emit;
using System.Runtime.ExceptionServices;
using System.Threading;
using LanGame.SaveControl;

namespace LanGame.Sons
{
    // Use the game's callback registry. An IL2CPP Harmony trampoline catches
    // original exceptions, so detouring File.Copy changes native failure behavior.
    internal sealed class NativeSaveCompletion : IDisposable
    {
        [ThreadStatic] private static Capture current;
        private MethodInfo registerBefore, unregisterBefore, registerAfter, unregisterAfter;
        private Type beforeType, afterType;
        private static Capture retained;
        private bool disposed;

        internal static NativeSaveCompletion Install()
        {
            NativeSaveCompletion observer = new NativeSaveCompletion();
            Type manager = NativeBinding.Type("Sons.Save", "Sons.Save.SaveGameManager");
            observer.beforeType = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Action");
            observer.afterType = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Action`1").MakeGenericType(typeof(bool));
            BindingFlags flags = BindingFlags.Public | BindingFlags.Static;
            observer.registerBefore = NativeBinding.Method(manager, "RegisterBeforeSaveCallback", flags, typeof(void), observer.beforeType);
            observer.unregisterBefore = NativeBinding.Method(manager, "UnregisterBeforeSaveCallback", flags, typeof(void), observer.beforeType);
            observer.registerAfter = NativeBinding.Method(manager, "RegisterAfterSaveCallback", flags, typeof(void), observer.afterType);
            observer.unregisterAfter = NativeBinding.Method(manager, "UnregisterAfterSaveCallback", flags, typeof(void), observer.afterType);
            return observer;
        }
        internal Capture Begin()
        {
            if (disposed || retained != null) throw new SaveFailure("api_unavailable");
            if (current != null) throw new SaveFailure("save_busy");
            Capture capture = current = new Capture(this);
            try
            {
                NativeCallbackReceiver.Get(beforeType, afterType, out capture.Before, out capture.After);
                capture.BeforeAttempted = true;
                registerBefore.Invoke(null, new[] {capture.Before});
                capture.AfterAttempted = true;
                registerAfter.Invoke(null, new[] {capture.After});
                return capture;
            }
            catch (Exception primary)
            {
                try { capture.Dispose(); }
                catch (Exception cleanup) { throw new AggregateException(primary, cleanup); }
                throw;
            }
        }
        public void Dispose()
        {
            disposed = true;
            if (current != null && current.Owner == this) current.Dispose();
        }
        internal static void BeforeCurrent() { if (current != null) current.BeforeSave(); }
        internal static void AfterCurrent(bool playerOnly) { if (current != null) current.AfterSave(playerOnly); }
        internal static SaveReceipt Invoke(Capture capture, Func<SaveReceipt> invoke)
        {
            SaveReceipt receipt;
            InvocationDiagnostics diagnostic = null;
            try
            {
                if (Environment.GetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE") == "true")
                    diagnostic = new InvocationDiagnostics(capture);
                Exception invocationError = null;
                try { receipt = invoke(); }
                catch (Exception error) { invocationError = error; throw; }
                finally
                {
                    try { if (diagnostic != null) diagnostic.Dispose(); }
                    catch (Exception cleanup)
                    {
                        if (invocationError != null) throw new AggregateException(invocationError, cleanup);
                        throw;
                    }
                }
                if (receipt == SaveReceipt.Completed) receipt = capture.Receipt();
            }
            catch (Exception primary)
            {
                if (diagnostic != null) diagnostic.Attach(primary);
                try { capture.Dispose(); }
                catch (Exception cleanup) { throw new AggregateException(primary, cleanup); }
                throw;
            }
            // A successful native write cannot be reported completed when its
            // observer failed to unregister. Never mask an original write error.
            capture.Dispose();
            return receipt;
        }
        private sealed class InvocationDiagnostics : IDisposable
        {
            private readonly Capture capture;
            private readonly EventHandler<FirstChanceExceptionEventArgs> handler;
            private Exception first;
            private bool handling, unavailable;
            internal InvocationDiagnostics(Capture capture)
            {
                this.capture = capture; handler = Observe;
                AppDomain.CurrentDomain.FirstChanceException += handler;
            }
            private void Observe(object sender, FirstChanceExceptionEventArgs args)
            {
                if (handling || first != null || !capture.IsCurrentOwnerThread()) return;
                handling = true;
                try
                {
                    Exception error = args.Exception;
                    if (!(error is System.IO.IOException) && error.GetType().FullName != "Il2CppInterop.Runtime.Il2CppException") return;
                    string message = error.Message ?? "", stack = error.StackTrace ?? "";
                    if (message.Length > 65536 || stack.Length > 65536) return;
                    if ((error is System.IO.IOException || message.IndexOf("IOException", StringComparison.Ordinal) >= 0) &&
                        message.IndexOf("SaveData.zip", StringComparison.OrdinalIgnoreCase) >= 0 &&
                        (message.IndexOf("SaveGameManager.WriteJsonZip", StringComparison.Ordinal) >= 0 ||
                         stack.IndexOf("SaveGameManager.WriteJsonZip", StringComparison.Ordinal) >= 0)) first = error;
                }
                catch (Exception) { unavailable = true; }
                finally { handling = false; }
            }
            internal void Attach(Exception failure)
            {
                try
                {
                    if (first != null && first != failure) failure.Data["sons_native_write_exception"] = first;
                    if (unavailable) failure.Data["sons_native_diagnostic_unavailable"] = true;
                }
                catch (Exception) { unavailable = true; }
            }
            public void Dispose() { AppDomain.CurrentDomain.FirstChanceException -= handler; }
        }
        internal sealed class Capture : IDisposable
        {
            internal readonly NativeSaveCompletion Owner;
            private readonly int thread = Thread.CurrentThread.ManagedThreadId;
            internal object Before, After;
            internal bool BeforeAttempted, AfterAttempted;
            private int beforeCount, afterCount;
            private bool invalid, closed;
            internal Capture(NativeSaveCompletion owner) { Owner = owner; }
            internal void BeforeSave()
            {
                if (current != this || Thread.CurrentThread.ManagedThreadId != thread || closed) return;
                beforeCount++;
            }
            internal void AfterSave(bool savePlayerOnly)
            {
                if (current != this || Thread.CurrentThread.ManagedThreadId != thread || closed) return;
                afterCount++;
                if (savePlayerOnly || beforeCount != 1) invalid = true;
            }
            internal SaveReceipt Receipt()
            {
                if (current != this || closed || invalid || beforeCount != 1 || afterCount != 1)
                {
                    SaveFailure failure = new SaveFailure("native_outcome_unknown");
                    if (Environment.GetEnvironmentVariable("LANGAME_NATIVE_SAVE_FAILURE_ACCEPTANCE") == "true")
                        failure.Data["sons_callback_counts"] = new[] {beforeCount, afterCount, invalid ? 1 : 0,
                            current == this ? 1 : 0, Thread.CurrentThread.ManagedThreadId == thread ? 1 : 0, closed ? 1 : 0};
                    throw failure;
                }
                return SaveReceipt.Completed;
            }
            internal bool IsCurrentOwnerThread()
            { return current == this && !closed && Thread.CurrentThread.ManagedThreadId == thread; }
            public void Dispose()
            {
                if (closed) return;
                closed = true;
                if (current == this) current = null;
                List<Exception> failures = new List<Exception>();
                if (AfterAttempted)
                {
                    try { Owner.unregisterAfter.Invoke(null, new[] {After}); }
                    catch (Exception error) { failures.Add(error); }
                }
                if (BeforeAttempted)
                {
                    try { Owner.unregisterBefore.Invoke(null, new[] {Before}); }
                    catch (Exception error) { failures.Add(error); }
                }
                if (failures.Count != 0)
                {
                    // Keep failed subscriptions alive and inert until native
                    // scene/process teardown; allow no additional subscriptions.
                    retained = this;
                    throw new AggregateException("Native save callback cleanup failed", failures);
                }
                Before = null; After = null;
            }
        }
    }

    // The dynamic receiver calls across assembly boundaries. Both this type and
    // its leaf methods must be public; neither method performs native I/O.
    public static class NativeSaveCallbackBridge
    {
        public static void Before() { NativeSaveCompletion.BeforeCurrent(); }
        public static void After(bool playerOnly) { NativeSaveCompletion.AfterCurrent(playerOnly); }
    }

    internal static class NativeCallbackReceiver
    {
        private const BindingFlags PublicInstance = BindingFlags.Public | BindingFlags.Instance;
        private const BindingFlags PublicStatic = BindingFlags.Public | BindingFlags.Static;
        private static readonly object gate = new object();
        private static object receiver, before, after;
        private static bool attempted;

        internal static void Get(Type beforeType, Type afterType, out object beforeCallback, out object afterCallback)
        {
            lock (gate)
            {
                if (receiver == null)
                {
                    // A failed injection cannot be repeated: the native class
                    // may already exist even when the managed call threw.
                    if (attempted) throw new SaveFailure("api_unavailable");
                    attempted = true;
                    Create(beforeType, afterType);
                }
                beforeCallback = before; afterCallback = after;
            }
        }
        private static MethodInfo Generic(Type owner, string name)
        {
            MethodInfo found = null;
            foreach (MethodInfo method in owner.GetMethods(PublicStatic))
                if (method.Name == name && method.IsGenericMethodDefinition &&
                    method.GetGenericArguments().Length == 1 && method.GetParameters().Length == 0)
                { if (found != null) throw new SaveFailure("api_unavailable"); found = method; }
            if (found == null) throw new SaveFailure("api_unavailable");
            return found;
        }
        private static void Create(Type beforeType, Type afterType)
        {
            Type nativeObject = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Object");
            Type injector = NativeBinding.Type("Il2CppInterop.Runtime", "Il2CppInterop.Runtime.Injection.ClassInjector");
            MethodInfo body = null;
            foreach (MethodInfo method in injector.GetMethods(PublicStatic))
                if (method.Name == "DerivedConstructorBody" && !method.IsGenericMethod && method.ReturnType == typeof(void) &&
                    method.GetParameters().Length == 1 && method.GetParameters()[0].ParameterType.IsAssignableFrom(nativeObject))
                { if (body != null) throw new SaveFailure("api_unavailable"); body = method; }
            ConstructorInfo baseConstructor = nativeObject.GetConstructor(new[] {typeof(IntPtr)});
            if (body == null || baseConstructor == null) throw new SaveFailure("api_unavailable");
            AssemblyBuilder assembly = AssemblyBuilder.DefineDynamicAssembly(
                new AssemblyName("LanGame.Sons.NativeSaveReceiver"), AssemblyBuilderAccess.Run);
            TypeBuilder builder = assembly.DefineDynamicModule("Receiver").DefineType(
                "LanGame.Sons.NativeSaveReceiver", TypeAttributes.Public | TypeAttributes.Sealed, nativeObject);
            ILGenerator fromPointer = builder.DefineConstructor(MethodAttributes.Public, CallingConventions.Standard,
                new[] {typeof(IntPtr)}).GetILGenerator();
            fromPointer.Emit(OpCodes.Ldarg_0); fromPointer.Emit(OpCodes.Ldarg_1);
            fromPointer.Emit(OpCodes.Call, baseConstructor); fromPointer.Emit(OpCodes.Ret);
            ILGenerator create = builder.DefineConstructor(MethodAttributes.Public, CallingConventions.Standard, Type.EmptyTypes).GetILGenerator();
            create.Emit(OpCodes.Ldarg_0);
            create.Emit(OpCodes.Call, Generic(injector, "DerivedConstructorPointer").MakeGenericMethod(builder));
            create.Emit(OpCodes.Call, baseConstructor); create.Emit(OpCodes.Ldarg_0);
            create.Emit(OpCodes.Call, body); create.Emit(OpCodes.Ret);
            EmitCallback(builder, "Before", Type.EmptyTypes);
            EmitCallback(builder, "After", new[] {typeof(bool)});
            Type created = builder.CreateType();
            NativeBinding.Method(injector, "RegisterTypeInIl2Cpp", PublicStatic, typeof(void), typeof(Type))
                .Invoke(null, new object[] {created});
            object instance = Activator.CreateInstance(created);
            object first = Bind(instance, nativeObject, beforeType, "Before", false);
            object second = Bind(instance, nativeObject, afterType, "After", true);
            // Keep precisely one receiver and two delegates strongly rooted for
            // the process. The native registry holds only weak target references.
            before = first; after = second; receiver = instance;
        }
        private static void EmitCallback(TypeBuilder builder, string name, Type[] parameters)
        {
            ILGenerator method = builder.DefineMethod(name, MethodAttributes.Public | MethodAttributes.HideBySig,
                typeof(void), parameters).GetILGenerator();
            if (parameters.Length != 0) method.Emit(OpCodes.Ldarg_1);
            method.Emit(OpCodes.Call, typeof(NativeSaveCallbackBridge).GetMethod(name, PublicStatic));
            method.Emit(OpCodes.Ret);
        }
        private static object Property(Type owner, string name, object instance)
        {
            PropertyInfo property = owner.GetProperty(name, PublicInstance);
            if (property == null) throw new SaveFailure("api_unavailable");
            return property.GetValue(instance, null);
        }
        private static IntPtr Pointer(object value)
        {
            if (value == null) throw new SaveFailure("api_unavailable");
            object pointer = Property(value.GetType(), "Pointer", value);
            if (!(pointer is IntPtr) || (IntPtr)pointer == IntPtr.Zero) throw new SaveFailure("api_unavailable");
            return (IntPtr)pointer;
        }
        private static object Bind(object instance, Type nativeObject, Type target, string name, bool argument)
        {
            Type nativeType = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Type");
            Type delegateType = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Delegate");
            Type methodInfo = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Reflection.MethodInfo");
            object instanceType = NativeBinding.Method(nativeObject, "GetIl2CppType", PublicInstance, nativeType)
                .Invoke(instance, null);
            object method = NativeBinding.Method(nativeType, "GetMethod", PublicInstance, methodInfo, typeof(string))
                .Invoke(instanceType, new object[] {name});
            Verify(method, instanceType, name, argument);
            Type interopType = NativeBinding.Type("Il2CppInterop.Runtime", "Il2CppInterop.Runtime.Il2CppType");
            object type = Generic(interopType, "Of").MakeGenericMethod(target).Invoke(null, null);
            object callback = NativeBinding.Method(delegateType, "CreateDelegate", PublicStatic, delegateType,
                nativeType, nativeObject, methodInfo).Invoke(null, new[] {type, instance, method});
            if (Pointer(Property(delegateType, "Target", callback)) != Pointer(instance)) throw new SaveFailure("api_unavailable");
            Verify(Property(delegateType, "Method", callback), instanceType, name, argument);
            MethodInfo cast = null;
            foreach (MethodInfo candidate in delegateType.GetMethods(PublicInstance))
                if (candidate.Name == "Cast" && candidate.IsGenericMethodDefinition &&
                    candidate.GetGenericArguments().Length == 1 && candidate.GetParameters().Length == 0)
                { if (cast != null) throw new SaveFailure("api_unavailable"); cast = candidate; }
            if (cast == null) throw new SaveFailure("api_unavailable");
            object converted = cast.MakeGenericMethod(target).Invoke(callback, null);
            if (!target.IsInstanceOfType(converted)) throw new SaveFailure("api_unavailable");
            return converted;
        }
        private static void Verify(object method, object owner, string name, bool argument)
        {
            if (method == null) throw new SaveFailure("api_unavailable");
            Type member = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Reflection.MemberInfo");
            Type basis = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Reflection.MethodBase");
            Type info = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Reflection.MethodInfo");
            Type nativeType = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Type");
            if ((string)Property(member, "Name", method) != name ||
                Pointer(Property(member, "DeclaringType", method)) != Pointer(owner) ||
                (bool)Property(basis, "IsStatic", method) ||
                (string)Property(nativeType, "FullName", Property(info, "ReturnType", method)) != "System.Void")
                throw new SaveFailure("api_unavailable");
            MethodInfo getParameters = basis.GetMethod("GetParameters", PublicInstance, null, Type.EmptyTypes, null);
            IEnumerable parameters = getParameters == null ? null : getParameters.Invoke(method, null) as IEnumerable;
            if (parameters == null) throw new SaveFailure("api_unavailable");
            int count = 0;
            Type parameter = NativeBinding.Type("Il2Cppmscorlib", "Il2CppSystem.Reflection.ParameterInfo");
            foreach (object item in parameters)
            {
                if (!argument || count != 0 ||
                    (string)Property(nativeType, "FullName", Property(parameter, "ParameterType", item)) != "System.Boolean")
                    throw new SaveFailure("api_unavailable");
                count++;
            }
            if (count != (argument ? 1 : 0)) throw new SaveFailure("api_unavailable");
        }
    }
}

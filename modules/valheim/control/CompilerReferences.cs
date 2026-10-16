// Build-only declarations for the six external members emitted by this
// plugin. They are neither implementations nor runtime dependencies.
// The compiler helper writes their assemblies only into work/reference/;
// deploy only LanGame.Valheim.Control.dll, never these reference assemblies.
using System;
using System.Reflection;

#if REFERENCE_UNITY_CORE
[assembly: AssemblyVersion("0.0.0.0")]
namespace UnityEngine
{
    public class MonoBehaviour
    {
        public MonoBehaviour() { throw new NotSupportedException("Compiler reference only"); }
    }
    public class Time
    {
        public static float realtimeSinceStartup
        {
            get { throw new NotSupportedException("Compiler reference only"); }
        }
    }
}
#elif REFERENCE_BEPINEX
[assembly: AssemblyVersion("5.4.23.5")]
namespace BepInEx
{
    public class BepInPlugin : Attribute
    {
        public BepInPlugin(string id, string name, string version)
        {
            throw new NotSupportedException("Compiler reference only");
        }
    }
    public abstract class BaseUnityPlugin : UnityEngine.MonoBehaviour
    {
        protected BaseUnityPlugin() { throw new NotSupportedException("Compiler reference only"); }
        protected Logging.ManualLogSource Logger
        {
            get { throw new NotSupportedException("Compiler reference only"); }
        }
    }
}
namespace BepInEx.Logging
{
    public class ManualLogSource
    {
        public void LogInfo(object value) { throw new NotSupportedException("Compiler reference only"); }
        public void LogError(object value) { throw new NotSupportedException("Compiler reference only"); }
    }
}
#else
#error Select exactly one build-only reference contract.
#endif

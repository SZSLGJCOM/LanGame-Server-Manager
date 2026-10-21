using System;
using System.Diagnostics;
using BepInEx;
using UnityEngine;

namespace Lgsm.Valheim.Control
{
    [BepInPlugin("local.langame.valheim.control", "LanGame Valheim Control", "0.1.0")]
    public sealed class LgsmValheimControl : BaseUnityPlugin
    {
        private ControlFiles files;
        private ControlLoop loop;
        private float nextPoll;
        private readonly ControlPublication publication = new ControlPublication();
        private byte[] lastRequest;
        private string lastRequestVersion;

        private void Awake()
        {
            string root = Environment.GetEnvironmentVariable("LGSM_VALHEIM_CONTROL_ROOT");
            string environmentToken = Environment.GetEnvironmentVariable("LGSM_VALHEIM_CONTROL_TOKEN");
            if (String.IsNullOrEmpty(root) && String.IsNullOrEmpty(environmentToken)) return;
            try
            {
                files = new ControlFiles(root);
                string token = files.ReadSessionToken();
                if (!String.IsNullOrEmpty(environmentToken) && !ControlProtocol.SameToken(environmentToken, token))
                    throw new ControlFailure("invalid_bootstrap");
                loop = new ControlLoop(new ValheimNativeControl(), token, Process.GetCurrentProcess().Id,
                    delegate(string diagnostic) { Logger.LogError(diagnostic); });
                Logger.LogInfo("LGSM_VALHEIM_CONTROL_READY protocol=1 pid=" + Process.GetCurrentProcess().Id);
            }
            catch (Exception)
            {
                if (files != null) files.Dispose();
                files = null;
                Logger.LogError("LanGame Valheim control inactive: invalid bootstrap or unsafe control directory.");
            }
        }

        private void Update()
        {
            if (files == null || Time.realtimeSinceStartup < nextPoll) return;
            nextPoll = Time.realtimeSinceStartup + 0.2f;
            try
            {
                // Publication failures cannot suspend a pending operation's deadline.
                publication.Offer(loop.Tick(Time.realtimeSinceStartup), Time.realtimeSinceStartup);
                if (publication.Response == null)
                {
                    byte[] request = files.ReadRequest();
                    if (request != null && (!SameBytes(request, lastRequest) ||
                        files.LastRequestVersion != lastRequestVersion || loop.HasPending))
                    {
                        publication.Offer(loop.Submit(request, Time.realtimeSinceStartup), Time.realtimeSinceStartup);
                        lastRequest = request;
                        lastRequestVersion = files.LastRequestVersion;
                    }
                }
                if (publication.Response != null)
                {
                    files.Publish(publication.Response);
                    publication.Clear();
                }
            }
            catch (ControlFailure)
            {
                // Invalid uncorrelated requests have no response. A rejected
                // response path, however, must stop publication immediately.
                if (publication.Response != null) Deactivate("unsafe_response");
            }
            catch (System.IO.IOException)
            {
                if (publication.Expired(Time.realtimeSinceStartup)) Deactivate("publication_timeout");
            }
            catch (UnauthorizedAccessException) { Deactivate("access_denied"); }
            catch (System.Security.SecurityException) { Deactivate("access_denied"); }
        }

        private void Deactivate(string code)
        {
            if (loop != null) loop.Dispose();
            if (files != null) files.Dispose();
            files = null;
            Logger.LogError("LanGame Valheim control inactive: " + code);
        }

        private static bool SameBytes(byte[] left, byte[] right)
        {
            if (left == null || right == null || left.Length != right.Length) return false;
            for (int i = 0; i < left.Length; i++) if (left[i] != right[i]) return false;
            return true;
        }

        private void OnDestroy()
        {
            if (loop != null) loop.Dispose();
            if (files != null) files.Dispose();
            files = null;
        }
    }
}

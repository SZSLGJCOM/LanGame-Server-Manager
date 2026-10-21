using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

// A request expires before entering the native save, never during its writes.
// Timestamped nonces permit a bounded replay set without accepting old requests
// again when their entries have been removed. All admission runs on Unity's
// single main thread, alongside the synchronous save and its terminal result.
internal sealed class ForestSaveRequests
{
    private const long MaximumAgeMs = 10000, FutureToleranceMs = 2000;
    private readonly Dictionary<string, long> seen = new Dictionary<string, long>();

    [DllImport("kernel32.dll")] private static extern ulong GetTickCount64();

    internal static long NowMilliseconds()
    {
        // Shared with the desktop process; UTC correction cannot revive an
        // expired request or stall admission after the wall clock moves back.
        return checked((long)GetTickCount64());
    }

    internal static string Nonce(string command)
    {
        if (command == null || command.Length != 37 || !command.StartsWith("save ", StringComparison.Ordinal)) return null;
        for (int at = 5; at < command.Length; at++)
            if (!((command[at] >= '0' && command[at] <= '9') || (command[at] >= 'a' && command[at] <= 'f'))) return null;
        return command.Substring(5);
    }

    internal string Admit(string nonce, long now)
    {
        long issued = 0;
        for (int at = 0; at < 16; at++)
        {
            int digit = nonce[at] <= '9' ? nonce[at] - '0' : nonce[at] - 'a' + 10;
            if (issued > (Int64.MaxValue - digit) / 16) return "expired";
            issued = issued * 16 + digit;
        }
        if (issued > now + FutureToleranceMs || issued < now - MaximumAgeMs) return "expired";
        if (seen.ContainsKey(nonce)) return "replayed";
        List<string> expired = new List<string>();
        foreach (KeyValuePair<string, long> item in seen)
            if (item.Value < now - MaximumAgeMs) expired.Add(item.Key);
        foreach (string key in expired) seen.Remove(key);
        if (seen.Count >= 1024) return "request_capacity";
        seen.Add(nonce, issued);
        return null;
    }

    internal static string Completed(string nonce, long bytes)
    {
        if (bytes <= 0) throw new InvalidOperationException();
        return "native_save_completed nonce=" + nonce + " bytes=" + bytes;
    }

    internal static string Failed(string nonce, string reason)
    {
        return "native_save_failed nonce=" + nonce + " code=" + reason;
    }
}

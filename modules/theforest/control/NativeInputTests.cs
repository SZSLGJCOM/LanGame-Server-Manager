using System;
using System.Runtime.InteropServices;
using System.Threading;

internal static class ForestNativeInputTests
{
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool CreatePipe(out IntPtr read, out IntPtr write, IntPtr security, uint size);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool WriteFile(IntPtr handle, byte[] buffer, uint count, out uint written, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool CloseHandle(IntPtr handle);
    internal static void Verify()
    {
        foreach (bool partial in new bool[] { false, true })
        {
            IntPtr read, write;
            if (!CreatePipe(out read, out write, IntPtr.Zero, 0)) throw new Exception("Cannot create native input fixture");
            using (ManualResetEvent cancellation = new ManualResetEvent(false))
            using (ManualResetEvent started = new ManualResetEvent(false))
            using (ForestOwnedInputStream input = new ForestOwnedInputStream(read, cancellation))
            {
                string result = "unread";
                Exception failure = null;
                Thread thread = new Thread(delegate() { started.Set(); try { result = ForestControlProtocol.Read(input); } catch (Exception error) { failure = error; } });
                try
                {
                    if (partial) Write(write, new byte[] { 115, 97 });
                    thread.Start();
                    if (!started.WaitOne(1000)) throw new Exception("Native reader fixture did not start");
                    cancellation.Set();
                    bool cancelledWithoutInput = thread.Join(1000);
                    // Cooperative release of the deliberately blocking red path;
                    // no leaked reader and no thread abort/forced termination.
                    if (!cancelledWithoutInput) { Write(write, new byte[] { 10 }); if (!thread.Join(1000)) throw new Exception("Native reader fixture did not drain"); }
                    if (!cancelledWithoutInput || failure != null || result != null)
                    {
                        Console.Error.WriteLine("Owned input cancellation failed without an extra byte; partial=" + partial);
                        throw new Exception("Owned input cancellation regression");
                    }
                }
                finally { CloseHandle(write); if (thread.IsAlive) thread.Join(1000); }
            }
        }
        IntPtr normalRead, normalWrite;
        if (!CreatePipe(out normalRead, out normalWrite, IntPtr.Zero, 0)) throw new Exception("Cannot create native input fixture");
        using (ManualResetEvent cancellation = new ManualResetEvent(false))
        using (ForestOwnedInputStream input = new ForestOwnedInputStream(normalRead, cancellation))
        {
            Write(normalWrite, new byte[] { 104, 101, 108, 112, 13, 10 });
            CloseHandle(normalWrite);
            if (ForestControlProtocol.Read(input) != "help" || ForestControlProtocol.Read(input) != null) throw new Exception("Native input framing or EOF failed");
        }
        Console.WriteLine("The Forest owned input: idle/partial-frame cancellation and native framing/EOF passed");
    }
    private static void Write(IntPtr pipe, byte[] bytes)
    {
        uint written;
        if (!WriteFile(pipe, bytes, (uint)bytes.Length, out written, IntPtr.Zero) || written != bytes.Length) throw new Exception("Native input fixture write failed");
    }
}
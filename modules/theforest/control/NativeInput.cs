using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;

// The bridge is the sole reader of its duplicated, owned anonymous stdin pipe.
internal sealed class ForestOwnedInputStream : Stream
{
    private sealed class PipeHandle : SafeHandle
    {
        internal PipeHandle(IntPtr value) : base(IntPtr.Zero, true) { SetHandle(value); }
        public override bool IsInvalid { get { return handle == IntPtr.Zero || handle == new IntPtr(-1); } }
        protected override bool ReleaseHandle() { return CloseHandle(handle); }
    }
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool ReadFile(PipeHandle handle, byte[] buffer, uint count, out uint read, IntPtr overlapped);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool PeekNamedPipe(PipeHandle handle, IntPtr buffer, uint size, IntPtr read, out uint available, IntPtr left);
    [DllImport("kernel32.dll", SetLastError=true)] private static extern bool CloseHandle(IntPtr handle);
    private readonly PipeHandle pipe;
    private readonly WaitHandle cancellation;
    private readonly byte[] chunk = new byte[256];
    internal ForestOwnedInputStream(IntPtr owned, WaitHandle cancellation) { pipe = new PipeHandle(owned); this.cancellation = cancellation; }
    public override bool CanRead { get { return !cancellation.WaitOne(0); } }
    public override bool CanSeek { get { return false; } }
    public override bool CanWrite { get { return false; } }
    public override long Length { get { throw new NotSupportedException(); } }
    public override long Position { get { throw new NotSupportedException(); } set { throw new NotSupportedException(); } }
    public override void Flush() { }
    public override long Seek(long offset, SeekOrigin origin) { throw new NotSupportedException(); }
    public override void SetLength(long length) { throw new NotSupportedException(); }
    public override void Write(byte[] buffer, int offset, int count) { throw new NotSupportedException(); }
    public override int Read(byte[] buffer, int offset, int count)
    {
        if (buffer == null) throw new ArgumentNullException("buffer");
        if (offset < 0 || count < 0 || offset > buffer.Length - count) throw new ArgumentOutOfRangeException();
        if (count == 0) return 0;
        while (!cancellation.WaitOne(0))
        {
            uint available;
            if (!PeekNamedPipe(pipe, IntPtr.Zero, 0, IntPtr.Zero, out available, IntPtr.Zero))
            {
                int error = Marshal.GetLastWin32Error();
                if (error == 109 || error == 232) return 0;
                throw new IOException("Owned input pipe inspection failed");
            }
            if (available == 0) { cancellation.WaitOne(20); continue; }
            // Only this reader consumes the owned pipe; never issue a blocking
            // read for bytes that are not already present in its buffer.
            uint requested = Math.Min((uint)Math.Min(count, chunk.Length), available);
            uint read;
            if (!ReadFile(pipe, chunk, requested, out read, IntPtr.Zero))
            {
                int error = Marshal.GetLastWin32Error();
                if (error == 109 || error == 232) return 0;
                throw new IOException("Owned input pipe read failed");
            }
            Buffer.BlockCopy(chunk, 0, buffer, offset, (int)read);
            return (int)read;
        }
        return 0;
    }
    protected override void Dispose(bool disposing) { if (disposing) pipe.Dispose(); base.Dispose(disposing); }
}

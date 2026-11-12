using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

// PowerShell 5's filesystem provider and legacy .NET path normalization can
// reject files Rust has already created. Keep installer IO on extended Win32
// paths without changing machine policy or relaxing the plain-tree boundary.
public static class LanGameInstallFiles {
  [StructLayout(LayoutKind.Sequential)] private struct Information {
    public uint Attributes;
    public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
    public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
  }
  [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] private struct FindData {
    public uint Attributes;
    public System.Runtime.InteropServices.ComTypes.FILETIME Creation, Access, Write;
    public uint SizeHigh, SizeLow, Reserved0, Reserved1;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=260)] public string Name;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst=14)] public string Alternate;
  }
  private sealed class FindHandle : SafeHandleZeroOrMinusOneIsInvalid {
    public FindHandle() : base(true) {}
    protected override bool ReleaseHandle() { return FindClose(handle); }
  }
  public sealed class Entry {
    public string FullName, Name;
    public uint Attributes;
    public bool IsDirectory { get { return (Attributes & 16) != 0; } }
  }
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern uint GetFullPathNameW(string path, uint size, StringBuilder buffer, IntPtr part);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern uint GetFileAttributesW(string path);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool SetFileAttributesW(string path, uint attributes);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern FindHandle FindFirstFileW(string path, out FindData data);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool FindNextFileW(FindHandle handle, out FindData data);
  [DllImport("kernel32.dll", SetLastError=true)] private static extern bool FindClose(IntPtr handle);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern SafeFileHandle CreateFileW(string path, uint access, uint sharing, IntPtr security, uint disposition, uint flags, IntPtr template);
  [DllImport("kernel32.dll", SetLastError=true)]
  private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out Information information);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool CreateDirectoryW(string path, IntPtr security);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool MoveFileW(string source, string destination);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool CopyFileW(string source, string destination, bool failIfExists);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool DeleteFileW(string path);
  [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
  private static extern bool RemoveDirectoryW(string path);

  private static Win32Exception Error(string path) {
    return new Win32Exception(Marshal.GetLastWin32Error(), "Installation file operation failed: " + path);
  }
  private static int RootLength(string path) {
    if (path.Length >= 3 && Char.IsLetter(path[0]) && path[1] == ':' && path[2] == '\\') return 3;
    if (path.StartsWith(@"\\", StringComparison.Ordinal) && !path.StartsWith(@"\\.\", StringComparison.Ordinal)) {
      int server = path.IndexOf('\\', 2);
      if (server > 2 && server + 1 < path.Length) {
        int share = path.IndexOf('\\', server + 1);
        return share < 0 ? path.Length : share + 1;
      }
    }
    throw new IOException("An absolute drive or UNC installation path is required.");
  }
  public static string FullPath(string path) {
    if (String.IsNullOrEmpty(path) || path.IndexOf('\0') >= 0) throw new IOException("Invalid installation path.");
    path = path.Replace('/', '\\');
    if (path.StartsWith(@"\\?\UNC\", StringComparison.OrdinalIgnoreCase)) path = @"\\" + path.Substring(8);
    else if (path.StartsWith(@"\\?\", StringComparison.Ordinal)) path = path.Substring(4);
    RootLength(path);
    StringBuilder result = new StringBuilder(32768);
    uint count = GetFullPathNameW(path, (uint)result.Capacity, result, IntPtr.Zero);
    if (count == 0) throw Error(path);
    if (count >= result.Capacity) throw new PathTooLongException("Installation path exceeds the Win32 limit.");
    string full = result.ToString();
    int root = RootLength(full);
    return full.Length > root ? full.TrimEnd('\\') : full;
  }
  private static string NativePath(string path) {
    path = FullPath(path);
    return path.StartsWith(@"\\", StringComparison.Ordinal) ? @"\\?\UNC\" + path.Substring(2) : @"\\?\" + path;
  }
  public static string Combine(string parent, string relative) {
    if (relative.IndexOf(':') >= 0 || relative.StartsWith("\\") || relative.StartsWith("/"))
      throw new IOException("Installation child path must be relative.");
    return FullPath(FullPath(parent).TrimEnd('\\') + "\\" + relative);
  }
  public static string Parent(string path) {
    path = FullPath(path);
    int root = RootLength(path);
    if (path.Length <= root) return null;
    int separator = path.LastIndexOf('\\');
    return path.Substring(0, Math.Max(root, separator));
  }
  public static int Attributes(string path) {
    uint attributes = GetFileAttributesW(NativePath(path));
    if (attributes != UInt32.MaxValue) return (int)attributes;
    int code = Marshal.GetLastWin32Error();
    if (code == 2 || code == 3) return -1;
    throw new Win32Exception(code, "Cannot inspect installation path: " + path);
  }
  public static Entry ReadItem(string path) {
    path = FullPath(path);
    int attributes = Attributes(path);
    if (attributes < 0) throw new FileNotFoundException("Installation path is missing.", path);
    return new Entry { FullName=path, Name=path.Substring(path.LastIndexOf('\\')+1), Attributes=(uint)attributes };
  }
  public static void AssertPlainItem(Entry item) {
    if ((item.Attributes & 0x400) != 0) throw new IOException("Installation data contains a link or reparse point: " + item.FullName);
    // Hard links have no reparse attribute. Preserve the previous PowerShell
    // LinkType boundary before retained data can overwrite a staged file.
    if (!item.IsDirectory) {
      using (SafeFileHandle handle=CreateFileW(NativePath(item.FullName),0,7,IntPtr.Zero,3,0x00200000,IntPtr.Zero)) {
        if (handle.IsInvalid) throw Error(item.FullName);
        Information information;
        if (!GetFileInformationByHandle(handle,out information)) throw Error(item.FullName);
        AssertPlainFile(information,item.FullName);
      }
    }
  }
  private static void AssertPlainFile(Information information, string path) {
    if ((information.Attributes & 0x400) != 0 || information.Links > 1)
      throw new IOException("Installation data contains a link or reparse point: " + path);
    if ((information.Attributes & 16) != 0) throw new IOException("Installation file is not a plain file: " + path);
  }
  public static void AssertPlainPathChain(string path) {
    bool leaf = true;
    for (string cursor=FullPath(path); cursor != null; cursor=Parent(cursor)) {
      Entry item = ReadItem(cursor);
      AssertPlainItem(item);
      if (!leaf && !item.IsDirectory) throw new IOException("Installation ancestor is not a directory: " + cursor);
      leaf = false;
    }
  }
  public static Entry[] Entries(string path) {
    Entry parent = ReadItem(path);
    AssertPlainItem(parent);
    if (!parent.IsDirectory) throw new IOException("Installation entry is not a directory: " + path);
    List<Entry> entries = new List<Entry>();
    FindData data;
    using (FindHandle handle = FindFirstFileW(NativePath(path).TrimEnd('\\') + @"\*", out data)) {
      if (handle.IsInvalid) {
        int code = Marshal.GetLastWin32Error();
        if (code == 2) return entries.ToArray();
        throw new Win32Exception(code, "Cannot enumerate installation directory: " + path);
      }
      do {
        if (data.Name != "." && data.Name != "..")
          entries.Add(new Entry { FullName=Combine(path,data.Name), Name=data.Name, Attributes=data.Attributes });
      } while (FindNextFileW(handle, out data));
      int error = Marshal.GetLastWin32Error();
      if (error != 18) throw new Win32Exception(error, "Cannot finish installation enumeration: " + path);
    }
    return entries.ToArray();
  }
  public static void AssertPlainTree(string path) {
    AssertPlainPathChain(path);
    Stack<string> pending = new Stack<string>();
    pending.Push(path);
    while (pending.Count != 0) {
      Entry item = ReadItem(pending.Pop());
      AssertPlainItem(item);
      if (item.IsDirectory) foreach (Entry child in Entries(item.FullName)) pending.Push(child.FullName);
    }
  }
  public static void CreateDirectory(string path) {
    path = FullPath(path);
    if (Attributes(path) >= 0) {
      AssertPlainPathChain(path);
      if (!ReadItem(path).IsDirectory) throw new IOException("Installation directory is occupied by a file: " + path);
      return;
    }
    string parent = Parent(path);
    if (parent == null) throw new DirectoryNotFoundException(path);
    CreateDirectory(parent);
    if (!CreateDirectoryW(NativePath(path), IntPtr.Zero)) throw Error(path);
  }
  private static FileStream Open(string path, bool write, uint disposition) {
    SafeFileHandle handle = CreateFileW(NativePath(path), write ? 0x40000000u : 0x80000000u,
      write ? 0u : 1u, IntPtr.Zero, disposition, 0x00200080, IntPtr.Zero);
    if (handle.IsInvalid) { var error=Error(path); handle.Dispose(); throw error; }
    try {
      Information information;
      if (!GetFileInformationByHandle(handle,out information)) throw Error(path);
      AssertPlainFile(information,path);
      return new FileStream(handle, write ? FileAccess.Write : FileAccess.Read);
    } catch { handle.Dispose(); throw; }
  }
  public static FileStream OpenRead(string path) { AssertPlainPathChain(path); return Open(path,false,3); }
  public static FileStream CreateNew(string path) { AssertPlainPathChain(Parent(path)); return Open(path,true,1); }
  public static void WriteAllText(string path, string text) {
    AssertPlainPathChain(Parent(path));
    if (Attributes(path) >= 0) AssertPlainItem(ReadItem(path));
    using (FileStream file=Open(path,true,2)) {
      byte[] bytes=new UTF8Encoding(false).GetBytes(text);
      file.Write(bytes,0,bytes.Length); file.Flush(true);
    }
  }
  public static void CopyFile(string source, string destination, bool overwrite) {
    AssertPlainPathChain(source); AssertPlainPathChain(Parent(destination));
    if (Attributes(destination) >= 0) AssertPlainItem(ReadItem(destination));
    if (!CopyFileW(NativePath(source),NativePath(destination),!overwrite)) throw Error(destination);
  }
  public static string DirectoryIdentity(string path) {
    using (SafeFileHandle handle=CreateFileW(NativePath(path),0,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero)) {
      if (handle.IsInvalid) throw Error(path);
      Information info;
      if (!GetFileInformationByHandle(handle,out info)) throw Error(path);
      if ((info.Attributes & 0x410) != 0x10) throw new IOException("Publication source must be a plain directory.");
      return info.Volume.ToString("X8")+info.IndexHigh.ToString("X8")+info.IndexLow.ToString("X8");
    }
  }
  public static void MoveDirectory(string source, string destination) {
    if (!MoveFileW(NativePath(source),NativePath(destination))) throw Error(destination);
  }
  public static void DeleteFile(string path) {
    int attributes=Attributes(path);
    if (attributes < 0) return;
    AssertPlainItem(ReadItem(path));
    if ((attributes & 1) != 0 && !SetFileAttributesW(NativePath(path),(uint)(attributes & ~1))) throw Error(path);
    if (!DeleteFileW(NativePath(path))) throw Error(path);
  }
  public static void RemoveTree(string path) {
    if (Attributes(path) < 0) return;
    Entry item=ReadItem(path); AssertPlainItem(item);
    if (!item.IsDirectory) { DeleteFile(path); return; }
    foreach (Entry child in Entries(path)) RemoveTree(child.FullName);
    if (!RemoveDirectoryW(NativePath(path))) throw Error(path);
  }
}

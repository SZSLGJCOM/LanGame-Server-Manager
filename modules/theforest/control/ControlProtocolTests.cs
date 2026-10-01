using System;
using System.IO;
using System.Text;

internal static class ForestControlProtocolTests
{
    private static void Equal(string expected, string actual)
    {
        if (expected != actual) throw new Exception("Native control frame mismatch");
    }

    public static void Main(string[] args)
    {
        ForestBclContractTests.Verify(Path.Combine(Path.GetFullPath(args[0]), "LanGame.TheForest.Control.dll"));
        ForestNativeInputTests.Verify();
        ForestWorldReadinessTests.Verify();
        using (MemoryStream input = new MemoryStream(Encoding.ASCII.GetBytes(" HELP \r\nstatus\nsave\nshutdown\n")))
        {
            foreach (string command in new string[] { "help", "status", "save", "shutdown" })
                Equal(command, ForestControlProtocol.Read(input));
            Equal(null, ForestControlProtocol.Read(input));
        }
        foreach (string rejected in new string[] { new string('x', 129), "sa\rve", "sa\0ve", "sa\tve", "save\r\r" })
        {
            using (MemoryStream input = new MemoryStream(Encoding.ASCII.GetBytes(rejected + "\nsave\n")))
            {
                Equal(ForestControlProtocol.Invalid, ForestControlProtocol.Read(input));
                Equal("save", ForestControlProtocol.Read(input));
            }
        }
        using (MemoryStream input = new MemoryStream(Encoding.UTF8.GetBytes("保存\nstatus\n")))
        {
            Equal(ForestControlProtocol.Invalid, ForestControlProtocol.Read(input));
            Equal("status", ForestControlProtocol.Read(input));
        }
        using (MemoryStream input = new MemoryStream(Encoding.ASCII.GetBytes("shutdown")))
            Equal(null, ForestControlProtocol.Read(input));
        using (MemoryStream input = new MemoryStream(Encoding.ASCII.GetBytes(new string('x', 128) + "\n")))
            Equal(new string('x', 128), ForestControlProtocol.Read(input));
        Console.WriteLine("The Forest control protocol: 10 framing cases passed");
        string root = Path.Combine(Path.GetFullPath(args[0]), "checkpoint-test-" + Guid.NewGuid().ToString("N"));
        Directory.CreateDirectory(root);
        try
        {
            string path = Path.Combine(root, "__RESUME__");
            File.WriteAllText(path, "recent old checkpoint");
            bool refused = false;
            try { ForestNativeCheckpoint.Save(path, delegate() { }); }
            catch (IOException) { refused = true; }
            if (!refused) throw new Exception("An unchanged recent checkpoint was accepted");
            long count = ForestNativeCheckpoint.Save(path, delegate()
            {
                GC.Collect();
                GC.WaitForPendingFinalizers();
                File.Move(path, path + "prev");
                File.WriteAllText(path, "new native checkpoint");
                GC.Collect();
                GC.WaitForPendingFinalizers();
            });
            if (count == 0 || File.ReadAllText(path + "prev") != "recent old checkpoint") throw new Exception("Native replacement was not observed");
            count = ForestNativeCheckpoint.Save(path, delegate()
            {
                GC.Collect();
                GC.WaitForPendingFinalizers();
                File.Delete(path + "prev");
                File.Move(path, path + "prev");
                File.WriteAllText(path, "second native checkpoint");
            });
            if (count == 0 || File.ReadAllText(path + "prev") != "new native checkpoint") throw new Exception("Repeated native replacement was not observed");
            File.Delete(path);
            count = ForestNativeCheckpoint.Save(path, delegate() { File.WriteAllText(path, "first checkpoint"); });
            if (count == 0) throw new Exception("First checkpoint was not observed");
            Console.WriteLine("The Forest native checkpoint: unchanged-file rejection, repeated replacement across GC and first-save cases passed");
        }
        finally { Directory.Delete(root, true); }
    }
}

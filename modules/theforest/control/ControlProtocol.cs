using System;
using System.IO;

internal static class ForestControlProtocol
{
    internal const string Invalid = "!invalid";
    internal const int MaximumLength = 128;

    // One fixed-size frame; rejected input is drained through its newline so it
    // cannot turn the suffix of an overlong command into a second command.
    internal static string Read(Stream stream)
    {
        char[] line = new char[MaximumLength];
        int count = 0;
        bool invalid = false, carriageReturn = false;
        while (true)
        {
            int value = stream.ReadByte();
            if (value < 0) return null;
            if (value == 10)
            {
                if (invalid) return Invalid;
                string frame = new string(line, 0, count);
                // Preserve request spelling for its strict nonce grammar. The
                // original single-word commands retain case/space tolerance.
                string normalized = frame.Trim().ToLowerInvariant();
                return normalized.StartsWith("save ", StringComparison.Ordinal) ? frame : normalized;
            }
            if (carriageReturn) invalid = true;
            if (value == 13) { carriageReturn = true; continue; }
            if (value < 32 || value > 126 || count == line.Length) invalid = true;
            else if (!invalid) line[count++] = (char)value;
        }
    }
}

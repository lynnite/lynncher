using System.Reflection;
using HarmonyLib;

namespace ExamplePatch;

public static class Patch
{
    private static readonly Harmony Harmony = new("org.lynncher.example");

    public static void PatchEngine(Assembly clientAssembly)
    {
        Console.WriteLine($"[ExamplePatch] PatchEngine(client={(clientAssembly.GetName().Name)})");

        var type = FindType("Robust.Client.Program");
        var method = type is null ? null : AccessTools.Method(type, "Main");
        if (method is not null)
        {
        }
    }

    public static void PatchContent()
    {
        Console.WriteLine("[ExamplePatch] PatchContent()");
    }

    private static Type? FindType(string name)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            if (asm.IsDynamic)
                continue;
            var t = asm.GetType(name, throwOnError: false);
            if (t is not null)
                return t;
        }
        return null;
    }
}

using System.Reflection;
using HarmonyLib;

namespace Lynncher.Patches;

public static class PatchTools
{
    private static Harmony? _harmony;

    public static Harmony Harmony => _harmony ??= new Harmony("org.lynncher.clientpatches");

    public static Type? GetTypeByQualifiedName(string qualifiedName)
    {
        foreach (var asm in AppDomain.CurrentDomain.GetAssemblies())
        {
            if (asm.IsDynamic)
                continue;
            Type? type;
            try
            {
                type = asm.GetType(qualifiedName, throwOnError: false);
            }
            catch
            {
                continue;
            }
            if (type is not null)
                return type;
        }
        return null;
    }

    public static MethodInfo? FindMethod(Type type, string name)
    {
        const BindingFlags flags =
            BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static | BindingFlags.Instance;
        return type.GetMethod(name, flags)
               ?? type.GetMethods(flags).FirstOrDefault(m => m.Name == name);
    }
}

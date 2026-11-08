
using System;
using System.IO;
using System.Reflection;
using System.Runtime.Loader;

namespace SS14.Loader;

internal static class LoaderPatchHook
{
    private const string FrameworkAssemblyName = "Lynncher.Patches.dll";
    private const string EntryTypeName = "Lynncher.Patches.PatchEntry";

    private static readonly Lazy<Type?> EntryType = new(() =>
    {
        var dir = Path.GetDirectoryName(typeof(LoaderPatchHook).Assembly.Location)!;
        var path = Path.Combine(dir, FrameworkAssemblyName);
        if (!File.Exists(path))
            return null;

        AssemblyLoadContext.Default.Resolving += (ctx, name) =>
        {
            if (string.IsNullOrEmpty(name.Name))
                return null;
            var candidate = Path.Combine(dir, name.Name + ".dll");
            if (!File.Exists(candidate))
                return null;
            try
            {
                return ctx.LoadFromAssemblyPath(candidate);
            }
            catch
            {
                return null;
            }
        };

        var assembly = AssemblyLoadContext.Default.LoadFromAssemblyPath(path);
        return assembly.GetType(EntryTypeName, throwOnError: false);
    });

    public static IDisposable InstallEngineHooks(Assembly clientAssembly, Assembly sharedAssembly, string robustPath)
    {
        var type = EntryType.Value;
        if (type is null)
        {
            Console.WriteLine("[lynncher-patches] framework not found next to loader; skipping patches.");
            return new NoopHandle();
        }

        var method = type.GetMethod("InstallEngineHooks", BindingFlags.Public | BindingFlags.Static);
        if (method is null)
        {
            Console.WriteLine("[lynncher-patches] InstallEngineHooks not found; skipping patches.");
            return new NoopHandle();
        }

        return (IDisposable)method.Invoke(null, [clientAssembly, sharedAssembly, robustPath])!;
    }

    public static void ApplyContentHooks()
    {
        var type = EntryType.Value;
        type?.GetMethod("ApplyContentHooks", BindingFlags.Public | BindingFlags.Static)?.Invoke(null, null);
    }

    internal sealed class NoopHandle : IDisposable
    {
        public void Dispose()
        {
        }
    }
}

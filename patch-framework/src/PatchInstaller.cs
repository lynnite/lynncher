using System.Reflection;
using System.Runtime.CompilerServices;
using HarmonyLib;

namespace Lynncher.Patches;

public static class PatchInstaller
{
    private static readonly List<Assembly> _loaded = new();
    private static bool _engineApplied;
    private static bool _contentApplied;

    public static int LoadBundles()
    {
        if (!PatchConfig.Load())
        {
            Console.WriteLine("[lynncher-patches] patching not requested; nothing to do.");
            return 0;
        }

        AssemblyHidingManager.HideBasicAssemblies();
        AssemblyHidingManager.HideAssembly(typeof(PatchInstaller).Assembly);
        AssemblyHidingManager.PatchDetectionVectors();

        EnsureHarmonyLoaded();

        var count = 0;
        foreach (var bundle in PatchConfig.Bundles)
        {
            var asmPath = bundle.Assembly;
            if (string.IsNullOrEmpty(asmPath) || !File.Exists(asmPath))
            {
                Console.WriteLine($"[lynncher-patches] bundle '{bundle.Name}' has no loadable assembly; skipping.");
                continue;
            }

            try
            {
                var asm = Assembly.LoadFrom(asmPath);
                _loaded.Add(asm);

                AssemblyHidingManager.HideAssembly(asm);

                var bundleDir = Path.GetDirectoryName(asmPath);
                foreach (var dep in asm.GetReferencedAssemblies())
                {
                    if (dep.Name is not { } dn)
                        continue;
                    if (IsFrameworkAssembly(dn))
                        continue;
                    if (!PrivateDependencyLivesNextTo(dn, bundleDir))
                        continue;
                    AssemblyHidingManager.HideAssemblyByName(dn);
                }

                Console.WriteLine($"[lynncher-patches] loaded bundle '{bundle.Name}'");
                count++;
            }
            catch (Exception ex)
            {
                Console.WriteLine($"[lynncher-patches] failed to load bundle '{bundle.Name}': {ex.Message}");
            }
        }

        return count;
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    public static bool InstallDeferredTrigger(Assembly sharedAssembly, Assembly? clientAssembly)
    {
        try
        {
            var iocManager = sharedAssembly.GetType("Robust.Shared.IoC.IoCManager", throwOnError: false);
            var buildGraph = iocManager?.GetMethod(
                "BuildGraph",
                BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
            if (buildGraph is null)
            {
                Console.WriteLine("[lynncher-patches] could not locate IoCManager.BuildGraph; running patches immediately.");
                return false;
            }

            var postfix = new HarmonyMethod(typeof(PatchInstaller).GetMethod(
                nameof(OnBuildGraphPostfix),
                BindingFlags.NonPublic | BindingFlags.Static)!);
            var harmony = new Harmony("org.lynncher.deferred-trigger");
            harmony.Patch(buildGraph, postfix: postfix);
            _deferredClientAssembly = clientAssembly;
            return true;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] deferred trigger install failed; running immediately: {ex.Message}");
            return false;
        }
    }

    private static Assembly? _deferredClientAssembly;

    private static void OnBuildGraphPostfix()
    {
        try
        {
            if (_engineApplied && _contentApplied)
                return;

            if (_deferredClientAssembly is not null)
                ApplyEnginePatches(_deferredClientAssembly);
            ApplyContentPatches();
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] deferred patch run failed: {ex.Message}");
        }
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    public static void ApplyEnginePatches(Assembly clientAssembly)
    {
        if (_engineApplied)
            return;
        _engineApplied = true;

        foreach (var asm in _loaded.ToArray())
            InvokeEntryPoints(asm, "PatchEngine", clientAssembly);
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    public static void ApplyContentPatches()
    {
        if (_contentApplied || !PatchConfig.ContentLevel)
            return;
        _contentApplied = true;

        foreach (var asm in _loaded.ToArray())
            InvokeEntryPoints(asm, "PatchContent", null);
    }

    private static void InvokeEntryPoints(Assembly asm, string entryName, Assembly? clientAssembly)
    {
        Type[] types;
        try
        {
            types = asm.GetTypes();
        }
        catch (ReflectionTypeLoadException ex)
        {
            types = ex.Types.Where(t => t is not null).Cast<Type>().ToArray();
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] cannot enumerate types of {asm.GetName().Name}: {ex.Message}");
            return;
        }

        foreach (var type in types)
        {
            MethodInfo? method;
            try
            {
                method = type.GetMethod(entryName,
                    BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static);
            }
            catch
            {
                continue;
            }
            if (method is null)
                continue;

            try
            {
                var ps = method.GetParameters();
                object?[] args = ps.Length switch
                {
                    0 => [],
                    1 when ps[0].ParameterType == typeof(Assembly) => [clientAssembly],
                    _ => [],
                };
                method.Invoke(null, args.Length == ps.Length ? args : null);
                Console.WriteLine($"[lynncher-patches] ran {type.FullName}.{entryName}");
            }
            catch (Exception ex)
            {
                Console.WriteLine($"[lynncher-patches] {type.FullName}.{entryName} threw: {ex.InnerException?.Message ?? ex.Message}");
            }
        }
    }

    private static void EnsureHarmonyLoaded()
    {
        try
        {
            _ = PatchTools.Harmony;
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] Harmony preload warning: {ex.Message}");
        }
    }

    private static bool IsFrameworkAssembly(string name)
        => name.StartsWith("System", StringComparison.Ordinal)
           || name.StartsWith("Microsoft", StringComparison.Ordinal)
           || name == "netstandard"
           || name == "mscorlib"
           || name.StartsWith("Robust.", StringComparison.Ordinal)
           || name is "Robust" or "Robust.Client" or "Robust.Shared"
           || name.StartsWith("Content.", StringComparison.Ordinal)
           || name is "Content.Client" or "Content.Shared";

    private static bool PrivateDependencyLivesNextTo(string dependencyName, string? bundleDir)
    {
        if (string.IsNullOrEmpty(bundleDir) || !Directory.Exists(bundleDir))
            return false;

        var candidate = Path.Combine(bundleDir, dependencyName + ".dll");
        if (!File.Exists(candidate))
            return false;

        foreach (var loaded in AppDomain.CurrentDomain.GetAssemblies())
        {
            if (string.Equals(loaded.GetName().Name, dependencyName, StringComparison.Ordinal))
            {
                var loc = loaded.Location;
                if (!string.IsNullOrEmpty(loc) &&
                    !string.Equals(Path.GetFullPath(loc), Path.GetFullPath(candidate),
                        StringComparison.Ordinal))
                {
                    return false;
                }
            }
        }

        return true;
    }
}

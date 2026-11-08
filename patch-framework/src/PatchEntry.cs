using System.Reflection;

namespace Lynncher.Patches;

public static class PatchEntry
{
    public static IDisposable InstallEngineHooks(Assembly clientAssembly, Assembly sharedAssembly,
        string robustClientExecutable)
    {
        _ = sharedAssembly;
        _ = robustClientExecutable;
        var handle = new NoopHandle();

        try
        {
            if (!PatchConfig.Load())
                return handle;

            var loaded = PatchInstaller.LoadBundles();
            if (loaded == 0)
            {
                Console.WriteLine("[lynncher-patches] no bundles loaded; continuing unpatched.");
                return handle;
            }

            if (!PatchInstaller.InstallDeferredTrigger(sharedAssembly, clientAssembly))
            {
                PatchInstaller.ApplyEnginePatches(clientAssembly);
            }

            Console.WriteLine($"[lynncher-patches] {loaded} bundle(s) active (level={(PatchConfig.FullLevel ? "full" : "engine")}).");
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] install failed: {ex}");
        }

        return handle;
    }

    public static void ApplyContentHooks()
    {
        try
        {
            PatchInstaller.ApplyContentPatches();
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] content hooks failed: {ex}");
        }
    }

    private sealed class NoopHandle : IDisposable
    {
        public void Dispose()
        {
        }
    }
}

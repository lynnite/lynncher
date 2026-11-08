using System.Reflection;
using HarmonyLib;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;

namespace HealthBars;

public static class Patch
{
    private const string HarmonyId = "org.lynncher.healthbars";

    private static Harmony? _harmony;

    public static void PatchEngine(Assembly clientAssembly)
    {
        _ = clientAssembly;

        HealthBarConfig.Load();

        if (!HealthBarConfig.Enabled)
        {
            HealthBarController.Log("disabled by configuration");
            return;
        }

        _harmony = new Harmony(HarmonyId);

        var target = AccessTools.Method(typeof(EntitySystemManager), "Initialize");
        if (target is null)
        {
            HealthBarController.Log("could not find EntitySystemManager.Initialize; installing immediately (may fail)");
            TryInstallNow();
            return;
        }

        var postfix = AccessTools.Method(typeof(Patch), nameof(OnEntitySystemsInitialized));
        _harmony.Patch(target, postfix: new HarmonyMethod(postfix));
        HealthBarController.Log("deferred install hooked (EntitySystemManager.Initialize)");
    }

    private static void OnEntitySystemsInitialized()
    {
        try
        {
            var init = AccessTools.Method(typeof(EntitySystemManager), "Initialize");
            if (init is not null)
                _harmony?.Unpatch(init, HarmonyPatchType.Postfix, HarmonyId);
        }
        catch
        {
        }

        TryInstallNow();
    }

    private static void TryInstallNow()
    {
        try
        {
            if (!HealthBarConfig.Enabled)
                return;

            var entManager = IoCManager.Resolve<IEntityManager>();
            HealthBarController.Install(entManager);
        }
        catch (Exception ex)
        {
            HealthBarController.Log($"install failed: {ex}");
        }
    }
}

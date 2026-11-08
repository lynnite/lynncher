using System.Reflection;
using HarmonyLib;
using Robust.Client.Graphics;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;

namespace HitMarker;

public static class Patch
{
    private static Harmony? _harmony;

    public static void PatchEngine(Assembly clientAssembly)
    {
        HitMarkerConfig.Load();

        if (!HitMarkerConfig.Enabled)
        {
            HitPatches.Log("disabled by configuration");
            return;
        }

        var entManager = IoCManager.Resolve<IEntityManager>();
        HitPatches.EntManager = entManager;

        _harmony = new Harmony("org.lynncher.hitmarker");
        HitPatches.Apply(_harmony);

        var overlays = IoCManager.Resolve<IOverlayManager>();
        overlays.AddOverlay(new HitMarkerOverlay());

        HitPatches.Log(
            $"ready (hiddenFromRecording={HitMarkerConfig.HiddenFromRecording}, " +
            $"sound={HitMarkerConfig.PlaySound}, damageNumbers={HitMarkerConfig.ShowDamageNumbers})");
    }
}

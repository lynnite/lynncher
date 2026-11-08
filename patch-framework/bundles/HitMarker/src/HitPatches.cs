using Content.Shared.Projectiles;
using Content.Shared.Weapons.Melee;
using HarmonyLib;
using Robust.Shared.GameObjects;
using Robust.Shared.Player;
using XenoComponent = Content.Shared._RMC14.Xenonids.XenoComponent;

namespace HitMarker;

internal static class HitPatches
{
    internal static IEntityManager? EntManager;

    private static Robust.Client.Player.IPlayerManager? _players;

    internal static EntityUid? LocalPlayer()
    {
        if (EntManager is null)
            return null;
        _players ??= Robust.Shared.IoC.IoCManager.Resolve<Robust.Client.Player.IPlayerManager>();
        return _players?.LocalEntity;
    }

    private static bool IsXeno(EntityUid uid)
    {
        if (EntManager is null)
            return false;
        if (uid == EntityUid.Invalid || !EntManager.EntityExists(uid))
            return false;
        return EntManager.HasComponent<XenoComponent>(uid);
    }

    internal static void Apply(Harmony harmony)
    {
        ApplyRanged(harmony);
        ApplyMelee(harmony);
    }

    private static void ApplyRanged(Harmony harmony)
    {
        var target = AccessTools.Method(
            typeof(SharedProjectileSystem),
            nameof(SharedProjectileSystem.ProjectileCollide));
        if (target is null)
        {
            Log("projectile hook: ProjectileCollide not found");
            return;
        }

        var prefix = AccessTools.Method(typeof(HitPatches), nameof(ProjectileCollidePrefix));
        var postfix = AccessTools.Method(typeof(HitPatches), nameof(ProjectileHitPostfix));
        harmony.Patch(target,
            prefix: new HarmonyMethod(prefix),
            postfix: new HarmonyMethod(postfix));
        Log("projectile hook installed");
    }

    private static void ApplyMelee(Harmony harmony)
    {
        var clientMelee = AccessTools.TypeByName("Content.Client.Weapons.Melee.MeleeWeaponSystem");
        if (clientMelee is null)
        {
            Log("melee hook: client MeleeWeaponSystem not found");
            return;
        }

        var target = AccessTools.Method(clientMelee, "DoDamageEffect");
        if (target is null)
        {
            Log("melee hook: DoDamageEffect not found");
            return;
        }

        var postfix = AccessTools.Method(typeof(HitPatches), nameof(MeleeDamageEffectPostfix));
        harmony.Patch(target, postfix: new HarmonyMethod(postfix));
        Log("melee hook installed");
    }

    private readonly struct ProjectileState
    {
        public readonly bool AlreadySpent;
        public ProjectileState(bool alreadySpent) => AlreadySpent = alreadySpent;
    }

    private static bool ProjectileCollidePrefix(
        Entity<ProjectileComponent, Robust.Shared.Physics.Components.PhysicsComponent> projectile,
        out ProjectileState __state)
    {
        __state = new ProjectileState(projectile.Comp1.ProjectileSpent);
        return true;
    }

    private static void ProjectileHitPostfix(
        Entity<ProjectileComponent, Robust.Shared.Physics.Components.PhysicsComponent> projectile,
        EntityUid target,
        ProjectileState __state)
    {
        try
        {
            if (!HitMarkerConfig.Enabled)
                return;

            if (__state.AlreadySpent || !projectile.Comp1.ProjectileSpent)
                return;

            var local = LocalPlayer();
            if (local is null)
                return;

            var comp = projectile.Comp1;
            if (comp.Shooter != local.Value)
                return;

            if (target == local.Value)
                return;

            if (!IsXeno(target))
                return;

            var dmg = (int)Math.Round(comp.Damage?.GetTotal().Double() ?? 0d);
            HitEventBus.Publish(dmg);
            HitSoundPlayer.Play();
        }
        catch
        {
        }
    }


    private static void MeleeDamageEffectPostfix(List<EntityUid> targets, EntityUid? user)
    {
        try
        {
            if (!HitMarkerConfig.Enabled)
                return;

            var local = LocalPlayer();
            if (local is null || user != local)
                return;

            var hitXeno = false;
            if (targets is not null)
            {
                foreach (var target in targets)
                {
                    if (IsXeno(target))
                    {
                        hitXeno = true;
                        break;
                    }
                }
            }

            if (!hitXeno)
                return;

            HitEventBus.Publish(0);
        }
        catch
        {
        }
    }

    internal static void Log(string msg)
    {
        Console.WriteLine($"[hitmarker] {msg}");
    }
}

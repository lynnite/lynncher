namespace HitMarker;

public readonly struct HitInfo
{
    public readonly int Damage;
    public readonly double Time;

    public HitInfo(int damage, double time)
    {
        Damage = damage;
        Time = time;
    }
}

public static class HitEventBus
{
    private static readonly object _lock = new();

    private static int _pendingDamage;
    private static double _pendingDamageStart;

    public static event Action<HitInfo>? OnHit;

    public static void Publish(int damage)
    {
        if (!HitMarkerConfig.Enabled)
            return;

        var now = HitMarkerConfig.NowSeconds();

        int total;
        lock (_lock)
        {
            if (now - _pendingDamageStart > HitMarkerConfig.DamageCoalesceSeconds)
            {
                _pendingDamage = 0;
                _pendingDamageStart = now;
            }
            _pendingDamage += Math.Max(0, damage);
            total = _pendingDamage;
        }

        OnHit?.Invoke(new HitInfo(total, now));
    }
}

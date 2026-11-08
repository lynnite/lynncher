using System.Globalization;
using Robust.Shared.Maths;

namespace HitMarker;

public static class HitMarkerConfig
{
    public static bool Enabled { get; private set; } = true;

    public static bool HiddenFromRecording { get; private set; }

    public static bool VisualsMuted { get; set; }

    public static bool PlaySound { get; private set; } = true;
    public static bool ShowDamageNumbers { get; private set; } = true;

    public static float Volume { get; set; } = 0.5f;

    public const float VolumeStep = 0.1f;

    public static float AdjustVolume(float delta)
    {
        Volume = Math.Clamp(Volume + delta, 0f, 1f);
        return Volume;
    }

    public static Color MarkerColor { get; private set; } = new(255, 255, 255);

    public static double DamageCoalesceSeconds { get; private set; } = 0.7;

    public static double MarkerLifetime { get; private set; } = 0.45;

    private static bool _loaded;
    private static readonly System.Diagnostics.Stopwatch Clock = System.Diagnostics.Stopwatch.StartNew();

    public static double NowSeconds() => Clock.Elapsed.TotalSeconds;

    public static void Load()
    {
        if (_loaded)
            return;
        _loaded = true;

        Enabled = BoolEnv("LYNNCHER_HITMARKER", true);
        HiddenFromRecording = BoolEnv("LYNNCHER_HITMARKER_HIDDEN", false);
        VisualsMuted = HiddenFromRecording;
        PlaySound = BoolEnv("LYNNCHER_HITMARKER_SOUND", true);
        ShowDamageNumbers = BoolEnv("LYNNCHER_HITMARKER_DAMAGE", true);
        Volume = FloatEnv("LYNNCHER_HITMARKER_VOLUME", 0.5f, 0f, 1f);

        var color = Env("LYNNCHER_HITMARKER_COLOR");
        if (!string.IsNullOrEmpty(color))
        {
            var parts = color.Split(',');
            if (parts.Length == 3 &&
                byte.TryParse(parts[0], out var r) &&
                byte.TryParse(parts[1], out var g) &&
                byte.TryParse(parts[2], out var b))
            {
                MarkerColor = new Color(r, g, b);
            }
        }
    }

    private static string? Env(string key) => Environment.GetEnvironmentVariable(key);

    private static bool BoolEnv(string key, bool fallback)
    {
        var raw = Env(key);
        if (string.IsNullOrEmpty(raw))
            return fallback;
        return raw is "1" || raw.Equals("true", StringComparison.OrdinalIgnoreCase);
    }

    private static float FloatEnv(string key, float fallback, float min, float max)
    {
        var raw = Env(key);
        if (string.IsNullOrEmpty(raw))
            return fallback;
        if (float.TryParse(raw, NumberStyles.Float, CultureInfo.InvariantCulture, out var v))
            return Math.Clamp(v, min, max);
        return fallback;
    }
}

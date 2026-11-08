namespace HealthBars;

public static class HealthBarConfig
{
    public static bool Enabled { get; private set; } = true;

    public static bool Visible { get; set; } = true;

    public static bool HiddenFromRecording { get; private set; }

    public static IReadOnlyList<string> DamageContainers { get; private set; } =
        new[] { "Biological", "Xeno", "StructuralXeno" };

    public static string? StatusIcon { get; private set; }

    private static bool _loaded;

    public static void Load()
    {
        if (_loaded)
            return;
        _loaded = true;

        Enabled = BoolEnv("LYNNCHER_HEALTHBARS", true);
        HiddenFromRecording = BoolEnv("LYNNCHER_HEALTHBARS_HIDDEN", false);
        Visible = !HiddenFromRecording;

        var containers = Env("LYNNCHER_HEALTHBARS_CONTAINERS");
        if (!string.IsNullOrEmpty(containers))
        {
            var parsed = new List<string>();
            foreach (var part in containers.Split(',', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries))
            {
                if (part.Length > 0)
                    parsed.Add(part);
            }

            if (parsed.Count > 0)
                DamageContainers = parsed;
        }

        var icon = Env("LYNNCHER_HEALTHBARS_STATUS_ICON");
        if (!string.IsNullOrEmpty(icon))
            StatusIcon = icon;
    }

    private static string? Env(string key) => Environment.GetEnvironmentVariable(key);

    private static bool BoolEnv(string key, bool fallback)
    {
        var raw = Env(key);
        if (string.IsNullOrEmpty(raw))
            return fallback;
        return raw is "1" || raw.Equals("true", StringComparison.OrdinalIgnoreCase);
    }
}


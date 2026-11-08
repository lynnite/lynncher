using System.Text.Json;
using System.Text.Json.Serialization;

namespace Lynncher.Patches;

public sealed class PatchBundleConfig
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("root")] public string Root { get; set; } = "";
    [JsonPropertyName("assembly")] public string? Assembly { get; set; }
    [JsonPropertyName("resources")] public string? Resources { get; set; }
}

[JsonSerializable(typeof(List<PatchBundleConfig>))]
internal partial class PatchJsonContext : JsonSerializerContext
{
}

public static class PatchConfig
{
    public const string EnvEnable = "LYNNCHER_PATCHING";
    public const string EnvLevel = "LYNNCHER_PATCH_LEVEL";
    public const string EnvBundles = "LYNNCHER_PATCH_BUNDLES";

    public static bool Enabled { get; private set; }

    public static bool FullLevel { get; private set; }

    public static bool ContentLevel => FullLevel;

    public static IReadOnlyList<PatchBundleConfig> Bundles { get; private set; } = Array.Empty<PatchBundleConfig>();

    private static bool _loaded;

    public static bool Load()
    {
        if (_loaded)
            return Enabled && Bundles.Count > 0;

        _loaded = true;

        var enable = Environment.GetEnvironmentVariable(EnvEnable);
        Enabled = enable == "1" || string.Equals(enable, "true", StringComparison.OrdinalIgnoreCase);

        var level = Environment.GetEnvironmentVariable(EnvLevel);
        FullLevel = !string.IsNullOrEmpty(level) &&
                    level.Equals("full", StringComparison.OrdinalIgnoreCase);

        var raw = Environment.GetEnvironmentVariable(EnvBundles);
        if (!string.IsNullOrEmpty(raw))
        {
            try
            {
                Bundles = JsonSerializer.Deserialize(raw, PatchJsonContext.Default.ListPatchBundleConfig)
                          ?? new List<PatchBundleConfig>();
            }
            catch (Exception ex)
            {
                Console.WriteLine($"[lynncher-patches] failed to parse {EnvBundles}: {ex.Message}");
                Bundles = Array.Empty<PatchBundleConfig>();
            }
        }

        var safe = new List<PatchBundleConfig>(Bundles.Count);
        foreach (var b in Bundles)
        {
            if (string.IsNullOrEmpty(b.Root) || !System.IO.Path.IsPathRooted(b.Root))
            {
                Console.WriteLine($"[lynncher-patches] ignoring bundle with non-absolute root: {b.Name}");
                continue;
            }
            safe.Add(b);
        }
        Bundles = safe;

        return Enabled && Bundles.Count > 0;
    }
}

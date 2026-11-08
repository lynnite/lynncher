using Robust.Client.Audio;
using Robust.Shared.Audio;
using Robust.Shared.IoC;

namespace HitMarker;

internal static class HitSoundPlayer
{
    private const string EmbeddedResourceName = "HitMarker.Resources.hitmarker_2.ogg";

    private static AudioStream? _stream;
    private static bool _loadAttempted;

    private static Robust.Client.Audio.AudioSystem? _audio;
    private static double _lastPlay;
    private const double MinInterval = 0.03;

    public static void Play()
    {
        if (!HitMarkerConfig.PlaySound)
            return;
        if (HitPatches.EntManager is null)
            return;

        var now = HitMarkerConfig.NowSeconds();
        if (now - _lastPlay < MinInterval)
            return;
        _lastPlay = now;

        _audio ??= HitPatches.EntManager.EntitySysManager
            .GetEntitySystem<Robust.Client.Audio.AudioSystem>();

        if (_audio is null)
            return;

        var stream = GetStream();
        if (stream is null)
            return;

        var parms = AudioParams.Default
            .WithVolume(HitMarkerConfig.Volume)
            .WithMaxDistance(0f)
            .WithVariation(0.06f);

        _audio.PlayGlobal(stream, null, parms);
    }

    private static AudioStream? GetStream()
    {
        if (_loadAttempted)
            return _stream;
        _loadAttempted = true;

        try
        {
            var asm = typeof(HitSoundPlayer).Assembly;
            using var resource = asm.GetManifestResourceStream(EmbeddedResourceName);
            if (resource is null)
            {
                HitPatches.Log($"sound resource '{EmbeddedResourceName}' not embedded; hit sound disabled");
                return null;
            }

            var audioManager = IoCManager.Resolve<IAudioManager>();
            _stream = audioManager.LoadAudioOggVorbis(resource, "hitmarker_2.ogg");
            return _stream;
        }
        catch (Exception ex)
        {
            HitPatches.Log($"failed to load hit sound: {ex.Message}");
            return null;
        }
    }
}

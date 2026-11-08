using System.Numerics;
using Robust.Client.Graphics;
using Robust.Client.Input;
using Robust.Shared.Enums;
using Robust.Shared.IoC;
using Robust.Shared.Maths;

namespace HitMarker;

public sealed class HitMarkerOverlay : Overlay
{
    public override OverlaySpace Space => OverlaySpace.ScreenSpace;
    public override bool RequestScreenTexture => false;

    private readonly IInputManager _input;
    private bool _prevToggleDown;
    private bool _prevVolDown;
    private bool _prevVolUp;

    private const float ArmLength = 9f;
    private const float ArmGap = 3f;
    private const float Thickness = 2.5f;

    private readonly List<ActiveMarker> _markers = new();
    private readonly object _lock = new();

    private IInputManager Input => _input;

    public HitMarkerOverlay()
    {
        _input = IoCManager.Resolve<IInputManager>();
        HitEventBus.OnHit += OnHit;
    }

    private void OnHit(HitInfo info)
    {
        lock (_lock)
        {
            _markers.Add(new ActiveMarker
            {
                Damage = info.Damage,
                Start = info.Time,
            });
            if (_markers.Count > 64)
                _markers.RemoveRange(0, _markers.Count - 64);
        }
    }

    protected override void FrameUpdate(Robust.Shared.Timing.FrameEventArgs args)
    {
        var down = Input.IsKeyDown(Robust.Client.Input.Keyboard.Key.F9);
        if (down && !_prevToggleDown)
        {
            HitMarkerConfig.VisualsMuted = !HitMarkerConfig.VisualsMuted;
            HitPatches.Log($"visuals {(HitMarkerConfig.VisualsMuted ? "hidden" : "shown")} (F9)");
        }
        _prevToggleDown = down;

        var volDown = Input.IsKeyDown(Robust.Client.Input.Keyboard.Key.LBracket);
        if (volDown && !_prevVolDown)
        {
            var v = HitMarkerConfig.AdjustVolume(-HitMarkerConfig.VolumeStep);
            HitPatches.Log($"hit sound volume {v * 100f:0}% ([)");
        }
        _prevVolDown = volDown;

        var volUp = Input.IsKeyDown(Robust.Client.Input.Keyboard.Key.RBracket);
        if (volUp && !_prevVolUp)
        {
            var v = HitMarkerConfig.AdjustVolume(HitMarkerConfig.VolumeStep);
            HitPatches.Log($"hit sound volume {v * 100f:0}% (])");
        }
        _prevVolUp = volUp;
    }

    protected override bool BeforeDraw(in OverlayDrawArgs args)
    {
        if (!HitMarkerConfig.Enabled || HitMarkerConfig.VisualsMuted)
            return false;

        lock (_lock)
            return _markers.Count > 0;
    }

    protected override void Draw(in OverlayDrawArgs args)
    {
        var handle = args.ScreenHandle;
        var now = HitMarkerConfig.NowSeconds();
        var life = HitMarkerConfig.MarkerLifetime;

        var viewport = args.Viewport;
        var center = viewport.Size / 2f;
        var mouse = Input.MouseScreenPosition.Position;
        var origin = new Vector2(mouse.X, mouse.Y);
        if (float.IsNaN(origin.X) || origin.X < 0 || origin.X > viewport.Size.X ||
            origin.Y < 0 || origin.Y > viewport.Size.Y)
        {
            origin = new Vector2(center.X, center.Y);
        }

        lock (_lock)
        {
            for (var i = _markers.Count - 1; i >= 0; i--)
            {
                var m = _markers[i];
                var age = now - m.Start;
                if (age >= life || age < 0)
                {
                    _markers.RemoveAt(i);
                    continue;
                }

                var t = (float)(age / life);
                var fade = 1f - t;
                var scale = 1f + 0.35f * t;
                var color = HitMarkerConfig.MarkerColor.WithAlpha(fade);

                DrawCross(handle, origin, scale, color);

                if (HitMarkerConfig.ShowDamageNumbers && m.Damage > 0)
                    DrawDamage(handle, origin, m.Damage, fade, t);
            }
        }
    }

    private static void DrawCross(DrawingHandleScreen handle, Vector2 c, float scale, Color color)
    {
        var arm = ArmLength * scale;
        var gap = ArmGap * scale;

        DrawThick(handle, c + new Vector2(-gap, -gap), c + new Vector2(-gap - arm, -gap - arm), Thickness, color);
        DrawThick(handle, c + new Vector2(gap, -gap), c + new Vector2(gap + arm, -gap - arm), Thickness, color);
        DrawThick(handle, c + new Vector2(-gap, gap), c + new Vector2(-gap - arm, gap + arm), Thickness, color);
        DrawThick(handle, c + new Vector2(gap, gap), c + new Vector2(gap + arm, gap + arm), Thickness, color);
    }

    private static void DrawDamage(DrawingHandleScreen handle, Vector2 origin, int damage, float fade, float t)
    {
        var text = damage.ToString();
        var color = new Color(1f, 0.85f, 0.3f, fade);
        var y = origin.Y + 18f - 26f * t;
        var x = origin.X;
        var scale = 1.2f * 2.5f;

        foreach (var ch in text)
            x += DrawDigit(handle, ch, new Vector2(x, y), scale, color);
    }

    private static readonly byte[,] Digits = BuildDigits();

    private static byte[,] BuildDigits()
    {
        var d = new byte[10, 5];
        void Set(int n, params int[] rows)
        {
            for (var i = 0; i < 5; i++) d[n, i] = (byte)rows[i];
        }
        Set(0, 0b111, 0b101, 0b101, 0b101, 0b111);
        Set(1, 0b010, 0b110, 0b010, 0b010, 0b111);
        Set(2, 0b111, 0b001, 0b111, 0b100, 0b111);
        Set(3, 0b111, 0b001, 0b111, 0b001, 0b111);
        Set(4, 0b101, 0b101, 0b111, 0b001, 0b001);
        Set(5, 0b111, 0b100, 0b111, 0b001, 0b111);
        Set(6, 0b111, 0b100, 0b111, 0b101, 0b111);
        Set(7, 0b111, 0b001, 0b010, 0b010, 0b010);
        Set(8, 0b111, 0b101, 0b111, 0b101, 0b111);
        Set(9, 0b111, 0b101, 0b111, 0b001, 0b111);
        return d;
    }

    private static float DrawDigit(DrawingHandleScreen handle, char ch, Vector2 topLeft, float scale, Color color)
    {
        if (ch < '0' || ch > '9')
            return 4f * scale;
        var n = ch - '0';
        const float px = 1.0f;
        for (var row = 0; row < 5; row++)
        {
            var bits = Digits[n, row];
            for (var col = 0; col < 3; col++)
            {
                if ((bits & (0b100 >> col)) == 0)
                    continue;
                var pos = topLeft + new Vector2(col * px * scale, row * px * scale);
                var size = new Vector2(px * scale, px * scale);
                handle.DrawRect(new UIBox2(pos, pos + size), color, true);
            }
        }
        return 4f * scale;
    }

    private static void DrawThick(DrawingHandleScreen handle, Vector2 a, Vector2 b, float thickness, Color color)
    {
        if (thickness <= 1.01f)
        {
            handle.DrawLine(a, b, color);
            return;
        }

        var dir = b - a;
        var len = dir.Length();
        if (len < 0.001f)
            return;
        var perp = new Vector2(-dir.Y, dir.X) / len * (thickness * 0.5f);

        handle.DrawLine(a + perp, b + perp, color);
        handle.DrawLine(a - perp, b - perp, color);
        if (thickness > 2.5f)
        {
            handle.DrawLine(a, b, color);
        }
    }

    private struct ActiveMarker
    {
        public int Damage;
        public double Start;
    }
}

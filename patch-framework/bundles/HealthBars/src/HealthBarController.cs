using Content.Client.Overlays;
using Robust.Client.Graphics;
using Robust.Client.Input;
using Robust.Shared.Enums;
using Robust.Shared.GameObjects;
using Robust.Shared.IoC;
using Robust.Shared.Prototypes;

namespace HealthBars;

internal static class HealthBarController
{
    private const string LogPrefix = "[healthbars]";

    private static EntityHealthBarOverlay? _overlay;
    private static IOverlayManager? _overlayManager;
    private static bool _installed;

    public static void Install(IEntityManager entManager)
    {
        if (_installed)
            return;

        try
        {
            _overlayManager = IoCManager.Resolve<IOverlayManager>();
            var prototype = IoCManager.Resolve<IPrototypeManager>();

            _overlay = new EntityHealthBarOverlay(entManager, prototype);
            foreach (var id in HealthBarConfig.DamageContainers)
                _overlay.DamageContainers.Add(id);

            if (HealthBarConfig.StatusIcon is { } icon)
                _overlay.StatusIcon = icon;

            if (!_overlayManager.HasOverlay<ToggleOverlay>())
                _overlayManager.AddOverlay(new ToggleOverlay());

            _installed = true;

            if (HealthBarConfig.Visible)
                Show();

            Log($"ready (containers={string.Join(",", HealthBarConfig.DamageContainers)}, " +
                $"statusIcon={HealthBarConfig.StatusIcon ?? "<none>"}, visible={HealthBarConfig.Visible}, " +
                $"overlayPresent={_overlayManager.HasOverlay<EntityHealthBarOverlay>()})");
        }
        catch (Exception ex)
        {
            Log($"install failed: {ex}");
        }
    }

    public static void Show()
    {
        if (_overlay is null || _overlayManager is null)
            return;
        if (!_overlayManager.HasOverlay<EntityHealthBarOverlay>())
        {
            var added = _overlayManager.AddOverlay(_overlay);
            Log($"AddOverlay returned {added}");
        }
        HealthBarConfig.Visible = true;
    }

    public static void Hide()
    {
        if (_overlay is null || _overlayManager is null)
            return;
        if (_overlayManager.HasOverlay<EntityHealthBarOverlay>())
            _overlayManager.RemoveOverlay(_overlay);
        HealthBarConfig.Visible = false;
    }

    public static void Toggle()
    {
        if (HealthBarConfig.Visible)
        {
            Hide();
            Log("health bars hidden");
        }
        else
        {
            Show();
            Log("health bars shown");
        }
    }

    internal static void Log(string msg) => Console.WriteLine($"{LogPrefix} {msg}");

    private sealed class ToggleOverlay : Overlay
    {
        public override OverlaySpace Space => OverlaySpace.ScreenSpace;

        private readonly IInputManager _input;
        private bool _prevDown;

        public ToggleOverlay()
        {
            _input = IoCManager.Resolve<IInputManager>();
        }

        protected override void FrameUpdate(Robust.Shared.Timing.FrameEventArgs args)
        {
            var down = _input.IsKeyDown(Robust.Client.Input.Keyboard.Key.F8);
            if (down && !_prevDown)
                Toggle();
            _prevDown = down;
        }

        protected override void Draw(in OverlayDrawArgs args)
        {
        }
    }
}


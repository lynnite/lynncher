using System.Collections.Concurrent;
using System.Diagnostics;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.Loader;

namespace Lynncher.Patches;

public static class AssemblyHidingManager
{
    private static readonly ConcurrentDictionary<string, bool> _hidden = new();

    private static readonly string[] _alwaysHiddenDynamic =
    [
        "Anonymously Hosted DynamicMethods Assembly",
        "HarmonySharedState",
    ];

    private const string HiddenLoadContextName = "Assembly.Load(byte[], ...)";

    [ThreadStatic] private static bool _inTypeAssemblyHook;

    private static readonly object _lock = new();
    private static bool _patched;
    private static Assembly? _surrogate;

    public static void HideBasicAssemblies()
    {
        HideAssembly("Lynncher", exact: false);
        HideAssembly("Harmony", exact: false);
        HideAssembly("MonoMod", exact: false);
        HideAssembly("0Harmony", exact: false);
        HideAssembly("System.Reflection.Emit", exact: true);
    }

    public static void HideAssembly(string identifier, bool exact)
    {
        if (string.IsNullOrWhiteSpace(identifier))
            throw new ArgumentException("Assembly identifier must not be empty.", nameof(identifier));

        _hidden[identifier] = exact;
    }

    public static void HideAssembly(Assembly assembly, bool exact = false)
        => HideAssembly(AssemblyNameOf(assembly), exact);

    public static void HideAssemblyByName(string name) => HideAssembly(name, exact: true);

    public static void PatchDetectionVectors()
    {
        lock (_lock)
        {
            if (_patched)
                return;

            var postfix = GetMethod(nameof(DetectionVectorPostfix));

            TryPatch(typeof(AppDomain).GetMethod(nameof(AppDomain.GetAssemblies)), postfix);
            TryPatch(typeof(AssemblyLoadContext).GetProperty("Assemblies")?.GetGetMethod(), postfix);
            TryPatch(typeof(AssemblyLoadContext).GetProperty("All")?.GetGetMethod(), postfix);
            TryPatch(typeof(Assembly).GetMethod(nameof(Assembly.GetTypes)), postfix);
            TryPatch(typeof(Assembly).GetMethod(nameof(Assembly.GetType), [typeof(string)]), postfix);

            var execAsmType = typeof(AssemblyHidingManager).Assembly.GetType();
            TryPatch(execAsmType?.GetMethod(nameof(Assembly.GetReferencedAssemblies)), postfix);

            var runtimeAssembly = typeof(object).Assembly.GetType();
            TryPatch(runtimeAssembly?.GetProperty("DefinedTypes")?.GetGetMethod(), postfix);
            TryPatch(runtimeAssembly?.GetProperty(nameof(Assembly.Location))?.GetGetMethod(),
                GetMethod(nameof(LieLocation)));
            TryPatch(runtimeAssembly?.GetMethod(nameof(Assembly.GetType), [typeof(string), typeof(bool), typeof(bool)]),
                GetMethod(nameof(GetTypeCaseSensitiveThrowOnErrorPatch)));

            TryPatch(typeof(Environment).GetProperty("StackTrace")?.GetGetMethod(),
                GetMethod(nameof(LieStackTrace)));
            TryPatch(typeof(Exception).GetProperty("StackTrace")?.GetGetMethod(),
                GetMethod(nameof(LieExceptionStackTrace)));

            PatchTypeAssemblyGetter();

            PatchEngineEnumerators(postfix);

            _patched = true;
        }
    }

    private static void PatchEngineEnumerators(MethodInfo postfix)
    {
        var reflectionManager = PatchTools.GetTypeByQualifiedName("Robust.Shared.Reflection.ReflectionManager");
        if (reflectionManager is not null)
        {
            TryPatch(reflectionManager.GetProperty("Assemblies")?.GetGetMethod(), postfix);
            TryPatch(reflectionManager.GetMethod("FindAllTypes",
                BindingFlags.Public | BindingFlags.Instance, null, Type.EmptyTypes, null), postfix);
        }
    }

    private static void PatchTypeAssemblyGetter()
    {
        var targets = new List<MethodInfo>();
        var baseGetter = typeof(Type).GetProperty(nameof(Type.Assembly))?.GetGetMethod();
        if (baseGetter is { IsAbstract: false })
            targets.Add(baseGetter);

        var runtimeType = typeof(object).GetType();
        var runtimeGetter = runtimeType
            .GetProperty(nameof(Type.Assembly), BindingFlags.Public | BindingFlags.Instance)
            ?.GetGetMethod();
        if (runtimeGetter is { IsAbstract: false } && !targets.Contains(runtimeGetter))
            targets.Add(runtimeGetter);

        var postfix = GetMethod(nameof(LieTypeAssembly));
        foreach (var t in targets)
            TryPatch(t, postfix);
    }

    private static void DetectionVectorPostfix(ref object __result)
    {
        if (__result is null || IsCallsiteThroughHiddenAssembly())
            return;

        switch (__result)
        {
            case Assembly[] a:
                __result = a.Where(x => !ShouldHide(AssemblyNameOf(x))).ToArray();
                break;
            case IEnumerable<Assembly> a:
                __result = a.Where(x => !ShouldHide(AssemblyNameOf(x))).ToArray();
                break;
            case IEnumerable<AssemblyLoadContext> c:
                __result = c.Where(x => x.Name != HiddenLoadContextName).ToArray();
                break;
            case Type[] t:
                __result = t.Where(x => !ShouldHideType(x)).ToArray();
                break;
            case Type t:
                if (ShouldHideType(t))
                    __result = null!;
                break;
            case IEnumerable<TypeInfo> ti:
                __result = ti.Where(x => !ShouldHideType(x)).ToArray();
                break;
            case IEnumerable<Type> te:
                __result = te.Where(x => !ShouldHideType(x)).ToList();
                break;
            case AssemblyName[] an:
                __result = an.Where(x => !ShouldHide(x.Name ?? "")).ToArray();
                break;
        }
    }

    private static void LieLocation(ref string __result, Assembly __instance)
    {
        if (IsCallsiteThroughHiddenAssembly())
            return;
        if (!ShouldHide(AssemblyNameOf(__instance)))
            return;
        __result = typeof(object).Assembly.Location;
    }

    private static void LieStackTrace(ref string __result)
    {
        if (IsCallsiteThroughHiddenAssembly())
            return;
        __result = FilterTrace(__result);
    }

    private static void LieExceptionStackTrace(ref string __result)
    {
        if (IsCallsiteThroughHiddenAssembly())
            return;
        __result = FilterTrace(__result);
    }

    private static string FilterTrace(string original)
    {
        if (string.IsNullOrEmpty(original))
            return original;

        var lines = original.Split('\n');
        var kept = new List<string>(lines.Length);
        foreach (var line in lines)
        {
            if (!StackLineHidden(line))
                kept.Add(line);
        }
        return string.Join('\n', kept);
    }

    private static bool StackLineHidden(string line)
    {
        foreach (var key in _hidden.Keys)
        {
            if (line.Contains(key, StringComparison.Ordinal))
                return true;
        }
        foreach (var dyn in _alwaysHiddenDynamic)
        {
            if (line.Contains(dyn, StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }

    private static void LieTypeAssembly(ref Assembly __result, Type __instance)
    {
        if (_inTypeAssemblyHook)
            return;

        _inTypeAssemblyHook = true;
        try
        {
            if (IsCallsiteThroughHiddenAssembly())
                return;
            if (IsInternalReflectionCaller())
                return;
            if (!ShouldHide(AssemblyNameOf(__result)) && !ShouldHideType(__instance))
                return;
            __result = Surrogate();
        }
        finally
        {
            _inTypeAssemblyHook = false;
        }
    }

    private static void GetTypeCaseSensitiveThrowOnErrorPatch(ref Type? __result, ref string name,
        ref bool throwOnError, ref bool ignoreCase)
    {
        if (__result is null || !ShouldHideType(__result) || IsCallsiteThroughHiddenAssembly())
            return;
        if (throwOnError)
            throw new TypeLoadException($"Type {name} could not be found");
        __result = null;
    }

    private static Assembly Surrogate()
    {
        if (_surrogate is { IsDynamic: false })
            return _surrogate;
        _surrogate = AppDomain.CurrentDomain.GetAssemblies()
            .FirstOrDefault(a => AssemblyNameOf(a) == "Robust.Client" && !a.IsDynamic);
        return _surrogate ?? typeof(object).Assembly;
    }

    private static bool IsInternalReflectionCaller()
    {
        var frames = new StackTrace(2, false).GetFrames();
        foreach (var frame in frames.Take(10))
        {
            var m = frame.GetMethod();
            if (m?.DeclaringType is not { } dt)
                continue;
            var asm = AssemblyNameOf(dt.Assembly);
            if (asm.Contains("Harmony", StringComparison.Ordinal) ||
                asm.Contains("MonoMod", StringComparison.Ordinal) ||
                asm.Contains("Mono.Cecil", StringComparison.Ordinal))
                return true;
            if (dt.FullName is { } fn &&
                (fn.StartsWith("System.Reflection.", StringComparison.Ordinal) ||
                 fn.StartsWith("System.RuntimeType", StringComparison.Ordinal) ||
                 fn.StartsWith("System.RuntimeMethodHandle", StringComparison.Ordinal)))
                return true;
        }
        return false;
    }

    public static bool IsCallsiteThroughHiddenAssembly(int ignoredFirst = 2)
    {
        var frames = new StackTrace().GetFrames();
        var index = 0;
        foreach (var frame in frames)
        {
            if (++index <= ignoredFirst)
                continue;
            var m = frame.GetMethod();
            if (m?.DeclaringType is not { } dt)
                continue;
            if (ShouldHide(AssemblyNameOf(dt.Assembly)))
                return true;
        }
        return false;
    }

    public static bool ShouldHide(string name)
    {
        if (string.IsNullOrEmpty(name))
            return false;

        foreach (var (id, exact) in _hidden)
        {
            if (exact)
            {
                if (name == id)
                    return true;
            }
            else if (name.Contains(id, StringComparison.Ordinal))
            {
                return true;
            }
        }

        foreach (var dyn in _alwaysHiddenDynamic)
        {
            if (string.Equals(name, dyn, StringComparison.OrdinalIgnoreCase))
                return true;
        }
        return false;
    }

    private static bool ShouldHideType(Type type) => ShouldHide(AssemblyNameOf(type.Assembly));

    private static string AssemblyNameOf(Assembly assembly) => assembly.GetName().Name ?? "";

    private static MethodInfo GetMethod(string name)
        => typeof(AssemblyHidingManager).GetMethod(name, BindingFlags.NonPublic | BindingFlags.Static)
           ?? throw new MissingMethodException(name);

    private static void TryPatch(MethodInfo? target, MethodInfo? postfix)
    {
        if (target is null || postfix is null)
            return;
        try
        {
            PatchTools.Harmony.Patch(target, postfix: new HarmonyLib.HarmonyMethod(postfix));
        }
        catch (Exception ex)
        {
            Console.WriteLine($"[lynncher-patches] failed to hide {target.DeclaringType?.Name}.{target.Name}: {ex.Message}");
        }
    }
}

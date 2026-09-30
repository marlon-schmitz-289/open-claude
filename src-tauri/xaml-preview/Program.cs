// Helfer der WPF-Vorschau von Open Claude.
// stdin: XAML (UTF-8). Argument 1: Projektordner. Argument 2 (optional): absoluter Pfad der App.xaml des Projekts.
// stdout: PNG. Bei Fehlern: lesbare Meldung auf stderr (UTF-8) und Exit-Code 1.
// Relative Verweise im XAML (Bilder) gelten ab dem Arbeitsverzeichnis.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.RegularExpressions;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Documents;
using System.Windows.Markup;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using System.Windows.Threading;
using System.Xaml;
using System.Xml;
using System.Xml.Linq;
using XamlReader = System.Windows.Markup.XamlReader;
using XamlParseException = System.Windows.Markup.XamlParseException;

static class Program
{
    static readonly XNamespace X = "http://schemas.microsoft.com/winfx/2006/xaml";
    static readonly XNamespace Blend = "http://schemas.microsoft.com/expression/blend/2008";
    // Verweisen auf Code-Behind, das es hier nicht gibt.
    static readonly string[] CodeOnly = { "Class", "Subclass", "ClassModifier", "FieldModifier" };

    [STAThread]
    static int Main(string[] args)
    {
        try
        {
            var xaml = new StreamReader(Console.OpenStandardInput(), Encoding.UTF8).ReadToEnd();
            var root = Path.GetFullPath(args.Length > 0 ? args[0] : Environment.CurrentDirectory);
            var png = Render(xaml, root, args.Length > 1 ? args[1] : null);
            using var stdout = Console.OpenStandardOutput();
            stdout.Write(png, 0, png.Length);
            return 0;
        }
        catch (Exception e)
        {
            // Roh als UTF-8: die Konsolen-Codepage wuerde Umlaute zerstoeren.
            var msg = Encoding.UTF8.GetBytes(e.Message + "\n");
            using var stderr = Console.OpenStandardError();
            stderr.Write(msg, 0, msg.Length);
            return 1;
        }
    }

    static byte[] Render(string xaml, string project, string? appXaml)
    {
        var doc = Parse(xaml);
        var root = doc.Root!;
        Check(root, true);
        Strip(root);
        var dir = Environment.CurrentDirectory;
        Merge(root, dir, appXaml != null ? Path.GetDirectoryName(Path.GetFullPath(appXaml))! : dir, project);

        // App.xaml ist nur ein Versuch: fehlt etwas daraus, nennt der Fehler unten den Grund.
        string appNote = "";
        if (appXaml != null)
        {
            try { new Application { Resources = AppResources(appXaml, project) }; }
            catch (Exception e) { appNote = $"\nApp.xaml wurde nicht geladen: {Innermost(e).Message}"; }
        }

        object loaded;
        var text = doc.ToString(SaveOptions.DisableFormatting);
        try
        {
            loaded = Load(text, dir);
        }
        catch (XamlParseException e)
        {
            throw new Exception(Describe(e, doc, text) + appNote);
        }
        catch (Exception e)
        {
            throw new Exception(Innermost(e).Message + appNote);
        }

        // d:DesignWidth/-Height zaehlen wie im Designer, wenn das Element keine feste Groesse hat.
        double Design(string name) =>
            double.TryParse((string?)root.Attribute(Blend + name), System.Globalization.NumberStyles.Float,
                System.Globalization.CultureInfo.InvariantCulture, out var v) ? v : double.NaN;

        var host = new Border { Background = Brushes.White };
        double width, height;
        if (loaded is Window win)
        {
            // Ein Fenster laesst sich nicht ohne Anzeige rendern: sein Inhalt kommt in einen Rahmen mit denselben Werten.
            var content = win.Content;
            win.Content = null;
            host.Child = content as UIElement ?? new ContentPresenter { Content = content };
            host.Resources = win.Resources;
            host.DataContext = win.DataContext;
            if (win.Background != null) host.Background = win.Background;
            TextElement.SetFontFamily(host, win.FontFamily);
            TextElement.SetFontSize(host, win.FontSize);
            TextElement.SetForeground(host, win.Foreground);
            (width, height) = (win.Width, win.Height);
        }
        else if (loaded is FrameworkElement el)
        {
            host.Child = el;
            (width, height) = (el.Width, el.Height);
        }
        else
        {
            throw new Exception($"<{root.Name.LocalName}> hat keine Oberfläche, die sich darstellen lässt.");
        }
        if (double.IsNaN(width)) width = Design("DesignWidth");
        if (double.IsNaN(height)) height = Design("DesignHeight");
        int w = Pixels(width, 800), h = Pixels(height, 600);

        host.Measure(new Size(w, h));
        host.Arrange(new Rect(0, 0, w, h));
        host.UpdateLayout();
        // Bindings und dynamische Ressourcen laufen ueber den Dispatcher: einmal leerlaufen lassen.
        host.Dispatcher.Invoke(DispatcherPriority.ApplicationIdle, new Action(() => { }));
        host.UpdateLayout();

        var bitmap = new RenderTargetBitmap(w, h, 96, 96, PixelFormats.Pbgra32);
        bitmap.Render(host);
        var encoder = new PngBitmapEncoder();
        encoder.Frames.Add(BitmapFrame.Create(bitmap));
        using var ms = new MemoryStream();
        encoder.Save(ms);
        return ms.ToArray();
    }

    static int Pixels(double value, int fallback) =>
        double.IsNaN(value) || double.IsInfinity(value) ? fallback : (int)Math.Clamp(value, 1, 4096);

    static XDocument Parse(string xaml)
    {
        try
        {
            return XDocument.Parse(xaml, LoadOptions.SetLineInfo | LoadOptions.PreserveWhitespace);
        }
        catch (XmlException e)
        {
            throw new Exception($"Kein gültiges XML (Zeile {e.LineNumber}): {e.Message}");
        }
    }

    static object Load(string xaml, string baseDir)
    {
        var context = new ParserContext { BaseUri = new Uri(Path.GetFullPath(baseDir) + Path.DirectorySeparatorChar) };
        using var stream = new MemoryStream(Encoding.UTF8.GetBytes(xaml));
        return XamlReader.Load(stream, context);
    }

    /// Entfernt, was nur mit kompiliertem Code-Behind geht: x:Class und Event-Handler.
    static void Strip(XElement root)
    {
        var schema = XamlReader.GetWpfSchemaContext();
        foreach (var el in root.DescendantsAndSelf())
            foreach (var a in el.Attributes().ToList())
                if ((a.Name.Namespace == X && CodeOnly.Contains(a.Name.LocalName)) || IsEvent(schema, el, a))
                    a.Remove();
    }

    static XamlType? TypeOf(XamlSchemaContext schema, string ns, string name)
    {
        try { return schema.GetXamlType(new System.Xaml.Schema.XamlTypeName(ns, name)); }
        catch { return null; }
    }

    /// Click="..." am Element oder angehaengt als Button.Click="...". Unbekannte Typen bleiben unangetastet.
    static bool IsEvent(XamlSchemaContext schema, XElement el, XAttribute a)
    {
        if (a.IsNamespaceDeclaration) return false;
        var name = a.Name.LocalName;
        var dot = name.IndexOf('.');
        if (dot < 0)
            return a.Name.Namespace == XNamespace.None
                && TypeOf(schema, el.Name.NamespaceName, el.Name.LocalName)?.GetMember(name)?.IsEvent == true;
        var ns = a.Name.Namespace == XNamespace.None ? el.GetDefaultNamespace().NamespaceName : a.Name.NamespaceName;
        var owner = TypeOf(schema, ns, name.Substring(0, dot));
        var member = name.Substring(dot + 1);
        return (owner?.GetMember(member) ?? owner?.GetAttachableMember(member))?.IsEvent == true;
    }

    // ---------- Haertung ----------
    // XAML kann Code ausfuehren (ObjectDataProvider ruft beliebige Methoden) und beim Laden ins Netz greifen.
    // Die App rendert deshalb erst nach Freigabe des Projekts; die Pruefung hier faengt zusaetzlich die bekannten Wege ab.
    // ponytail: Sperrliste, keine Sandbox. Dicht waere nur ein eigener Prozess ohne Rechte (AppContainer).

    static readonly string[] Blocked = { "ObjectDataProvider", "XmlDataProvider", "Frame", "WebBrowser", "NavigationWindow" };
    // sys:String, sys:Double ... in Ressourcen sind ueblich und harmlos.
    static readonly Regex SystemNs = new(@"^clr-namespace:System;assembly=(mscorlib|System\.Runtime|System\.Private\.CoreLib)$");
    static readonly Regex Unc = new(@"^\s*\\\\");
    static readonly Regex Scheme = new(@"^\s*(//|(?!pack:)[a-z][a-z0-9+.\-]*://)", RegexOptions.IgnoreCase);
    // Eigenschaften, die WPF als Adresse laedt. Text="https://..." bleibt erlaubt.
    static readonly Regex UriProp = new("(Source|FontFamily|Icon|Cursor)$");

    /// Typen aus eigenem Code oder fremden Assemblies: die kennt der Helfer nicht, und sie duerften alles.
    static bool Foreign(string ns) =>
        (ns.StartsWith("clr-namespace:") || ns.StartsWith("using:")) && !SystemNs.IsMatch(ns);

    static bool RemoteSource(string name, string value) =>
        Unc.IsMatch(value) || (UriProp.IsMatch(name) && Scheme.IsMatch(value));

    /// Wirft bei allem, was Code ausfuehrt oder ins Netz greift. strip: fremde xmlns-Deklarationen fallen weg
    /// (jede Window-Vorlage hat xmlns:local), wer sie benutzt, scheitert danach am unbekannten Praefix.
    static void Check(XElement root, bool strip)
    {
        foreach (var el in root.DescendantsAndSelf())
        {
            var name = el.Name.LocalName;
            Exception No(string why) => new Exception($"Fehler in <{name}> (Zeile {((IXmlLineInfo)el).LineNumber}): {why}");
            if (Foreign(el.Name.NamespaceName))
                throw No("Typen aus eigenem Code (clr-namespace) kann die Vorschau nicht laden.");
            if (Blocked.Contains(name) || (el.Name.Namespace == X && (name == "Code" || name == "Arguments")))
                throw No("Dieses Element zeigt die Vorschau aus Sicherheitsgründen nicht.");
            if (!el.HasElements && RemoteSource(name, el.Value))
                throw No("Quellen aus dem Netz lädt die Vorschau nicht.");
            foreach (var a in el.Attributes().ToList())
            {
                if (a.IsNamespaceDeclaration)
                {
                    if (!Foreign(a.Value)) continue;
                    if (!strip) throw No($"xmlns \"{a.Value}\" kann die Vorschau nicht prüfen.");
                    a.Remove();
                }
                else if (Foreign(a.Name.NamespaceName))
                    throw No("Eigenschaften aus eigenem Code (clr-namespace) kann die Vorschau nicht laden.");
                else if (a.Name.Namespace == X && a.Name.LocalName == "FactoryMethod")
                    throw No("x:FactoryMethod führt die Vorschau aus Sicherheitsgründen nicht aus.");
                else if (RemoteSource(a.Name.LocalName, a.Value))
                    throw No($"{a.Name.LocalName}=\"{a.Value}\": Quellen aus dem Netz lädt die Vorschau nicht.");
            }
        }
    }

    static bool Under(string file, string dir) =>
        file.StartsWith(dir.TrimEnd('/', '\\') + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);

    /// Eingebundene Datei pruefen, samt allem, was sie selbst einbindet. WPF laedt sie spaeter unveraendert von der Platte.
    static void CheckFile(string file, string project, HashSet<string> seen)
    {
        if (!seen.Add(file)) return;
        try
        {
            var root = Parse(File.ReadAllText(file)).Root!;
            Check(root, false);
            var merged = root.DescendantsAndSelf().Where(e => e.Name.LocalName == "ResourceDictionary");
            foreach (var source in merged.Select(e => (string?)e.Attribute("Source")).Where(s => s != null))
            {
                // Nur schlichte relative Pfade: alles andere loest WPF anders auf als diese Pruefung.
                var next = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(file)!, source!));
                if (source!.Contains(':') || source.StartsWith('/') || source.StartsWith('\\') || !Under(next, project) || !File.Exists(next))
                    throw new Exception($"Source=\"{source}\" kann die Vorschau nicht prüfen.");
                CheckFile(next, project, seen);
            }
        }
        catch (Exception e)
        {
            throw new Exception($"{Path.GetFileName(file)}: {e.Message}");
        }
    }

    /// <ResourceDictionary Source="..."> auf gepruefte Dateien im Projekt umschreiben; was es dort nicht gibt, bleibt leer.
    /// dir = Ordner der Datei (relative Verweise), app = Ordner der App.xaml (Verweise ab Wurzel und pack-URIs).
    static void Merge(XElement root, string dir, string app, string project)
    {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var merged in root.DescendantsAndSelf().Where(e => e.Name.LocalName == "ResourceDictionary"))
        {
            var source = (string?)merged.Attribute("Source");
            if (source == null) continue;
            var file = LocalFile(dir, app, source, project);
            if (file != null) CheckFile(file, project, seen);
            merged.SetAttributeValue("Source", file == null ? null : new Uri(file).AbsoluteUri);
        }
    }

    /// Ressourcen aus <Application.Resources>, als eigenes ResourceDictionary geladen.
    // ponytail: eingebundene Woerterbuecher nur aus Dateien im Projekt; Themes aus NuGet-Paketen
    // (MaterialDesign ...) fallen weg. Nachruesten hiesse, die Assemblies des Projekts zu laden.
    static ResourceDictionary AppResources(string appXaml, string project)
    {
        var dir = Path.GetDirectoryName(Path.GetFullPath(appXaml))!;
        var doc = Parse(File.ReadAllText(appXaml));
        var root = doc.Root!;
        var wpf = root.Name.Namespace;
        var holder = root.Element(wpf + "Application.Resources");
        if (holder == null) return new ResourceDictionary();

        var children = holder.Elements().ToList();
        var dict = children.Count == 1 && children[0].Name == wpf + "ResourceDictionary"
            ? children[0]
            : new XElement(wpf + "ResourceDictionary", children);
        // Praefixe in Attributwerten ({x:Type ...}, {StaticResource ...}) brauchen die Deklarationen der Wurzel.
        foreach (var decl in root.Attributes().Where(a => a.IsNamespaceDeclaration))
            if (dict.Attribute(decl.Name) == null) dict.SetAttributeValue(decl.Name, decl.Value);
        Check(dict, true);
        Strip(dict);
        Merge(dict, dir, dir, project);
        return (ResourceDictionary)Load(dict.ToString(SaveOptions.DisableFormatting), dir);
    }

    /// "Themes/A.xaml", "/Themes/A.xaml", "/App;component/Themes/A.xaml", "pack://application:,,,/Themes/A.xaml"
    /// -> Datei im Projekt, null wenn es sie dort nicht gibt. Die letzten drei zaehlen ab dem App.xaml-Ordner.
    static string? LocalFile(string dir, string app, string source, string project)
    {
        const string pack = "pack://application:,,,";
        var rooted = source.StartsWith(pack, StringComparison.OrdinalIgnoreCase) || source.StartsWith('/') || source.StartsWith('\\');
        if (source.StartsWith(pack, StringComparison.OrdinalIgnoreCase)) source = source.Substring(pack.Length);
        var component = source.IndexOf(";component/", StringComparison.OrdinalIgnoreCase);
        if (component >= 0) source = source.Substring(component + ";component/".Length);
        try
        {
            var file = Path.GetFullPath(Path.Combine(rooted || component >= 0 ? app : dir, source.TrimStart('/', '\\')));
            return Under(file, project) && File.Exists(file) ? file : null;
        }
        catch
        {
            return null;
        }
    }

    static Exception Innermost(Exception e)
    {
        while (e.InnerException != null) e = e.InnerException;
        return e;
    }

    /// "Fehler in <Button> (Zeile 12): ..." — Zeile und Name aus dem Original, nicht aus dem bereinigten Text.
    static string Describe(XamlParseException e, XDocument original, string text)
    {
        var reason = Innermost(e).Message;
        if (e.LineNumber <= 0) return reason;
        try
        {
            // Der bereinigte Text hat dieselben Elemente in derselben Reihenfolge: das letzte vor der Fehlerstelle suchen.
            var cleaned = XDocument.Parse(text, LoadOptions.SetLineInfo | LoadOptions.PreserveWhitespace);
            var index = cleaned.Descendants().Cast<IXmlLineInfo>().TakeWhile(i =>
                i.LineNumber < e.LineNumber || (i.LineNumber == e.LineNumber && i.LinePosition <= Math.Max(e.LinePosition, 2))).Count() - 1;
            var el = original.Descendants().ElementAtOrDefault(Math.Max(index, 0));
            if (el == null) return reason;
            return $"Fehler in <{el.Name.LocalName}> (Zeile {((IXmlLineInfo)el).LineNumber}): {reason}";
        }
        catch
        {
            return reason;
        }
    }
}

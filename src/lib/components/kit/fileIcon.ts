// Datei-Icon und Farbe nach Name/Endung (Baum, Tabs, QuickOpen, Verlauf).
import FileIcon from "@lucide/svelte/icons/file";
import FileCodeIcon from "@lucide/svelte/icons/file-code";
import FileBracesIcon from "@lucide/svelte/icons/file-braces";
import FileCogIcon from "@lucide/svelte/icons/file-cog";
import FileTextIcon from "@lucide/svelte/icons/file-text";
import FileImageIcon from "@lucide/svelte/icons/file-image";
import FileTerminalIcon from "@lucide/svelte/icons/file-terminal";

export type FileIconInfo = { icon: typeof FileIcon; tint: string };
const MAP: [RegExp, typeof FileIcon, string][] = [
  [/\.(m|c)?(t|j)sx?$/, FileCodeIcon, "text-(--syn-type)"],
  [/\.(svelte|vue|html?|astro|razor|cshtml)$/, FileCodeIcon, "text-(--syn-const)"],
  [/\.(css|scss|sass|less|uss)$/, FileCodeIcon, "text-(--syn-regexp)"],
  [/\.(rs|cs|go|py|c|h|cpp|hpp|java|kt|swift|rb|php|lua)$/, FileCodeIcon, "text-(--syn-meta)"],
  [/\.(json5?|jsonc|lock)$/, FileBracesIcon, "text-(--syn-number)"],
  [/\.(toml|ya?ml|ini|xml|csproj|props|config)$|^\.(env|editorconfig|gitignore)/, FileCogIcon, "text-(--syn-number)"],
  [/\.(md|mdx|txt|rst)$/, FileTextIcon, "text-(--syn-prop)"],
  [/\.(png|jpe?g|gif|svg|webp|ico|bmp)$/, FileImageIcon, "text-(--syn-string)"],
  [/\.(sh|bash|zsh|ps1|bat|cmd)$/, FileTerminalIcon, "text-(--syn-fn)"],
];
export function fileIcon(path: string): FileIconInfo {
  const name = path.slice(Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\")) + 1).toLowerCase();
  const hit = MAP.find(([re]) => re.test(name));
  return hit ? { icon: hit[1], tint: hit[2] } : { icon: FileIcon, tint: "text-muted-foreground" };
}

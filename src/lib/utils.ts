import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export type WithoutChild<T> = T extends { child?: unknown } ? Omit<T, "child"> : T;
export type WithoutChildren<T> = T extends { children?: unknown } ? Omit<T, "children"> : T;
export type WithoutChildrenOrChild<T> = WithoutChildren<WithoutChild<T>>;
export type WithElementRef<T, U extends HTMLElement = HTMLElement> = T & {
  ref?: U | null;
};

const AGE_STEPS: [number, string][] = [
  [31536e6, "J"],
  [2592e6, "Mo"],
  [864e5, "T"],
  [36e5, "h"],
  [6e4, "min"],
];

/** Kurzes Alter eines ISO-Zeitpunkts: "3 T", "2 Mo"; leer ohne Datum. */
export function age(iso: string) {
  if (!iso) return "";
  const diff = Date.now() - Date.parse(iso);
  for (const [ms, unit] of AGE_STEPS) if (diff >= ms) return `${Math.floor(diff / ms)} ${unit}`;
  return "jetzt";
}

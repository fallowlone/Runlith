import { useCallback, useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { inTauri } from "./api";

export type ThemePref = "dark" | "light" | "system";
export type Density = 32 | 40 | 48;
export type TextSize = 12 | 14 | 16;

export interface Settings {
  theme: ThemePref;
  density: Density;
  textSize: TextSize;
  reduceMotion: boolean;
  agentOpen: boolean;
}

const KEY = "runlith.settings.v1";
const DEFAULTS: Settings = { theme: "dark", density: 40, textSize: 14, reduceMotion: false, agentOpen: true };

/** Per-viewer conveniences only; a broken or blocked store falls back to defaults. */
function read(): Settings {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const value = JSON.parse(raw) as Partial<Settings>;
    return {
      theme: value.theme === "light" || value.theme === "system" ? value.theme : "dark",
      density: value.density === 32 || value.density === 48 ? value.density : 40,
      textSize: value.textSize === 12 || value.textSize === 16 ? value.textSize : 14,
      reduceMotion: value.reduceMotion === true,
      agentOpen: value.agentOpen !== false,
    };
  } catch {
    return DEFAULTS;
  }
}

function write(settings: Settings): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(settings));
  } catch {
    // Storage unavailable (private mode, blocked): settings stay for this session only.
  }
}

function systemTheme(): "dark" | "light" {
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function useSettings(): [Settings, (patch: Partial<Settings>) => void, "dark" | "light"] {
  const [settings, setSettings] = useState<Settings>(read);
  const [system, setSystem] = useState<"dark" | "light">(systemTheme);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (): void => setSystem(query.matches ? "dark" : "light");
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  // Native vibrancy follows the window appearance, so keep it in step with the app theme.
  useEffect(() => {
    if (!inTauri) return;
    getCurrentWindow()
      .setTheme(settings.theme === "system" ? null : settings.theme)
      .catch(() => undefined); // cosmetic: vibrancy then follows the system appearance
  }, [settings.theme]);

  const update = useCallback((patch: Partial<Settings>) => {
    setSettings((previous) => {
      const next = { ...previous, ...patch };
      write(next);
      return next;
    });
  }, []);

  const resolved = settings.theme === "system" ? system : settings.theme;
  return [settings, update, resolved];
}

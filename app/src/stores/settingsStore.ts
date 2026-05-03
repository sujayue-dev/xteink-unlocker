import { create } from "zustand";

const STORAGE_KEY = "xteink-unlocker-settings";

type SettingsState = {
  showCustomFirmwareOption: boolean;
  setShowCustomFirmwareOption: (value: boolean) => void;
};

function loadSettings(): Pick<SettingsState, "showCustomFirmwareOption"> {
  if (typeof window === "undefined") {
    return { showCustomFirmwareOption: false };
  }

  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return { showCustomFirmwareOption: false };
    const parsed = JSON.parse(raw) as Partial<SettingsState>;
    return {
      showCustomFirmwareOption: parsed.showCustomFirmwareOption === true,
    };
  } catch {
    return { showCustomFirmwareOption: false };
  }
}

function saveSettings(showCustomFirmwareOption: boolean) {
  window.localStorage.setItem(
    STORAGE_KEY,
    JSON.stringify({ showCustomFirmwareOption }),
  );
}

export const useSettingsStore = create<SettingsState>((set) => ({
  ...loadSettings(),
  setShowCustomFirmwareOption: (showCustomFirmwareOption) => {
    saveSettings(showCustomFirmwareOption);
    set({ showCustomFirmwareOption });
  },
}));

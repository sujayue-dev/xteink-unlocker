import { useEffect, useState } from "react";
import { Shell } from "./components/Shell";
import { Consent } from "./screens/Consent";
import { DeviceAndRegion } from "./screens/DeviceAndRegion";
import { Firmware } from "./screens/Firmware";
import { Connect } from "./screens/Connect";
import { Done, Failed, Live, Verify } from "./screens/Live";
import { useStateMachine } from "./store";
import { api } from "./api";
import type { Locale, Model } from "./types";

export function App() {
  const { state, error } = useStateMachine();
  const [model, setModel] = useState<Model | null>(null);
  const [locale, setLocale] = useState<Locale | null>(null);

  useEffect(() => {
    if (
      state === "selecting_firmware" ||
      state === "downloading_firmware" ||
      state === "setting_up_hotspot" ||
      state === "awaiting_client" ||
      state === "awaiting_device_request"
    ) {
      api.getSession().then((s) => {
        if (s.model) setModel(s.model);
        if (s.locale) setLocale(s.locale);
      });
    }
  }, [state]);

  return (
    <Shell state={state}>
      {(state === "idle" || state === "consenting") && <Consent />}
      {state === "selecting_device_and_region" && <DeviceAndRegion />}
      {state === "selecting_firmware" && model && locale && (
        <Firmware model={model} locale={locale} />
      )}
      {(state === "downloading_firmware" ||
        state === "setting_up_hotspot" ||
        state === "awaiting_client" ||
        state === "awaiting_device_request") && <Connect state={state} />}
      {(state === "armed" || state === "serving" || state === "flashing") && (
        <Live state={state} />
      )}
      {state === "verifying" && <Verify />}
      {state === "done" && <Done />}
      {state === "failed" && <Failed error={error} />}
      {state === "cleaning_up" && (
        <p className="text-sm text-stone-500">Reverting changes…</p>
      )}
    </Shell>
  );
}

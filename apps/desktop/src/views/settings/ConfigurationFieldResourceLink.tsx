import { useEffect, useRef, useState, type ReactNode } from "react";
import { openExternalUrl } from "../../api";
import { ActivityNotice } from "../../components/ActivityNotice";
import { ShellIcon } from "../../components/ShellIcon";
import type { TranslateFn } from "../../i18n";

export function ConfigurationFieldResourceLink(props: {
  children: ReactNode;
  id: string;
  t?: TranslateFn;
  url: string;
}) {
  const [error, setError] = useState("");
  const opening = useRef(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);

  async function openResource() {
    if (opening.current) return;
    opening.current = true;
    setError("");
    try {
      await openExternalUrl(props.url);
    } catch (cause) {
      if (mounted.current) {
        const message = cause instanceof Error ? cause.message : String(cause);
        setError(props.t?.("settings.resourceLink.openFailed", { message },
          "Could not open the official page: {message}") ?? `Could not open the official page: ${message}`);
      }
    } finally {
      opening.current = false;
    }
  }

  return <>
    <a id={props.id} className="detail-label settings-field-label configuration-field-resource-link"
      href={props.url} target="_blank" rel="noopener noreferrer"
      aria-description={props.t?.("settings.resourceLink.openOfficial", undefined,
        "Open the official page in your browser") ?? "Open the official page in your browser"}
      onClick={(event) => { event.preventDefault(); void openResource(); }}
      onAuxClick={(event) => {
        if (event.button === 1) { event.preventDefault(); void openResource(); }
      }}>
      <span>{props.children}</span><ShellIcon name="external-link" />
    </a>
    {error ? <ActivityNotice tone="error" onDismiss={() => setError("")}>{error}</ActivityNotice> : null}
  </>;
}

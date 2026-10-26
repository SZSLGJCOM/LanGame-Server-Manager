import { useEffect, useState } from "react";
import { findArkMapArtwork, type ArkMapArtwork } from "../../ark-map-artwork-catalog";
import { useI18n } from "../../i18n";
import { loadMediaImage } from "../../load-media-image";

interface State { key: string; artwork: ArkMapArtwork | null; loading: boolean }
export function useArkMapArtwork(edition: string, mapName: string, visible: boolean) {
  const { locale } = useI18n();
  const definition = findArkMapArtwork(edition, mapName);
  const key = `${edition}:${mapName.toLowerCase()}`;
  const [retry, setRetry] = useState(0);
  const [state, setState] = useState<State>({ key: "", artwork: null, loading: false });
  useEffect(() => {
    if (!definition || !visible) return;
    setState({ key, artwork: null, loading: true });
    // The shared media loader owns bounded cache/direct attempts and cancels
    // obsolete requests when the selected map changes or the panel is hidden.
    return loadMediaImage(definition.sources, image => {
      setState({ key, artwork: { ...definition, url: image.src }, loading: false });
    }, () => setState({ key, artwork: null, loading: false }), locale, {
      crossOrigin: null,
      canLoadDirectly: source => source === definition.url,
      acceptImage: image => image.naturalWidth === definition.width && image.naturalHeight === definition.height
    });
  }, [definition, key, visible, locale, retry]);
  const current = state.key === key ? state : null;
  const canRetry = Boolean(definition && visible && current && !current.artwork && !current.loading);
  return { artwork: current?.artwork ?? null, loading: Boolean(definition && visible && (current?.loading ?? true)),
    canRetry, refresh: () => { if (canRetry) setRetry(value => value + 1); } };
}

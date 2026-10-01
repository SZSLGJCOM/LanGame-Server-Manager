import { useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { searchSteamWorkshopItems } from "../../api";
import type { SteamWorkshopBrowseKind } from "../../types";
import { WorkshopBrowseController, workshopBrowseKey, type WorkshopBrowseState } from "./workshop-browse-controller";

export function useSteamWorkshopBrowse(appId: number | null, query: string, sort: string, page: number, browseKind: SteamWorkshopBrowseKind = "item") {
  const { locale } = useI18n();
  const controller = useRef<WorkshopBrowseController | null>(null);
  const previousQuery = useRef(query);
  const previousRevision = useRef(0);
  const [revision, setRevision] = useState(0);
  const [state, setState] = useState<WorkshopBrowseState>({ key: "", result: null, loading: false, error: null });
  useEffect(() => {
    const owned = new WorkshopBrowseController(
      (request) => searchSteamWorkshopItems(request.appId, request.query, request.sort, request.page, request.locale, request.browseKind),
      setState
    );
    controller.current = owned;
    return () => { owned.dispose(); controller.current = null; };
  }, []);
  useEffect(() => {
    if (appId === null) return;
    const delay = previousQuery.current === query ? 0 : 350;
    previousQuery.current = query;
    controller.current?.request({ appId, query, sort, page, locale, browseKind }, delay, revision !== previousRevision.current);
    previousRevision.current = revision;
  }, [appId, query, sort, page, locale, browseKind, revision]);
  const key = appId === null ? "" : workshopBrowseKey({ appId, query, sort, page, locale, browseKind });
  return {
    result: appId === null || state.locale !== locale || state.result?.app_id !== appId || state.result.browse_kind !== browseKind ? null : state.result,
    loading: appId !== null && (key !== state.key || state.loading),
    error: key === state.key ? state.error : null,
    retry: () => setRevision((value) => value + 1)
  };
}

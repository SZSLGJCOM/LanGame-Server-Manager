import { startTransition, type DragEvent, type PointerEvent, useCallback, useDeferredValue, useEffect, useMemo, useRef, useState } from "react";
import {
  downloadSteamWorkshopItems,
  installManualModReferences,
  lookupSteamWorkshopItems,
  readSteamWorkshopItemDetails,
  openExternalUrl,
  openLocalPath,
  readInstanceDetails,
  readBackgroundJobs,
  readManualModInventory,
  readProjectZomboidWorkshopModsSnapshot,
  readSteamWorkshopInstallationStatus,
  resolveManualModReferences,
  stageManualModFiles
} from "../../api";
import { describeError, isActiveJobStatus } from "../../app-state";
import { formatDesktopError } from "../../desktop-error-message";
import { DesktopFileDropError, droppedFilePaths, modDropEffect } from "../../desktop-file-drop";
import type { InstanceArchiveDetails } from "../../storage-management-types";
import { ShellIcon } from "../../components/ShellIcon";
import { ActivityNotice } from "../../components/ActivityNotice";
import { SingleFlightPoller } from "../../domain/single-flight-poller";
import { useI18n, type LocaleCode } from "../../i18n";
import type {
  InstanceDetails,
  LaunchPlan,
  ManualModInventoryItem,
  ManualModInventoryResult,
  ManualModReferenceResolveResult,
  ManualModStageResult,
  ThunderstoreVersionPolicy,
  ModuleDetails,
  ProjectZomboidWorkshopModsSnapshot,
  SaveInstanceSettingsOptions,
  SteamWorkshopBrowseKind,
  SteamWorkshopDownloadResult,
  SteamWorkshopInstallationSnapshot,
  SteamWorkshopLookupItem,
  UpdateInstanceInput,
  BackgroundJob
} from "../../types";
import { parseSettingsObject, parseWorkshopIdList } from "../settings/guided-settings";
import { ConfigurationHelp } from "../settings/ConfigurationFieldHelp";
import type { SettingsObject } from "../settings/settings-schema";
import { useModWorkbenchMutations } from "./useModWorkbenchMutations";
import type { SteamWorkshopAccountState } from "./useSteamWorkshopAccount";
import { requiresSteamWorkshopAccount } from "./steam-workshop-access-error";
import { DST_RAW_MOD_WARNING, hasDstRawModOverrides } from "./mod-workbench-dst-policy";
import { SteamWorkshopPreview, SteamWorkshopStoreDetail, type WorkshopStoreAction } from "./SteamWorkshopStoreDetail";
import { useSteamWorkshopBrowse } from "./useSteamWorkshopBrowse";
import { composeWorkshopLookups, rememberWorkshopPresentation, type WorkshopPresentationCache } from "./workshop-presentation";
import { WorkshopDetailsRequests } from "./workshop-details-requests";
import { buildWorkshopVerificationWarning } from "./workshop-warning";
import { DstWorkshopConfiguration } from "./DstWorkshopConfiguration";
import { ProjectZomboidMapOrderEditor, validateProjectZomboidMapOrder } from "./ProjectZomboidMapOrderEditor";
import { WorkshopManifestPanel } from "./WorkshopManifestPanel";
import { AsaModManifestPanel } from "./AsaModManifestPanel";
import { ArkModMapConfiguration } from "./ArkModMapConfiguration";
import { canUseNexusApi, readNexusAccountStatus, reviewNexusModReferences, installNexusModFiles, NexusApiError,
  type NexusAccountStatus, type NexusModReview } from "../../nexus-mods-api";
import { NexusCredentialsDialog } from "./NexusCredentialsDialog";
import { NexusFileSelectionDialog } from "./NexusFileSelectionDialog";
import { nexusErrorText, nexusText } from "./nexus-mods-messages";
import { nexusReferencesFromText } from "./nexus-mod-reference";
import { parseEvolvedModIds } from "./ark-evolved-mod-ids";
import { arkModFields, arkModCompatibilityDiffers, arkModUnionSettings, arkModEnabledCounts, applyArkModChange, applyArkSharedModAction,
  assertArkModChangeBase, readArkModMapOverrides, ArkModScopeError, type ArkModChange } from "./ark-map-mods";
import { WorkshopCollectionLibrary } from "./WorkshopCollectionLibrary";
import { WorkshopCollectionRemovalDialog } from "./WorkshopCollectionRemovalDialog";
import { useWorkshopCollectionRemoval } from "./useWorkshopCollectionRemoval";
import {
  buildWorkshopModEnablementPlan, buildWorkshopDownloadOwnershipPlan,
  buildProjectZomboidWorkshopRemovalPlan, readWorkshopControlStates, supportsWorkshopModEnablement, WorkshopControlError
} from "./mod-workbench-workshop-controls";
import {
  buildPalworldWorkshopPlan, inventoryWorkshopIds, isRemovedWorkshopInventoryItem, palworldWorkshopStates,
  restoreRemovedWorkshopIds, workshopInventoryItems, WorkshopInventoryControlError
} from "./mod-workbench-workshop-inventory";
import {
  readManagedWorkshopCollections, collectInstalledWorkshopCollections,
  mergeManagedWorkshopCollections, managedCollectionMemberIds, type ManagedWorkshopCollection
} from "./mod-workbench-collections";
import { ManualModInventoryList } from "./ManualModInventoryList";
import { ManagedModDeleteDialog } from "./ManagedModDeleteDialog";
import { useManagedManualMods } from "./useManagedManualMods";
import { WorkshopStatus } from "./WorkshopStatus";
import type { WorkshopManifestReview } from "./workshop-manifest";
import { changedSettingKeys, mergeSettingPatch } from "./mod-settings-patch";
import { ASA_MEMBERSHIP_KEYS, ASA_RAW_MOD_WARNING, AsaModControlError, canonicalAsaModId,
  buildAsaActiveListPlan, hasAsaRawModFlags, parseAsaModIds, readAsaModMembership, type AsaModAction } from "./mod-workbench-asa";
import { referenceCandidatesFromText } from "./mod-reference-parser";
import { MOD_WORKFLOW_CATALOG, modWorkbenchAvailability, type ModWorkflowCatalogEntry } from "./mod-workbench-capability";
import {
  buildConfiguredEntries,
  buildConfigurableEntries,
  buildDownloadableSteamIds,
  buildEnabledRows,
  buildSteamLookupIds,
  canReorderEnabledRow,
  canToggleModEnabledRow,
  expandWorkshopItemIds,
  formatBrowseSortLabel,
  formatCompactCount,
  instanceBlocksModChanges,
  isUnsupportedWorkshopItem,
  isClientOnlyDstWorkshopItem,
  isIncompleteWorkshopCollection,
  mergeTextList,
  mergeWorkshopList,
  parseDelimitedEntries,
  parseLineOrSemicolonEntries,
  readDstRemovedWorkshopModIds,
  removeTextListValues,
  reorderEnabledEntryValues,
  steamItemBelongsToApp,
  uniqueEntries,
  workflowProviderFromModuleDetails,
  type ModEnabledRow,
  type ModSourceEntry,
  type SteamWorkshopBrowseSort
} from "./mod-workbench-model";
import {
  buildModSettingsApplyPlan,
  buildModSettingsRemovePlan,
  buildDstModEnablementPlan,
  buildProjectZomboidEnablePlan,
  collectProjectZomboidLocalIds
} from "./mod-workbench-plans";
import { buildSteamWorkshopEnablementPlan } from "./steam-workshop-enablement-plan";
import {
  resolveWorkshopStoreItemState as resolveStoreItemState,
  formatWorkshopByteSize,
  workshopItemContentIds,
  type WorkshopStoreItemState
} from "./steam-workshop-store-model";

type ModWorkbenchView = "store" | "config";

const MOD_WORKBENCH_TABS = [
  { id: "store", labelKey: "servers.mods.mainTabs.store", fallback: "Store" },
  { id: "config", labelKey: "servers.mods.mainTabs.config", fallback: "My Mods" }
] as const satisfies ReadonlyArray<{
  id: ModWorkbenchView;
  labelKey: string;
  fallback: string;
}>;

const EMPTY_WORKSHOP_ITEMS: SteamWorkshopLookupItem[] = [];
const EMPTY_MANUAL_INVENTORY_ITEMS: ManualModInventoryItem[] = [];

export interface ModWorkbenchProps {
  steamAccount?: SteamWorkshopAccountState;
  onSteamAccountRequired?: (retry: (accountName: string) => Promise<SteamWorkshopDownloadResult>) => Promise<SteamWorkshopDownloadResult>;
  details: InstanceDetails;
  archive?: InstanceArchiveDetails;
  moduleDetails: ModuleDetails | null;
  jobs?: BackgroundJob[];
  launchPlan: LaunchPlan | null;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

function referencesFromDomDrop(event: DragEvent<HTMLElement>, parser = referenceCandidatesFromText): string[] {
  // These are alternative representations of the same selection. Combining
  // them can turn an anchor's thumbnail URL into a second Mod installation.
  for (const type of ["text/uri-list", "text/plain"]) {
    const references = parser(event.dataTransfer.getData(type));
    if (references.length) return references;
  }
  const html = event.dataTransfer.getData("text/html");
  const hrefs = Array.from(html.matchAll(/href=["']([^"']+)["']/gi), (match) => match[1]);
  return parser(hrefs.length ? hrefs.join("\n") : html);
}

export function ModWorkbench(props: ModWorkbenchProps) {
  const { locale, t } = useI18n();
  const moduleId = props.details.summary.module_id;
  const readOnly = Boolean(props.archive);
  const modChangesBlocked = readOnly || instanceBlocksModChanges(props.details);
  const instanceSettings = useMemo(
    () => parseSettingsObject(props.details.settings_json, t).value ?? {},
    [props.details.settings_json, t]
  );
  const isArk = arkModFields(moduleId).length > 0;
  const arkProjection = useMemo(() => {
    try { return { settings: arkModUnionSettings(instanceSettings, moduleId),
      differs: isArk && arkModCompatibilityDiffers(instanceSettings, moduleId), error: false }; }
    catch { return { settings: instanceSettings, differs: false, error: true }; }
  }, [instanceSettings, moduleId, isArk]);
  const settings = instanceSettings;
  const arkMapCounts = useMemo(() => isArk && !arkProjection.error ? arkModEnabledCounts(settings, moduleId)
    : { counts: new Map<string, number>(), total: 0 }, [isArk, arkProjection.error, settings, moduleId]);
  const workflow = MOD_WORKFLOW_CATALOG[moduleId] ?? {
    moduleId,
    provider: workflowProviderFromModuleDetails(props.moduleDetails),
    sourceLabel: props.moduleDetails?.workshop?.provider === "steam"
      ? "Steam Workshop"
      : props.moduleDetails?.mods?.source?.label ?? "Mod source",
    primaryUrl: props.moduleDetails?.mods?.source?.url,
    steamDownloadMode: props.moduleDetails?.workshop?.provider === "steam" ? "steamcmd-cache" : "not-wired"
  } satisfies ModWorkflowCatalogEntry;
  const manualStaging = props.moduleDetails?.mods?.manual_staging ?? null;
  const manualEnablement = props.moduleDetails?.mods?.enablement ?? null;
  // The Mods tab is disabled for these modules; the guard keeps every mutation path closed if mounted directly.
  const modWorkflowAvailability = modWorkbenchAvailability(moduleId, props.moduleDetails);
  const modWorkflowUnsupported = modWorkflowAvailability === "client_only" || modWorkflowAvailability === "unsupported";
  const manualSourceProvider = props.moduleDetails?.mods?.source?.provider?.trim().toLowerCase() ?? "";
  const manualSourceLabel = props.moduleDetails?.mods?.source?.label ?? workflow.sourceLabel;
  const manualSourceUrl = props.moduleDetails?.mods?.source?.url ?? workflow.primaryUrl ?? null;
  const nexusSource = manualSourceProvider === "nexus", nexusLocal = canUseNexusApi();
  const canInstallSiteReferences = Boolean(
    manualStaging &&
    !manualEnablement &&
    (manualSourceProvider === "thunderstore" || (nexusSource && nexusLocal))
  );
  const canUseManualReferences = Boolean(manualEnablement || canInstallSiteReferences);
  const managedCollections = useMemo(() => readManagedWorkshopCollections(settings, moduleId), [settings, moduleId]);
  const collectionMemberIds = useMemo(() => uniqueEntries(managedCollections.flatMap((collection) => collection.member_ids)), [managedCollections]);
  const managedCollectionIds = useMemo(() => new Set(managedCollections.map((entry) => entry.id)), [managedCollections]);
  const configuredEntries = useMemo(
    () => buildConfiguredEntries(moduleId, isArk ? arkProjection.settings : settings, props.moduleDetails, collectionMemberIds),
    [moduleId, props.moduleDetails, settings, collectionMemberIds, isArk, arkProjection.settings]
  );
  const configurableEntries = useMemo(
    () => buildConfigurableEntries(moduleId, configuredEntries),
    [configuredEntries, moduleId]
  );
  const enabledRows = useMemo(
    () => buildEnabledRows(configurableEntries),
    [configurableEntries]
  );
  const dstRowsById = useMemo(() => new Map(moduleId === "dontstarve" ? enabledRows.map((row) => [row.id, row] as const) : []), [enabledRows, moduleId]);
  const [browseQuery, setBrowseQuery] = useState("");
  const [browseSort, setBrowseSort] = useState<SteamWorkshopBrowseSort>("trend");
  const [browseKind, setBrowseKind] = useState<SteamWorkshopBrowseKind>("item");
  const [browsePage, setBrowsePage] = useState(1);
  const [selectedBrowseWorkshopId, setSelectedBrowseWorkshopId] = useState<string | null>(null);
  const [detailPreview, setDetailPreview] = useState<{ locale: LocaleCode; item: SteamWorkshopLookupItem } | null>(null);
  const [detailHistory, setDetailHistory] = useState<string[]>([]);
  const [installingWorkshopIds, setInstallingWorkshopIds] = useState<Set<string>>(() => new Set());
  const [retainedWorkshopIds, setRetainedWorkshopIds] = useState<string[]>([]);
  const [lookupCache, setLookupCache] = useState<Partial<Record<LocaleCode, Record<string, SteamWorkshopLookupItem>>>>({});
  const [presentationCache, setPresentationCache] = useState<Partial<Record<LocaleCode, WorkshopPresentationCache>>>({});
  const detailsRequests = useRef(new WorkshopDetailsRequests(readSteamWorkshopItemDetails));
  const lookupSequence = useRef(0);
  const itemLookupVersions = useRef(new Map<string, number>());
  const resolvedLookupMap = useMemo(() => lookupCache[locale] ?? {}, [lookupCache, locale]);
  const presentations = useMemo(() => presentationCache[locale] ?? {}, [presentationCache, locale]);
  // A late request may refresh its own language, but cannot replace the visible one.
  const setResolvedLookupMap = useCallback((update: (current: Record<string, SteamWorkshopLookupItem>) => Record<string, SteamWorkshopLookupItem>) => {
    setLookupCache((current) => ({ ...current, [locale]: update(current[locale] ?? {}) }));
  }, [locale]);
  const beginLookup = useCallback((ids: string[]) => {
    const version = ++lookupSequence.current;
    for (const id of ids) itemLookupVersions.current.set(`${locale}:${id}`, version);
    return version;
  }, [locale]);
  const [lookupError, setLookupError] = useState<string | null>(null);
  const [lookupLoading, setLookupLoading] = useState(false);
  const [lookupRevision, setLookupRevision] = useState(0);
  const [pzSnapshot, setPzSnapshot] = useState<ProjectZomboidWorkshopModsSnapshot | null>(null);
  const [pzError, setPzError] = useState<string | null>(null);
  const [workshopInstallationResult, setWorkshopInstallationResult] = useState<SteamWorkshopInstallationSnapshot | null>(null);
  const [workshopInstallationError, setWorkshopInstallationError] = useState<string | null>(null);
  const [workshopInstallationLoading, setWorkshopInstallationLoading] = useState(false);
  const [downloadState, setDownloadState] = useState<"idle" | "running" | "success" | "error">("idle");
  const [downloadResult, setDownloadResult] = useState<SteamWorkshopDownloadResult | null>(null);
  const [downloadError, setDownloadError] = useState<string | null>(null);
  const [applyMessage, setApplyMessage] = useState<string | null>(null);
  const [scanNonce, setScanNonce] = useState(0);
  const [activeWorkbenchView, setActiveWorkbenchView] = useState<ModWorkbenchView>(readOnly ? "config" : "store");
  const [manifestMode, setManifestMode] = useState(false);
  const [manifestDraft, setManifestDraft] = useState<string | null>(null);
  const [selectedCollectionId, setSelectedCollectionId] = useState<string | null>(null);
  const [selectedCollectionMember, setSelectedCollectionMember] = useState<{ collectionId: string; modId: string } | null>(null);
  const [selectedEnabledRowKey, setSelectedEnabledRowKey] = useState<string | null>(null);
  const [selectedArkModId, setSelectedArkModId] = useState<string | null>(null);
  const [selectedInventoryPath, setSelectedInventoryPath] = useState<string | null>(null);
  const [activeDetailSource, setActiveDetailSource] = useState<"enabled" | "inventory">("enabled");
  const [draggedEnabledRow, setDraggedEnabledRow] = useState<ModEnabledRow | null>(null);
  const [enabledDropTargetKey, setEnabledDropTargetKey] = useState<string | null>(null);
  const manualDropRequest = useRef<AbortController | null>(null);
  const browsePaneRef = useRef<HTMLDivElement | null>(null);
  const [manualDropActive, setManualDropActive] = useState(false);
  const [manualDropReferenceActive, setManualDropReferenceActive] = useState(false);
  const [manualStageState, setManualStageState] = useState<"idle" | "running" | "success" | "error">("idle");
  const [manualStageResult, setManualStageResult] = useState<ManualModStageResult | null>(null);
  const [manualStageError, setManualStageError] = useState<string | null>(null);
  const [manualInventory, setManualInventory] = useState<ManualModInventoryResult | null>(null);
  const [manualInventoryError, setManualInventoryError] = useState<string | null>(null);
  const workshopControlStates = useMemo(() => moduleId === "palworld" ? palworldWorkshopStates(settings, manualInventory)
    : readWorkshopControlStates(moduleId, settings, pzSnapshot), [moduleId, settings, manualInventory, pzSnapshot]);
  const asaMembership = useMemo(() => readAsaModMembership(moduleId === "arksurvivalascended" ? arkProjection.settings : {}), [moduleId, arkProjection.settings]);
  const sharedAsaMembership = useMemo(() => readAsaModMembership(moduleId === "arksurvivalascended" ? settings : {}), [moduleId, settings]);
  const [manualReferenceInput, setManualReferenceInput] = useState("");
  const [thunderstoreVersionPolicy, setThunderstoreVersionPolicy] = useState<ThunderstoreVersionPolicy>("follow_latest");
  useEffect(() => { setThunderstoreVersionPolicy("follow_latest"); }, [props.details.summary.id]);
  const [manualReferenceState, setManualReferenceState] = useState<"idle" | "running" | "success" | "error">("idle");
  const [manualReferenceResult, setManualReferenceResult] = useState<ManualModReferenceResolveResult | null>(null);
  const [manualReferenceError, setManualReferenceError] = useState<string | null>(null);
  const [nexusAccount, setNexusAccount] = useState<NexusAccountStatus | null>(null);
  const [nexusCredentialsOpen, setNexusCredentialsOpen] = useState(false);
  const [nexusReview, setNexusReview] = useState<{ references: string[]; reviews: NexusModReview[]; input: string } | null>(null);
  const [nexusInstallError, setNexusInstallError] = useState<string | null>(null);
  const nexusGeneration = useRef(0);
  useEffect(() => {
    const generation = ++nexusGeneration.current;
    setNexusAccount(null);
    setNexusCredentialsOpen(false); setNexusReview(null); setNexusInstallError(null);
    if (nexusSource && nexusLocal) void readNexusAccountStatus().then((status) => {
      if (nexusGeneration.current === generation) setNexusAccount(status);
    }).catch((failure) => {
      if (nexusGeneration.current === generation) setManualReferenceError(nexusErrorText(locale, failure));
    });
    return () => { nexusGeneration.current++; };
  }, [nexusSource, nexusLocal, props.details.summary.id]);
  const modChangesBlockedRef = useRef(modChangesBlocked);
  const latestSettingsRef = useRef(settings);
  const { mutationBusy, mutationError, setMutationError, runModMutation, launchModMutation } = useModWorkbenchMutations(
    props.details.summary.id, moduleId, modChangesBlockedRef, latestSettingsRef, t, props.steamAccount?.isBusy
  );
  const steamAccount = props.steamAccount;
  const managedManualMods = useManagedManualMods({
    instanceId: props.details.summary.id,
    runOperation: (operation) => runModMutation("staging", operation),
    onResult: (result) => {
      setManualInventory(result.inventory);
      setManualInventoryError(null);
      setScanNonce((current) => current + 1);
    }
  });
  const collectionRemoval = useWorkshopCollectionRemoval({
    instanceId: props.details.summary.id, moduleId,
    readState: readWritableInstanceState,
    runOperation: (operation) => runModMutation("configuration", operation),
    save: persistSettings,
    onRemoved: (id) => {
      if (selectedCollectionId === id) setSelectedCollectionId(null);
      setSelectedCollectionMember((current) => current?.collectionId === id ? null : current);
      setScanNonce((current) => current + 1);
    }
  });
  const selectedManagedCollection = managedCollections.find((entry) => entry.id === selectedCollectionId) ?? managedCollections[0];
  const collectionLookupIds = activeWorkbenchView === "config" && browseKind === "collection" && selectedManagedCollection
    ? [selectedManagedCollection.id, ...selectedManagedCollection.member_ids] : [];
  const collectionLookupIdsKey = collectionLookupIds.join("\n");
  const workshopIds = useMemo(() => buildSteamLookupIds(configuredEntries, []), [configuredEntries]);
  const workshopIdsKey = workshopIds.join("\n");
  const workshopLookupIds = useMemo(
    () => uniqueEntries([...workshopIds, ...retainedWorkshopIds, ...collectionLookupIdsKey.split("\n").filter(Boolean)]),
    [retainedWorkshopIds, workshopIds, collectionLookupIdsKey]
  );
  const workshopLookupIdsKey = workshopLookupIds.join("\n");
  const deferredWorkshopLookupIdsKey = useDeferredValue(workshopLookupIdsKey);
  const expectedAppId = props.moduleDetails?.workshop?.provider === "steam"
    ? props.moduleDetails.workshop.consumer_app_id ?? null
    : null;
  const isSteamWorkshopModule = props.moduleDetails?.workshop?.provider === "steam";
  const { result: browseResult, error: browseError, loading: browseLoading, retry: retryBrowse } =
    useSteamWorkshopBrowse(readOnly ? null : expectedAppId, browseQuery, browseSort, browsePage, browseKind);
  // Keep full lookups separate so a browse summary cannot erase collection children.
  const lookupMap = useMemo(() => composeWorkshopLookups(browseResult?.items ?? [], resolvedLookupMap, presentations),
    [browseResult, resolvedLookupMap, presentations]);
  const isManualEnablementModule = Boolean(manualEnablement && (!isSteamWorkshopModule || manualEnablement.id_strategy === "palworld_package_name"));
  const usesFileModManagement = Boolean(manualStaging && !manualEnablement && !isSteamWorkshopModule
    && moduleId !== "astroneer" && moduleId !== "satisfactory");
  const showsManualInventory = !readOnly && Boolean(manualStaging && (!manualEnablement || isManualEnablementModule));
  const workshopCatalogIds = useMemo(
    () => expandWorkshopItemIds([
      ...(browseResult?.items ?? []).map((item) => lookupMap[item.id] ?? item),
      ...workshopLookupIds.map((id) => lookupMap[id]).filter((item): item is SteamWorkshopLookupItem => Boolean(item))
    ]),
    [browseResult, lookupMap, workshopLookupIds]
  );
  const workshopInspectionIds = useMemo(
    () => uniqueEntries([...workshopLookupIds, ...workshopCatalogIds]),
    [workshopCatalogIds, workshopLookupIds]
  );
  const workshopInspectionIdsKey = workshopInspectionIds.join("\n");
  const pzWorkshopIds = useMemo(
    () => uniqueEntries([
      ...configuredEntries.filter((entry) => entry.key === "pz-workshop").flatMap((entry) => entry.ids),
      ...workshopInspectionIds
    ]),
    [configuredEntries, workshopInspectionIds]
  );
  const pzWorkshopIdsKey = pzWorkshopIds.join("\n");
  const mismatchedItems = workshopIds
    .map((id) => lookupMap[id])
    .filter((item): item is SteamWorkshopLookupItem => Boolean(item))
    .filter((item) => !steamItemBelongsToApp(item, expectedAppId));
  const asaRawFlagsActive = moduleId === "arksurvivalascended" && hasAsaRawModFlags(settings);
  const modMutationsDisabled = modChangesBlocked || mutationBusy || Boolean(steamAccount?.busy) || asaRawFlagsActive
    || arkProjection.error;
  const dstRawOverridesActive = moduleId === "dontstarve" && hasDstRawModOverrides(settings);
  const modEnablementDisabled = modMutationsDisabled || dstRawOverridesActive;
  const [activeJob, setActiveJob] = useState<BackgroundJob | null>(null);

  useEffect(() => {
    if (readOnly || downloadState !== "running") {
      setActiveJob(null);
      return;
    }

    const instanceId = props.details.summary.id;
    setActiveJob(null);
    const poller = new SingleFlightPoller<BackgroundJob[], number>({
      intervalMs: 350,
      poll: readBackgroundJobs,
      schedule: (callback, delayMs) => window.setTimeout(callback, delayMs),
      cancel: (handle) => window.clearTimeout(handle),
      onValue: (jobs) => {
        const matched = jobs.find(
          (job) =>
            job.target_id === instanceId &&
            (job.kind.toLowerCase() === "downloadworkshop" || job.kind.toLowerCase() === "download_workshop") &&
            isActiveJobStatus(job.status)
        ) ?? null;
        setActiveJob(matched);
      }
    });
    poller.start();
    return () => {
      poller.dispose();
    };
  }, [readOnly, downloadState, props.details.summary.id]);

  const activeWorkshopJob = useMemo(() => {
    if (readOnly) return null;
    return activeJob ?? (props.jobs ?? []).find(
      (job) =>
        job.target_id === props.details.summary.id &&
        (job.kind.toLowerCase() === "downloadworkshop" || job.kind.toLowerCase() === "download_workshop") &&
        isActiveJobStatus(job.status)
    ) ?? null;
  }, [readOnly, activeJob, props.details.summary.id, props.jobs]);

  const activeProgressPercent = typeof activeWorkshopJob?.progress_percent === "number" && Number.isFinite(activeWorkshopJob.progress_percent)
    ? Math.round(activeWorkshopJob.progress_percent)
    : null;

  useEffect(() => {
    latestSettingsRef.current = settings;
  }, [settings]);

  useEffect(() => {
    modChangesBlockedRef.current = modChangesBlocked;
  }, [modChangesBlocked]);

  useEffect(() => {
    if (!applyMessage) {
      return;
    }
    const timer = window.setTimeout(() => {
      setApplyMessage(null);
    }, 4500);
    return () => window.clearTimeout(timer);
  }, [applyMessage]);


  useEffect(() => {
    setActiveWorkbenchView(readOnly ? "config" : "store");
    setManifestMode(false);
    setManifestDraft(null);
    setSelectedCollectionId(null);
    setSelectedCollectionMember(null);
    setSelectedBrowseWorkshopId(null);
    setDetailPreview(null);
    setDetailHistory([]);
    setLookupCache({});
    setPresentationCache({});
    detailsRequests.current.clear();
    itemLookupVersions.current.clear();
    setSelectedEnabledRowKey(null);
    setSelectedArkModId(null);
    setSelectedInventoryPath(null);
    setActiveDetailSource("enabled");
    setDraggedEnabledRow(null);
    setEnabledDropTargetKey(null);
    setRetainedWorkshopIds([]);
    setManualDropActive(false);
    setManualStageState("idle");
    setManualStageResult(null);
    setManualStageError(null);
    setManualReferenceInput("");
    setManualReferenceState("idle");
    setManualReferenceResult(null);
    setManualReferenceError(null);
    setWorkshopInstallationResult(null);
    setWorkshopInstallationError(null);
    setMutationError(null);
  }, [moduleId, readOnly]);

  useEffect(() => {
    const ids = workshopIdsKey.split("\n").map((entry) => entry.trim()).filter(Boolean);
    if (!isSteamWorkshopModule || ids.length === 0) {
      return;
    }
    setRetainedWorkshopIds((current) => {
      const next = uniqueEntries([...current, ...ids]);
      return next.length === current.length ? current : next;
    });
  }, [isSteamWorkshopModule, workshopIdsKey]);

  useEffect(() => {
    const ids = deferredWorkshopLookupIdsKey.split("\n").map((entry) => entry.trim()).filter(Boolean);
    if (readOnly || props.moduleDetails?.workshop?.provider !== "steam" || ids.length === 0) {
      startTransition(() => {
        setLookupLoading(false);
        setLookupError(null);
      });
      return;
    }

    let cancelled = false;
    const version = beginLookup(ids);
    setLookupLoading(true);
    lookupSteamWorkshopItems(ids, locale)
      .then((items) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setResolvedLookupMap((current) => {
            const next = { ...current };
            for (const item of items) {
              if (itemLookupVersions.current.get(`${locale}:${item.id}`) === version) next[item.id] = item;
            }
            return next;
          });
          setLookupError(null);
          setLookupLoading(false);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setLookupError(describeError(error));
          setLookupLoading(false);
        });
      });

    return () => {
      cancelled = true;
    };
  }, [readOnly, deferredWorkshopLookupIdsKey, props.moduleDetails?.workshop?.provider, lookupRevision, locale, setResolvedLookupMap, beginLookup]);

  useEffect(() => {
    if (!browseResult) return;
    if (browsePaneRef.current) browsePaneRef.current.scrollTop = 0;
  }, [browseResult]);

  const [detailFailure, setDetailFailure] = useState<{ id: string; locale: LocaleCode; message: string } | null>(null);
  const detailError = detailFailure?.id === selectedBrowseWorkshopId && detailFailure.locale === locale
    ? detailFailure.message : null;
  const [detailRevision, setDetailRevision] = useState(0);
  useEffect(() => {
    if (readOnly || !selectedBrowseWorkshopId) return;
    let cancelled = false;
    const version = ++lookupSequence.current;
    setDetailFailure(null);
    detailsRequests.current.read(selectedBrowseWorkshopId, locale).then(({ item, cached }) => {
      if (cancelled) return;
      if (!item || item.id !== selectedBrowseWorkshopId) {
        throw new Error(JSON.stringify({ code: "workshop-collection-install", reason: "unresolved",
          item_id: selectedBrowseWorkshopId, message: "Workshop details did not contain the requested item." }));
      }
      // Reused display content cannot overwrite newer game/type/client-only evidence.
      const lookupKey = `${locale}:${item.id}`;
      if (!cached && (itemLookupVersions.current.get(lookupKey) ?? 0) < version) {
        itemLookupVersions.current.set(lookupKey, version);
        setResolvedLookupMap((current) => itemLookupVersions.current.get(lookupKey) === version
          ? { ...current, [item.id]: item } : current);
      }
      setPresentationCache((current) => ({ ...current, [locale]: rememberWorkshopPresentation(current[locale] ?? {}, item) }));
    }).catch((error) => {
      if (cancelled) return;
      setDetailFailure({ id: selectedBrowseWorkshopId, locale, message: describeError(error) });
    });
    return () => { cancelled = true; };
  }, [readOnly, selectedBrowseWorkshopId, detailRevision, locale, setResolvedLookupMap]);

  useEffect(() => {
    const ids = workshopInspectionIdsKey.split("\n").map((entry) => entry.trim()).filter(Boolean);
    if (readOnly || !isSteamWorkshopModule) {
      startTransition(() => {
        setWorkshopInstallationResult(null);
        setWorkshopInstallationError(null);
        setWorkshopInstallationLoading(false);
      });
      return;
    }

    let cancelled = false;
    setWorkshopInstallationLoading(true);
    readSteamWorkshopInstallationStatus(props.details.summary.id, ids)
      .then((result) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setWorkshopInstallationResult(result);
          setWorkshopInstallationError(null);
          setWorkshopInstallationLoading(false);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setWorkshopInstallationError(describeError(error));
          setWorkshopInstallationLoading(false);
        });
      });

    return () => {
      cancelled = true;
    };
  }, [readOnly, isSteamWorkshopModule, props.details.summary.id, scanNonce, workshopInspectionIdsKey]);

  useEffect(() => {
    const ids = pzWorkshopIdsKey.split("\n").map((entry) => entry.trim()).filter(Boolean);
    if (readOnly || moduleId !== "projectzomboid" || ids.length === 0) {
      startTransition(() => {
        setPzSnapshot(null);
        setPzError(null);
      });
      return;
    }

    let cancelled = false;
    readProjectZomboidWorkshopModsSnapshot(props.details.summary.id, ids)
      .then((snapshot) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setPzSnapshot(snapshot);
          setPzError(null);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setPzError(describeError(error));
        });
      });

    return () => {
      cancelled = true;
    };
  }, [readOnly, moduleId, props.details.summary.id, pzWorkshopIdsKey, scanNonce]);

  useEffect(() => {
    if (readOnly || !manualStaging) {
      startTransition(() => {
        setManualInventory(null);
        setManualInventoryError(null);
      });
      return;
    }

    let cancelled = false;
    readManualModInventory(props.details.summary.id)
      .then((inventory) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setManualInventory(inventory);
          setManualInventoryError(null);
        });
      })
      .catch((error) => {
        if (cancelled) {
          return;
        }
        startTransition(() => {
          setManualInventory(null);
          setManualInventoryError(t(
            "servers.mods.inventoryFailed",
            { message: describeError(error) },
            "Could not read local Workshop cache: {message}"
          ));
        });
      });

    return () => {
      cancelled = true;
    };
  }, [readOnly, manualStaging, props.details.summary.id, scanNonce, t]);

  const handleStageManualModPaths = useCallback(async (paths: string[]) => {
    const cleanPaths = Array.from(new Set(paths.filter((path) => path.trim().length > 0)));
    if (readOnly || !manualStaging) {
      return;
    }
    if (cleanPaths.length === 0) {
      setManualStageState("error");
      setManualStageResult(null);
      setManualStageError(t("servers.mods.manualDropNoPaths", undefined, "Drop a downloaded mod file or folder from File Explorer."));
      return;
    }

    try {
      await runModMutation("staging", async () => {
        setManualStageState("running");
        setManualStageResult(null);
        setManualStageError(null);
        if (moduleId === "arksurvivalascended") {
          const latest = await readWritableInstanceState();
          assertAsaMembershipBase(latest.settings, latest.settings);
        }
        const result = await stageManualModFiles(props.details.summary.id, cleanPaths);
        if (moduleId === "arksurvivalascended") {
          const inventory = await readManualModInventory(props.details.summary.id);
          const roots = new Set(result.affected_root_names.map((name) => name.toLowerCase()));
          const ids = uniqueEntries(inventory.items.filter((item) => roots.has(item.name.toLowerCase()))
            .map((item) => canonicalAsaModId(item.inferred_id)).filter((id): id is string => id !== null));
          if (ids.length) await applyAsaModAction(ids, "restore-files");
        }
        startTransition(() => {
          setManualStageResult(result);
          setManualStageState("success");
          setScanNonce((current) => current + 1);
        });
      });
    } catch (error) {
      startTransition(() => {
        setManualStageError(t(
          "servers.mods.manualStageFailed",
          { message: describeError(error) },
          "Mod file installation failed: {message}"
        ));
        setManualStageState("error");
      });
    }
  }, [manualStaging, moduleId, props.details.summary.id, runModMutation, t, instanceSettings, arkProjection.error]);

  useEffect(() => () => manualDropRequest.current?.abort(), [props.details.summary.id, moduleId, activeWorkbenchView, manifestMode]);

  useEffect(() => {
    if (!manualDropActive) return;
    const clear = () => setManualDropActive(false);
    window.addEventListener("dragend", clear);
    window.addEventListener("drop", clear);
    window.addEventListener("blur", clear);
    return () => {
      window.removeEventListener("dragend", clear);
      window.removeEventListener("drop", clear);
      window.removeEventListener("blur", clear);
    };
  }, [manualDropActive]);

  async function handleManualFileDrop(transfer: DataTransfer) {
    manualDropRequest.current?.abort();
    const request = new AbortController();
    manualDropRequest.current = request;
    try {
      const paths = await droppedFilePaths(transfer, request.signal);
      if (!request.signal.aborted) await handleStageManualModPaths(paths);
    } catch (error) {
      if (request.signal.aborted) return;
      setManualStageError(t(error instanceof DesktopFileDropError && error.code === "too_many"
        ? "servers.mods.manualDropTooMany" : "servers.mods.manualDropUnavailable"));
      setManualStageState("error");
    } finally {
      if (manualDropRequest.current === request) manualDropRequest.current = null;
    }
  }

  async function readWritableInstanceState(): Promise<{
    details: InstanceDetails;
    settings: SettingsObject;
    instanceSettings: SettingsObject;
  }> {
    if (readOnly) throw new Error(t("servers.archives.workspace.restoreForMods", undefined,
      "Restore this instance to manage Mods."));
    if (modChangesBlockedRef.current) {
      throw new Error(t("servers.mods.stopBeforeChanges", undefined, "Stop the instance before changing Mods."));
    }
    if (isArk && arkProjection.error) {
      throw new Error(locale === "zh-CN" ? "地图 MOD 配置无效，请刷新后重试。" : "Map Mod settings are invalid. Reload and retry.");
    }
    const latestDetails = await readInstanceDetails(props.details.summary.id);
    if (instanceBlocksModChanges(latestDetails) || modChangesBlockedRef.current) {
      throw new Error(t("servers.mods.stopBeforeChanges", undefined, "Stop the instance before changing Mods."));
    }
    const latestParseResult = parseSettingsObject(latestDetails.settings_json, t);
    if (!latestParseResult.value) {
      throw new Error(latestParseResult.error || t(
        "servers.mods.configSaveFailed",
        undefined,
        "Unable to read instance settings."
      ));
    }
    if (isArk) {
      try { readArkModMapOverrides(latestParseResult.value, moduleId); }
      catch (failure) {
        if (!(failure instanceof ArkModScopeError)) throw failure;
        throw new Error(locale === "zh-CN" ? "MOD 地图配置无效，请检查已保存的配置。" : "Mod map configuration is invalid. Review the saved configuration.");
      }
    }
    return { details: latestDetails, instanceSettings: latestParseResult.value, settings: latestParseResult.value };
  }

  async function persistSettings(nextSettings: SettingsObject, expectedBase?: SettingsObject,
    removal?: SaveInstanceSettingsOptions["collectionRemoval"],
    validateCurrent?: (current: SettingsObject) => void): Promise<SettingsObject> {
    if (readOnly || !props.onSaveSettings) throw new Error(t("servers.archives.workspace.restoreForMods", undefined,
      "Restore this instance to manage Mods."));
    const intendedBase = expectedBase ?? latestSettingsRef.current;
    const dirtyKeys = changedSettingKeys(
      intendedBase,
      nextSettings,
      Array.from(new Set([...Object.keys(intendedBase), ...Object.keys(nextSettings)]))
    );
    if (dirtyKeys.length === 0 && !removal?.retainCollection) {
      return intendedBase;
    }
    if (moduleId === "projectzomboid" && dirtyKeys.includes("map_name")) {
      const validation = validateProjectZomboidMapOrder(String(nextSettings.map_name ?? ""), locale, t);
      if (validation) throw new Error(validation);
    }

    const latestState = await readWritableInstanceState();
    validateCurrent?.(latestState.settings);
    if (expectedBase && changedSettingKeys(expectedBase, latestState.settings, dirtyKeys)
      .some((key) => changedSettingKeys(nextSettings, latestState.settings, [key]).length > 0)) {
      throw new Error(dirtyKeys.includes("steam_workshop_collections")
        ? t("servers.mods.collections.conflict", undefined, "The instance's collections changed elsewhere. Reload and retry.")
        : dirtyKeys.includes("asa_mod_configurations")
          ? t("servers.mods.configurationConflict", undefined, "Mod configuration changed elsewhere. Reload the saved settings before applying your draft.")
        : t("projectzomboid.mods.maps.conflict", undefined, "Map order changed elsewhere. Reload the saved order before applying your changes."));
    }
    const mergedSettings = mergeSettingPatch(latestState.settings, nextSettings, dirtyKeys);

    await props.onSaveSettings({
      id: latestState.details.summary.id,
      bind_ip: latestState.details.summary.bind_ip,
      auto_backup_on_stop: latestState.details.auto_backup_on_stop,
      backup_retention_count: latestState.details.backup_retention_count,
      settings_json: JSON.stringify(mergedSettings, null, 2),
      ports: latestState.details.ports
    }, {
      expectedSettingsJson: latestState.details.settings_json,
      ...(removal ? { collectionRemoval: removal } : {}),
      throwOnError: true,
      silent: true
    });
    latestSettingsRef.current = mergedSettings;
    return mergedSettings;
  }

  async function saveArkModChange(change: ArkModChange, base: SettingsObject): Promise<SettingsObject> {
    const save = props.onSaveSettings;
    if (readOnly || !save) throw new Error(t("servers.archives.workspace.restoreForMods"));
    let saved = base;
    await runModMutation("configuration", async () => {
      const latest = await readWritableInstanceState();
      let next: SettingsObject;
      try { assertArkModChangeBase(base, latest.settings, moduleId, change); next = applyArkModChange(latest.settings, moduleId, change); }
      catch (failure) {
        if (!(failure instanceof ArkModScopeError)) throw failure;
        throw new Error(failure.code === "changed" ? locale === "zh-CN" ? "此 MOD 的目标配置已在别处修改，请重新读取已保存配置后再保存。" : "This Mod's target configuration changed elsewhere. Reload saved configuration before saving."
          : failure.code === "scope" ? locale === "zh-CN" ? "所选地图已变更，请刷新后重新选择。" : "The selected maps changed. Reload and select them again."
          : failure.code === "raw-flags" ? t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING)
          : locale === "zh-CN" ? "MOD 地图配置无效，请检查已保存的配置。" : "Mod map configuration is invalid. Review the saved configuration.");
      }
      await save({ id: latest.details.summary.id, bind_ip: latest.details.summary.bind_ip,
        auto_backup_on_stop: latest.details.auto_backup_on_stop, backup_retention_count: latest.details.backup_retention_count,
        settings_json: JSON.stringify(next, null, 2), ports: latest.details.ports },
      { expectedSettingsJson: latest.details.settings_json, throwOnError: true, silent: true });
      saved = next; latestSettingsRef.current = next;
      setScanNonce((current) => current + 1);
    });
    return saved;
  }

  async function saveProjectZomboidMapOrder(value: string, expectedValue: string | undefined) {
    const validation = validateProjectZomboidMapOrder(value, locale, t);
    if (validation) throw new Error(validation);
    const expectedBase = { ...latestSettingsRef.current };
    if (expectedValue === undefined) delete expectedBase.map_name;
    else expectedBase.map_name = expectedValue;
    await runModMutation("configuration", async () => {
      await persistSettings({ ...expectedBase, map_name: value }, expectedBase);
    });
  }

  function isCollectionSummary(item: SteamWorkshopLookupItem | null | undefined): boolean {
    return item?.item_kind === "collection" && !resolvedLookupMap[item.id];
  }

  function canInstallWorkshopItems(ids: string[]): boolean {
    const targetIds = uniqueEntries(ids);
    if (
      modMutationsDisabled ||
      modWorkflowUnsupported ||
      workflow.installScope === "client_only" ||
      targetIds.some((id) => isCollectionSummary(lookupMap[id]) || isIncompleteWorkshopCollection(lookupMap[id])) ||
      targetIds.length === 0
    ) {
      return false;
    }
    if (workflow.steamDownloadMode === "steamcmd-cache") {
      return Boolean(expectedAppId) && buildDownloadableSteamIds(targetIds, lookupMap, expectedAppId).length > 0;
    }
    return buildModSettingsApplyPlan(moduleId, settings, targetIds, lookupMap, expectedAppId).canApply;
  }

  function handleInstallWorkshopItems(
    targetWorkshopIds: string[],
    manifest?: { review: WorkshopManifestReview; enable: boolean },
    purpose: "enable" | "prepare" = "enable"
  ): Promise<boolean> {
    const usesSteamCmd = workflow.steamDownloadMode === "steamcmd-cache";
    const usesPalworldPackageNames = manualEnablement?.id_strategy === "palworld_package_name";
    const selectedIds = uniqueEntries(targetWorkshopIds);
    const resolvedItems = manifest?.review.items ?? lookupMap;
    if (selectedIds.some((id) => isIncompleteWorkshopCollection(resolvedItems[id]))) {
      setApplyMessage(t("servers.mods.incompleteCollection", undefined, "This collection contains nested or unresolved entries. Add the individual Mods instead."));
      return Promise.resolve(false);
    }
    let reviewedCollections: ManagedWorkshopCollection[];
    try {
      reviewedCollections = collectInstalledWorkshopCollections(manifest?.review.ids ?? selectedIds, resolvedItems, expectedAppId);
    } catch (error) {
      setDownloadError(describeError(error));
      setDownloadState("error");
      return Promise.resolve(false);
    }
    const installedCollections = purpose === "prepare" ? [] : reviewedCollections;
    const downloadableIds = buildDownloadableSteamIds(selectedIds, resolvedItems, expectedAppId);
    const targetApplyPlan = buildModSettingsApplyPlan(moduleId, settings, selectedIds, resolvedItems, expectedAppId, purpose);
    const permitted = manifest
      ? !modMutationsDisabled && !modWorkflowUnsupported && usesSteamCmd && Boolean(expectedAppId) &&
        manifest.review.issues.length === 0 && downloadableIds.length > 0
      : canInstallWorkshopItems(selectedIds);
    if (!permitted || (!usesSteamCmd && !targetApplyPlan.nextSettings)) {
      return Promise.resolve(false);
    }

    setInstallingWorkshopIds(new Set(uniqueEntries([...selectedIds, ...downloadableIds])));
    return runModMutation("install", async () => {
      if (installedCollections.length) mergeManagedWorkshopCollections(latestSettingsRef.current, installedCollections);
      if (purpose === "enable" && moduleId === "dontstarve" && manifest?.enable !== false && hasDstRawModOverrides(latestSettingsRef.current)) {
        throw new Error(t("dst.settings.modStatus.rawOverrideWarning", undefined, DST_RAW_MOD_WARNING));
      }
      setDownloadState("running");
      setDownloadResult(null);
      setDownloadError(null);
      setApplyMessage(null);

      // Recheck disk immediately before applying a reviewed list; its preview may be stale.
      let downloadIds = downloadableIds;
      if (manifest) {
        await readWritableInstanceState();
        const inventory = await readSteamWorkshopInstallationStatus(props.details.summary.id, downloadableIds);
        const installed = new Set(inventory.items.filter((item) => item.installed).map((item) => item.item_id));
        downloadIds = downloadableIds.filter((id) => !installed.has(id));
        setWorkshopInstallationResult(inventory);
        setResolvedLookupMap((current) => ({ ...current, ...resolvedItems }));
      }

      let inventoryBefore: ManualModInventoryResult | null = null;
      if (purpose === "enable" && usesPalworldPackageNames && manifest?.enable !== false) {
        if (!manualEnablement || !manualStaging) {
          throw new Error(t(
            "servers.mods.palworldInstallContractMissing",
            undefined,
            "Palworld Mod staging is not configured for this module."
          ));
        }
        inventoryBefore = await readManualModInventory(props.details.summary.id);
        setManualInventory(inventoryBefore);
        setManualInventoryError(null);
      }

      let workshopDownload: SteamWorkshopDownloadResult | null = null;
      // Preparation reuses the machine cache and runs any instance deployment declared by the module.
      // A cache hit alone does not prove that the selected instance has its Mod files.
      if (usesSteamCmd && downloadableIds.length > 0) {
        const preparationIds = downloadableIds;
        const download = (accountName?: string) => downloadSteamWorkshopItems(props.details.summary.id, preparationIds, true, accountName);
        try {
          workshopDownload = await download(steamAccount?.loginAccount);
        } catch (error) {
          if (steamAccount?.loginAccount !== undefined || !requiresSteamWorkshopAccount(error) ||
            !steamAccount?.available || !props.onSteamAccountRequired) throw error;
          workshopDownload = await props.onSteamAccountRequired(download);
        }
        const downloadResults = new Map(
          workshopDownload.items.map((item) => [item.item_id, item.expected_path_exists])
        );
        const missingItemIds = preparationIds.filter((id) => downloadResults.get(id) !== true);
        if (missingItemIds.length > 0) {
          throw new Error(t(
            "servers.mods.installMissingItems",
            { ids: missingItemIds.join(", ") },
            "SteamCMD did not materialize Workshop items: {ids}"
          ));
        }
        setDownloadResult(workshopDownload);
      }

      let nextSettings: SettingsObject | null = null;
      let settingsBase: SettingsObject | undefined;
      let installedCount = manifest ? downloadIds.length : workshopDownload?.items.length ?? 0;

      if (purpose === "prepare") {
        // Reading configuration must never change this instance's enablement intent.
        nextSettings = null;
      } else if (manifest?.enable === false) {
        setRetainedWorkshopIds((current) => uniqueEntries([...current, ...selectedIds]));
        if (moduleId === "terraria" || moduleId === "dontstarve") {
          const latestState = await readWritableInstanceState();
          latestSettingsRef.current = latestState.settings;
          settingsBase = latestState.settings;
          nextSettings = { ...latestState.settings };
          const ownershipKey = moduleId === "dontstarve" ? "shared_workshop_mod_ids" : "tmodloader_workshop_item_ids";
          nextSettings[ownershipKey] = moduleId === "dontstarve"
            ? mergeWorkshopList(nextSettings, ownershipKey, downloadableIds).value
            : mergeTextList(nextSettings, ownershipKey, downloadableIds, parseDelimitedEntries).value;
          if (moduleId === "dontstarve") {
            const restored = new Set(downloadableIds);
            const removed = readDstRemovedWorkshopModIds(nextSettings).filter((id) => !restored.has(id));
            if (removed.length) nextSettings.dst_removed_workshop_mod_ids = removed;
            else delete nextSettings.dst_removed_workshop_mod_ids;
          }
        } else if (["projectzomboid", "arksurvivalevolved", "barotrauma", "conanexiles", "soulmask"].includes(moduleId)) {
          const latestState = await readWritableInstanceState();
          settingsBase = latestState.settings;
          nextSettings = buildWorkshopDownloadOwnershipPlan(moduleId, settingsBase, downloadableIds);
        }
      } else if (usesPalworldPackageNames) {
        if (!manualEnablement || !manualStaging || !inventoryBefore) {
          throw new Error(t(
            "servers.mods.palworldInstallContractMissing",
            undefined,
            "Palworld Mod staging is not configured for this module."
          ));
        }

        const inventoryAfter = await readManualModInventory(props.details.summary.id);
        setManualInventory(inventoryAfter);
        setManualInventoryError(null);
        const enablementPlan = buildSteamWorkshopEnablementPlan({
          downloadedWorkshopItemIds: downloadableIds,
          inventoryBefore: inventoryBefore.items,
          inventoryAfter: inventoryAfter.items
        });
        if (enablementPlan.unresolvedWorkshopItemIds.length > 0) {
          throw new Error(t(
            "servers.mods.palworldPackageNamesMissing",
            { ids: enablementPlan.unresolvedWorkshopItemIds.join(", ") },
            "Downloaded Workshop items are missing PackageName metadata: {ids}"
          ));
        }
        if (enablementPlan.inferredIds.length === 0) {
          throw new Error(t(
            "servers.mods.palworldPackageNameMissing",
            undefined,
            "The downloaded Palworld Mod does not expose a PackageName in Info.json."
          ));
        }

        const latestState = await readWritableInstanceState();
        latestSettingsRef.current = latestState.settings;
        settingsBase = latestState.settings;
        nextSettings = { ...latestState.settings };
        const enablement = mergeTextList(
          nextSettings,
          manualEnablement.setting_key,
          enablementPlan.inferredIds,
          parseDelimitedEntries
        );
        nextSettings[manualEnablement.setting_key] = enablement.value;
        installedCount = enablementPlan.inferredIds.length;
        if (enablement.addedValues.length === 0) {
          nextSettings = null;
        }
      } else {
        const latestState = await readWritableInstanceState();
        latestSettingsRef.current = latestState.settings;
        settingsBase = latestState.settings;
        const latestApplyPlan = buildModSettingsApplyPlan(
          moduleId,
          latestState.settings,
          selectedIds,
          resolvedItems,
          expectedAppId,
          purpose
        );
        if (moduleId === "dontstarve" && hasDstRawModOverrides(latestState.settings)) {
          throw new Error(t("dst.settings.modStatus.rawOverrideWarning", undefined, DST_RAW_MOD_WARNING));
        }
        nextSettings = latestApplyPlan.canApply && latestApplyPlan.nextSettings
          ? latestApplyPlan.nextSettings
          : null;
        if (moduleId === "projectzomboid") {
          const snapshot = await readProjectZomboidWorkshopModsSnapshot(props.details.summary.id, downloadableIds);
          setPzSnapshot(snapshot);
          const localById = new Map(snapshot.items.map((item) => [item.workshop_item_id, item]));
          const missingMetadata = downloadableIds.filter((id) => !localById.get(id)?.mods.some(
            (mod) => Boolean(mod.mod_id?.trim()) || mod.map_ids.length > 0
          ));
          if (missingMetadata.length > 0) {
            throw new Error(t("servers.mods.manifest.missingMetadata", { ids: missingMetadata.join(", ") }));
          }
          const localItems = downloadableIds.flatMap((id) => {
            const item = localById.get(id);
            return item ? [item] : [];
          });
          const enablePlan = buildProjectZomboidEnablePlan(nextSettings ?? latestState.settings, localItems);
          nextSettings = enablePlan.nextSettings ?? nextSettings;
        }
        installedCount = workshopDownload?.items.length ?? latestApplyPlan.addedIds.length;
        if (!usesSteamCmd && !nextSettings) {
          setDownloadState("idle");
          setApplyMessage(t(latestApplyPlan.summaryKey, latestApplyPlan.params, latestApplyPlan.summaryFallback));
          return;
        }
      }

      if (moduleId === "palworld" && purpose !== "prepare") {
        if (!nextSettings) {
          const current = await readWritableInstanceState();
          settingsBase = current.settings;
          nextSettings = current.settings;
        }
        nextSettings = restoreRemovedWorkshopIds(nextSettings, downloadableIds);
      }
      if (installedCollections.length) {
        if (!nextSettings) {
          const latestState = await readWritableInstanceState();
          latestSettingsRef.current = latestState.settings;
          settingsBase = latestState.settings;
          nextSettings = latestState.settings;
        }
        nextSettings = mergeManagedWorkshopCollections(nextSettings, installedCollections);
      }
      if (nextSettings) {
        await persistSettings(nextSettings, settingsBase);
      }

      const downloadOnly = purpose === "prepare" || manifest?.enable === false;
      const deployedToInstance = moduleId === "dontstarve" || Boolean(manualStaging);
      const completionKey = downloadOnly
        ? "servers.mods.manifest.downloadCompleted"
        : deployedToInstance
          ? "servers.mods.installCompleted"
          : "servers.mods.cacheConfiguredCompleted";
      const completionFallback = downloadOnly
        ? "Cached {count} additional Mod item(s) locally."
        : deployedToInstance
          ? "Deployed {count} Mod item(s) into this instance."
          : "Cached {count} Mod item(s) locally and configured this instance.";
      startTransition(() => {
        setDownloadState("success");
        setApplyMessage(t(completionKey, { count: installedCount }, completionFallback));
        setScanNonce((current) => current + 1);
      });
    }).then(() => true).catch((error) => {
      startTransition(() => {
        setDownloadError(describeError(error));
        setDownloadState("error");
      });
      setScanNonce((current) => current + 1);
      return false;
    }).finally(() => {
      setInstallingWorkshopIds(new Set());
    });
  }

  function handleRemoveWorkshopItems(ids: string[]) {
    const targetIds = [...ids];
    launchModMutation("enablement", async () => {
      const latestState = await readWritableInstanceState();
      if (moduleId === "projectzomboid") {
        try {
          const snapshot = await readProjectZomboidWorkshopModsSnapshot(props.details.summary.id,
            uniqueEntries([...parseWorkshopIdList(latestState.settings.workshop_items), ...targetIds]));
          const plan = buildProjectZomboidWorkshopRemovalPlan(latestState.settings, targetIds, snapshot);
          await persistSettings(plan.nextSettings, latestState.settings, undefined,
            (current) => assertWorkshopControlBase(latestState.settings, current));
          setPzSnapshot(snapshot);
          setApplyMessage(null);
          setScanNonce((value) => value + 1);
        } catch (error) { throw workshopControlError(error); }
        return;
      }
      if (moduleId === "terraria" && parseLineOrSemicolonEntries(latestState.settings.tmodloader_enabled_mod_names).length) {
        throw new Error(t("servers.mods.collections.removalError.terraria-metadata"));
      }
      if (moduleId === "arksurvivalevolved") {
        await persistSettings(applyArkSharedModAction(latestState.settings, moduleId, targetIds, "remove"), latestState.settings);
        setScanNonce((current) => current + 1); return;
      }
      const removePlan = buildModSettingsRemovePlan(
        moduleId,
        latestState.settings,
        targetIds,
        lookupMap,
        expectedAppId,
        pzSnapshot,
        readManagedWorkshopCollections(latestState.settings, moduleId).flatMap((collection) => collection.member_ids)
      );
      if (!removePlan.canRemove || !removePlan.nextSettings) {
        setApplyMessage(t(removePlan.summaryKey, removePlan.params, removePlan.summaryFallback));
        return;
      }

      await persistSettings(removePlan.nextSettings, latestState.settings, undefined,
        (current) => {
          if (moduleId === "dontstarve") assertDstModOwnership(current, targetIds);
          else assertWorkshopControlBase(latestState.settings, current);
        });
      startTransition(() => {
        setApplyMessage(null);
        setScanNonce((current) => current + 1);
      });
    });
  }

  function handleEnableManualInventoryItem(item: ManualModInventoryItem) {
    const inferredId = item.inferred_id?.trim();
    if (!manualEnablement || !inferredId) {
      return;
    }
    if (moduleId === "arksurvivalascended") { handleAsaModAction(inferredId, "enable"); return; }
    if (moduleId === "palworld") {
      const ids = inventoryWorkshopIds(item, manualInventory).filter((id) => workshopControlStates.get(id)?.owned);
      if (ids.length) { handleSetCollectionMembersEnabled(ids, true); return; }
    }
    launchModMutation("enablement", async () => {
      const nextSettings: SettingsObject = { ...latestSettingsRef.current };
      const merged = mergeTextList(nextSettings, manualEnablement.setting_key, [inferredId], parseDelimitedEntries);
      if (merged.addedValues.length === 0) {
        setApplyMessage(t("servers.mods.applySummary.alreadyApplied", undefined, "The selected items are already in this instance."));
        return;
      }
      nextSettings[manualEnablement.setting_key] = merged.value;
      await persistSettings(nextSettings);
      startTransition(() => {
        setApplyMessage(null);
        setScanNonce((current) => current + 1);
      });
    });
  }

  function entryValueParser(entry: ModSourceEntry): (value: unknown) => string[] {
    if (entry.key.startsWith("arksurvivalascended-") && ["mod_ids_csv", "passive_mod_ids_csv"].includes(entry.fieldLabel)) return parseAsaModIds;
    if (entry.key.startsWith("arksurvivalevolved-")) return parseEvolvedModIds;
    return entry.key === "pz-mods" || entry.key === "pz-maps"
      ? parseLineOrSemicolonEntries
      : parseDelimitedEntries;
  }

  function handleDisableEnabledSettingValue(entry: ModSourceEntry, value: string) {
    const targetValue = value.trim();
    if (!targetValue || !entry.fieldLabel) {
      return;
    }
    launchModMutation("enablement", async () => {
      const nextSettings: SettingsObject = { ...latestSettingsRef.current };
      const removed = removeTextListValues(nextSettings, entry.fieldLabel, [targetValue], entryValueParser(entry));
      if (removed.removedValues.length === 0) {
        setApplyMessage(t("servers.mods.removeSummary.notConfigured", undefined, "This mod is not configured in the instance."));
        return;
      }
      nextSettings[entry.fieldLabel] = removed.value;
      await persistSettings(nextSettings);
      startTransition(() => {
        setApplyMessage(null);
        setScanNonce((current) => current + 1);
      });
    });
  }

  function assertDstModOwnership(current: SettingsObject, ids: string[]): string[] {
    if (hasDstRawModOverrides(current)) {
      throw new Error(t("dst.settings.modStatus.rawOverrideWarning", undefined, DST_RAW_MOD_WARNING));
    }
    const members = readManagedWorkshopCollections(current, moduleId).flatMap((collection) => collection.member_ids);
    const owned = new Set(buildConfigurableEntries("dontstarve", buildConfiguredEntries("dontstarve", current, undefined, members))
      .flatMap((entry) => entry.ids));
    if (ids.some((id) => !owned.has(id))) {
      throw new Error(t("servers.mods.removeSummary.notConfigured", undefined, "This mod is not configured in the instance."));
    }
    return members;
  }

  function assertAsaMembershipBase(base: SettingsObject, current: SettingsObject) {
    if (hasAsaRawModFlags(current)) throw new Error(t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING));
    if (changedSettingKeys(base, current, ASA_MEMBERSHIP_KEYS).length) {
      throw new Error(t("servers.mods.membershipChanged", undefined, "The instance’s Mods changed elsewhere. Reload and retry."));
    }
  }

  async function applyAsaModAction(ids: string[], action: AsaModAction) {
    const latest = await readWritableInstanceState();
    assertAsaMembershipBase(latest.settings, latest.settings);
    const owned = readAsaModMembership(latest.settings).owned;
    const inventoryIds = ids.some((id) => !owned.has(id)) && action !== "add" && action !== "restore-files"
      ? (await readManualModInventory(props.details.summary.id)).items.map((item) => item.inferred_id ?? "") : [];
    let next: SettingsObject;
    try { next = applyArkSharedModAction(latest.settings, moduleId, ids, action, inventoryIds); }
    catch (error) {
      if (!(error instanceof AsaModControlError)) throw error;
      throw new Error(error.code === "raw-mod-flags" ? t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING) : error.code === "not-owned"
        ? t("servers.mods.removeSummary.notConfigured", undefined, "This mod is not configured in the instance.")
        : t("servers.mods.invalidCurseForgeId", undefined, "Enter a valid numeric CurseForge project ID."));
    }
    await persistSettings(next, latest.settings, undefined, (current) => assertAsaMembershipBase(latest.settings, current));
    startTransition(() => { setApplyMessage(null); setScanNonce((current) => current + 1); });
  }

  /** Checklist mode: the written list becomes exactly the shared active list (CAS against what the editor saw). */
  async function applyAsaActiveList(ids: string[], expected: string[], instanceId: string) {
    if (instanceId !== props.details.summary.id) throw new Error(t("servers.mods.membershipChanged", undefined, "The instance’s Mods changed elsewhere. Reload and retry."));
    const latest = await readWritableInstanceState();
    const assertExpected = (current: SettingsObject) => {
      assertAsaMembershipBase(latest.settings, current);
      if (readAsaModMembership(current).active.join("\n") !== expected.join("\n")) {
        throw new Error(t("servers.mods.membershipChanged", undefined, "The instance’s Mods changed elsewhere. Reload and retry."));
      }
    };
    assertExpected(latest.settings);
    let next: SettingsObject;
    try { next = buildAsaActiveListPlan(latest.settings, ids); }
    catch (error) {
      if (!(error instanceof AsaModControlError)) throw error;
      throw new Error(error.code === "raw-mod-flags" ? t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING)
        : t("servers.mods.invalidCurseForgeId", undefined, "Enter a valid numeric CurseForge project ID."));
    }
    await persistSettings(next, latest.settings, undefined, assertExpected);
    startTransition(() => { setApplyMessage(null); setScanNonce((current) => current + 1); });
  }

  function handleAsaModAction(id: string, action: AsaModAction) {
    launchModMutation("enablement", () => applyAsaModAction([id], action));
  }

  function handleSetDstModsEnabled(ids: string[], enabled: boolean) {
    const targetIds = uniqueEntries(ids);
    if (!targetIds.length) return;
    launchModMutation("enablement", async () => {
      const latestState = await readWritableInstanceState();
      const members = assertDstModOwnership(latestState.settings, targetIds);
      if (enabled && targetIds.some((id) => lookupMap[id] &&
        (isUnsupportedWorkshopItem(lookupMap[id], expectedAppId) || !steamItemBelongsToApp(lookupMap[id], expectedAppId)))) {
        throw new Error(t("servers.mods.configPanel.unsupportedItem", undefined,
          "This entry is a Steam guide or other non-installable content. Remove it from this instance's Mod list."));
      }
      const next = buildDstModEnablementPlan(latestState.settings, targetIds, enabled, members);
      if (!next) return;
      await persistSettings(next, latestState.settings, undefined, (current) => { assertDstModOwnership(current, targetIds); });
      startTransition(() => {
        if (targetIds.length === 1) setSelectedEnabledRowKey(`dst-${enabled ? "enabled" : "disabled"}:${targetIds[0]}`);
        setApplyMessage(null);
        setScanNonce((current) => current + 1);
      });
    });
  }

  function handleDisableEnabledRow(row: ModEnabledRow) {
    const id = formatEnabledRowDisplayId(row).trim();
    if (!id) {
      setApplyMessage(t("servers.mods.applySummary.empty", undefined, "Select mods from the shelf first."));
      return;
    }
    if (isSteamWorkshopModule) {
      setRetainedWorkshopIds((current) => uniqueEntries([...current, id]));
    }
    if (moduleId === "dontstarve" && row.entry.key === "dst-enabled") {
      handleSetDstModsEnabled([id], false);
      return;
    }
    if (moduleId === "arksurvivalascended") { handleAsaModAction(id, "disable"); return; }
    if (moduleId === "palworld") {
      const ids = uniqueEntries((manualInventory?.items ?? []).filter((item) =>
        item.inferred_id?.toLowerCase() === id.toLowerCase()).flatMap((item) => inventoryWorkshopIds(item, manualInventory)))
        .filter((workshopId) => workshopControlStates.get(workshopId)?.owned);
      if (ids.length) { handleSetCollectionMembersEnabled(ids, false); return; }
    }
    if (isManualEnablementModule || row.entry.ids.length === 0) {
      handleDisableEnabledSettingValue(row.entry, row.value || id);
      return;
    }
    handleRemoveWorkshopItems([id]);
  }

  function handleDeleteEnabledRow(row: ModEnabledRow) {
    const id = formatEnabledRowDisplayId(row).trim();
    if (!id) {
      return;
    }
    if (moduleId === "arksurvivalascended") { handleAsaModAction(id, "remove"); return; }
    if (isSteamWorkshopModule) {
      setRetainedWorkshopIds((current) => uniqueEntries([...current, id]));
    }
    if (moduleId === "palworld") {
      const ids = uniqueEntries((manualInventory?.items ?? []).filter((item) =>
        item.inferred_id?.toLowerCase() === id.toLowerCase()).flatMap((item) => inventoryWorkshopIds(item, manualInventory)))
        .filter((workshopId) => workshopControlStates.get(workshopId)?.owned);
      if (ids.length) { handleRemoveManagedWorkshopMembers(ids); return; }
    }
    if (isManualEnablementModule || row.entry.ids.length === 0) {
      handleDisableEnabledSettingValue(row.entry, row.value || id);
      return;
    }
    handleRemoveWorkshopItems([id]);
  }

  const handleEnableManualReferences = useCallback(async (references: string[]) => {
    const cleanReferences = nexusSource ? [...new Set(references.map((reference) => reference.trim()).filter(Boolean))]
      : uniqueEntries(references.map((reference) => reference.trim()).filter(Boolean));
    if (!manualEnablement && !canInstallSiteReferences) {
      return;
    }
    if (cleanReferences.length === 0) {
      setManualReferenceState("error");
      setManualReferenceResult(null);
      setManualReferenceError(t("servers.mods.referenceNoInput", undefined, "Drop a mod link or paste a mod ID first."));
      return;
    }

    try {
      await runModMutation(manualEnablement ? "enablement" : "staging", async () => {
        setManualReferenceState("running");
        setManualReferenceResult(null);
        setManualReferenceError(null);
        setManualStageError(null);
        if (nexusSource) {
          const generation = nexusGeneration.current, account = await readNexusAccountStatus();
          if (generation !== nexusGeneration.current) return;
          setNexusAccount(account);
          if (!account.configured) {
            setManualReferenceState("idle"); setNexusCredentialsOpen(true);
            setManualReferenceError(nexusText(locale, "nexus_auth_required")); return;
          }
          const reviews = await reviewNexusModReferences(props.details.summary.id, cleanReferences);
          if (generation !== nexusGeneration.current) return;
          setNexusReview({ references: cleanReferences, reviews, input: manualReferenceInput });
          setNexusInstallError(null); setManualReferenceState("idle"); return;
        }
        if (!manualEnablement) {
          setManualStageState("running");
          setManualStageResult(null);
          const result = await installManualModReferences(props.details.summary.id, cleanReferences,
            manualSourceProvider === "thunderstore" ? thunderstoreVersionPolicy : undefined);
          startTransition(() => {
            setManualStageResult(result);
            setManualStageState("success");
            setManualReferenceState("success");
            setManualReferenceInput("");
            setApplyMessage(null);
            setScanNonce((current) => current + 1);
          });
          return;
        }

        const result = await resolveManualModReferences(props.details.summary.id, cleanReferences);
        if (moduleId === "arksurvivalascended") {
          await applyAsaModAction(result.resolved_ids, "add");
          startTransition(() => {
            setManualReferenceState("success"); setManualReferenceResult(result); setManualReferenceInput("");
          });
          return;
        }
        const latestState = await readWritableInstanceState();
        latestSettingsRef.current = latestState.settings;
        const nextSettings: SettingsObject = { ...latestState.settings };
        const merged = mergeTextList(nextSettings, manualEnablement.setting_key, result.resolved_ids, parseDelimitedEntries);
        if (merged.addedValues.length === 0) {
          setManualReferenceState("success");
          setManualReferenceResult(result);
          setApplyMessage(t("servers.mods.applySummary.alreadyApplied", undefined, "The selected items are already in this instance."));
          return;
        }
        nextSettings[manualEnablement.setting_key] = merged.value;
        await persistSettings(nextSettings);
        startTransition(() => {
          setManualReferenceState("success");
          setManualReferenceResult(result);
          setManualReferenceInput("");
          setApplyMessage(null);
          setScanNonce((current) => current + 1);
        });
      });
    } catch (error) {
      startTransition(() => {
        if (nexusSource) {
          setManualReferenceError(nexusErrorText(locale, error)); setManualReferenceState("error");
          if (error instanceof NexusApiError && ["nexus_auth_required", "nexus_auth_invalid"].includes(error.code)) setNexusCredentialsOpen(true);
          return;
        }
        setManualReferenceError(t(
          manualEnablement ? "servers.mods.referenceFailed" : "servers.mods.referenceInstallFailed",
          { message: formatDesktopError(t, error) },
          manualEnablement ? "Mod reference resolution failed: {message}" : "Mod installation failed: {message}"
        ));
        setManualReferenceState("error");
        if (!manualEnablement) {
          setManualStageState("error");
        }
      });
    }
  }, [canInstallSiteReferences, manualEnablement, manualSourceProvider, thunderstoreVersionPolicy, moduleId, props.details.summary.id, runModMutation, t, instanceSettings, arkProjection.error, nexusSource, locale, manualReferenceInput]);

  async function handleInstallNexusFiles(selected: Record<number, string>) {
    const review = nexusReview, generation = nexusGeneration.current;
    if (!review) return;
    try {
      await runModMutation("staging", async () => {
        const selections = review.reviews.map((project) => {
          const file = project.files.find((candidate) => candidate.file_id === selected[project.reference_index] && candidate.downloadable);
          if (!file) throw new NexusApiError("nexus_file_unavailable");
          return { reference: review.references[project.reference_index], file_id: file.file_id };
        });
        setManualStageState("running"); setManualStageResult(null); setNexusInstallError(null); setManualReferenceError(null);
        const result = await installNexusModFiles(props.details.summary.id, selections);
        if (generation !== nexusGeneration.current) return;
        setManualStageResult(result); setManualStageState("success"); setManualReferenceState("success"); setNexusReview(null);
        setManualReferenceInput((input) => input === review.input ? "" : input);
        setScanNonce((current) => current + 1);
      });
    } catch (failure) {
      if (generation !== nexusGeneration.current) return;
      setNexusInstallError(nexusErrorText(locale, failure)); setManualStageState("error");
    }
  }

  function handleEnableManualReferenceInput() {
    void handleEnableManualReferences((nexusSource ? nexusReferencesFromText : referenceCandidatesFromText)(manualReferenceInput));
  }

  const browsedWorkshopItems = browseResult?.items ?? EMPTY_WORKSHOP_ITEMS;
  const manualInventoryItems = manualInventory?.items ?? EMPTY_MANUAL_INVENTORY_ITEMS;
  // File-based Workshop games deploy each item into its own numeric directory.
  // Only payloads in this instance's target establish membership, never shared caches.
  const deployedWorkshopInventoryItems = useMemo(
    () => isSteamWorkshopModule && !manualEnablement
      ? manualInventoryItems.filter((item) => item.item_type === "directory" && item.file_count > 0 && /^\d+$/.test(item.name))
      : EMPTY_MANUAL_INVENTORY_ITEMS,
    [isSteamWorkshopModule, manualEnablement, manualInventoryItems]
  );
  const configuredSteamIdSet = useMemo(
    () => new Set([
      ...configurableEntries.filter((entry) => entry.key !== "dst-disabled").flatMap((entry) => entry.ids),
      ...deployedWorkshopInventoryItems.map((item) => item.name),
      ...(moduleId === "palworld" ? [...workshopControlStates].filter(([, state]) => state.owned).map(([id]) => id) : [])
    ]),
    [configurableEntries, deployedWorkshopInventoryItems, moduleId, workshopControlStates]
  );
  const configuredManualModIdSet = useMemo(
    () => new Set(
      configuredEntries
        .flatMap((entry) => [...entry.values, ...entry.ids])
        .map((entry) => entry.toLowerCase())
    ),
    [configuredEntries]
  );
  const unconfiguredInventoryItems = useMemo(
    () => !manualEnablement ? manualInventoryItems : manualInventoryItems.filter((item) =>
      Boolean(item.inferred_id) &&
      !(moduleId === "arksurvivalascended" && asaMembership.removed.has(canonicalAsaModId(item.inferred_id) ?? "")) &&
      !(moduleId === "palworld" && isRemovedWorkshopInventoryItem(item, manualInventory, settings)) &&
      !configuredManualModIdSet.has(moduleId === "arksurvivalascended"
        ? canonicalAsaModId(item.inferred_id) ?? "" : String(item.inferred_id).toLowerCase())
    ),
    [configuredManualModIdSet, manualInventoryItems, manualEnablement, moduleId, manualInventory, settings, asaMembership]
  );
  // Workshop inventory includes machine-wide caches; it is not proof of deployment into this instance.
  const machineCachedSteamIdSet = useMemo(() => new Set(uniqueEntries([
    ...deployedWorkshopInventoryItems.map((item) => item.name),
    ...(workshopInstallationResult?.items ?? [])
      .filter((item) => item.installed)
      .map((item) => item.item_id),
    ...(downloadResult?.items ?? [])
      .filter((item) => item.expected_path_exists)
      .map((item) => item.item_id),
    ...(pzSnapshot?.items ?? [])
      .filter((item) => ["installed", "installed_with_warnings"].includes(item.status))
      .map((item) => item.workshop_item_id)
  ])), [deployedWorkshopInventoryItems, downloadResult, pzSnapshot, workshopInstallationResult]);
  const inspectedMachineSteamIdSet = useMemo(() => new Set(uniqueEntries([
    ...deployedWorkshopInventoryItems.map((item) => item.name),
    ...(workshopInstallationResult?.items ?? []).map((item) => item.item_id),
    ...(downloadResult?.items ?? []).map((item) => item.item_id),
    ...(pzSnapshot?.items ?? []).map((item) => item.workshop_item_id)
  ])), [deployedWorkshopInventoryItems, downloadResult, pzSnapshot, workshopInstallationResult]);
  const catalogItems = useMemo(
    () => browsedWorkshopItems.map((item) => lookupMap[item.id] ?? item)
      .filter((item) => !(item.status === "resolved" && item.item_kind === "item" &&
        item.consumer_app_id === expectedAppId && isClientOnlyDstWorkshopItem(item, expectedAppId))),
    [browsedWorkshopItems, lookupMap, expectedAppId]
  );
  const selectedBrowseWorkshopItem = useMemo(() => {
    if (!selectedBrowseWorkshopId) return null;
    const preview = detailPreview?.locale === locale && detailPreview.item.id === selectedBrowseWorkshopId ? detailPreview.item : null;
    const item = lookupMap[selectedBrowseWorkshopId] ?? preview ?? {
      id: selectedBrowseWorkshopId, title: selectedBrowseWorkshopId,
      detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${selectedBrowseWorkshopId}`,
      item_kind: "unknown", status: "unresolved", child_count: 0, children: []
    } satisfies SteamWorkshopLookupItem;
    if (presentations[item.id]) return item;
    const summary = browsedWorkshopItems.find((entry) => entry.id === item.id);
    // Do not flash canonical batch text while the requested language is loading.
    return { ...item, title: summary?.title ?? preview?.title ?? item.id,
      description: undefined, description_excerpt: undefined, children: [] };
  }, [detailPreview, lookupMap, selectedBrowseWorkshopId, locale, presentations, browsedWorkshopItems]);
  const selectedBrowseDetailsReady = Boolean(selectedBrowseWorkshopId && presentations[selectedBrowseWorkshopId]);

  function openWorkshopDetails(id: string | null) {
    setDetailHistory([]);
    setDetailPreview(null);
    setDetailFailure(null);
    setSelectedBrowseWorkshopId(id);
  }

  function openWorkshopChild(id: string) {
    if (!selectedBrowseWorkshopItem || id === selectedBrowseWorkshopItem.id) return;
    const child = selectedBrowseWorkshopItem.children.find((item) => item.id === id);
    if (!child) return;
    setDetailHistory((history) => [...history, selectedBrowseWorkshopItem.id].slice(-32));
    setDetailPreview({ locale, item: { ...child, child_count: 0, children: [],
      detail_url: `https://steamcommunity.com/sharedfiles/filedetails/?id=${id}` } });
    setDetailFailure(null);
    setSelectedBrowseWorkshopId(id);
  }

  function returnToWorkshopParent() {
    const parent = detailHistory[detailHistory.length - 1];
    if (!parent) return;
    setDetailHistory((history) => history.slice(0, -1));
    setDetailPreview(null);
    setDetailFailure(null);
    setSelectedBrowseWorkshopId(parent);
  }
  const selectedEnabledRow = useMemo(
    () => enabledRows.find((row) => row.key === selectedEnabledRowKey)
      ?? (isArk ? enabledRows.find((row) => row.id === selectedArkModId) : null) ?? enabledRows[0] ?? null,
    [enabledRows, selectedEnabledRowKey, isArk, selectedArkModId]
  );
  const selectedInventoryItem = useMemo(
    () => unconfiguredInventoryItems.find((item) => item.path === selectedInventoryPath) ?? unconfiguredInventoryItems[0] ?? null,
    [unconfiguredInventoryItems, selectedInventoryPath]
  );
  const configuredEntryCount = enabledRows.length;
  const verificationWarning = buildWorkshopVerificationWarning(t, uniqueEntries([
    ...workshopLookupIds, ...catalogItems.map((item) => item.id), ...(selectedBrowseWorkshopId ? [selectedBrowseWorkshopId] : [])
  ]).map((id) => lookupMap[id]).filter((item): item is SteamWorkshopLookupItem => Boolean(item)),
  selectedBrowseDetailsReady ? selectedBrowseWorkshopItem : null);

  useEffect(() => {
    // Same-value updates can still schedule work while inventory transitions are pending.
    if (enabledRows.length === 0) {
      if (selectedEnabledRowKey !== null) setSelectedEnabledRowKey(null);
    } else {
      const byKey = enabledRows.find((row) => row.key === selectedEnabledRowKey);
      const selected = byKey ?? (isArk ? enabledRows.find((row) => row.id === selectedArkModId) : null) ?? enabledRows[0];
      if (selectedEnabledRowKey !== selected.key) setSelectedEnabledRowKey(selected.key);
      if (isArk && selectedArkModId !== selected.id) setSelectedArkModId(selected.id);
    }
  }, [enabledRows, selectedEnabledRowKey, isArk, selectedArkModId]);

  useEffect(() => {
    if (!draggedEnabledRow) {
      return;
    }
    function clearPointerDragState() {
      setDraggedEnabledRow(null);
      setEnabledDropTargetKey(null);
    }
    window.addEventListener("pointerup", clearPointerDragState);
    window.addEventListener("blur", clearPointerDragState);
    return () => {
      window.removeEventListener("pointerup", clearPointerDragState);
      window.removeEventListener("blur", clearPointerDragState);
    };
  }, [draggedEnabledRow]);

  useEffect(() => {
    if (!showsManualInventory) return;
    if (unconfiguredInventoryItems.length === 0) {
      if (selectedInventoryPath !== null) setSelectedInventoryPath(null);
    } else if (!selectedInventoryPath || !unconfiguredInventoryItems.some((item) => item.path === selectedInventoryPath)) {
      setSelectedInventoryPath(unconfiguredInventoryItems[0].path);
    }
  }, [unconfiguredInventoryItems, showsManualInventory, selectedInventoryPath]);

  function resolveWorkshopStoreItemState(item: SteamWorkshopLookupItem): WorkshopStoreItemState {
    const state = resolveStoreItemState({
      item, moduleId, expectedAppId, configuredIds: configuredSteamIdSet,
      cachedIds: machineCachedSteamIdSet, inspectedIds: inspectedMachineSteamIdSet,
      inspectionFailed: Boolean(workshopInstallationError), installingIds: installingWorkshopIds,
      steamDownloadMode: workflow.steamDownloadMode,
      enabled: supportsWorkshopModEnablement(moduleId) || moduleId === "palworld"
        ? workshopItemContentIds(item).every((id) => workshopControlStates.get(id)?.enabled) : undefined
    });
    return item.item_kind === "collection"
      ? { ...state, action: managedCollectionIds.has(item.id) ? "manage" : "install" }
      : state;
  }

  function matchingWorkshopInventoryItems(contentIds: readonly string[], inventory = manualInventory): ManualModInventoryItem[] {
    return workshopInventoryItems(contentIds, inventory);
  }

  function resolveWorkshopContentSelection(contentIds: string[]) {
    const localIds = moduleId === "projectzomboid"
      ? collectProjectZomboidLocalIds(pzSnapshot, contentIds)
      : { modIds: [], mapIds: [] };
    const inventoryItems = matchingWorkshopInventoryItems(contentIds);
    const candidateIds = new Set([
      ...localIds.modIds, ...localIds.mapIds, ...contentIds,
      ...inventoryItems.flatMap((item) => item.inferred_id ? [item.inferred_id] : [])
    ].map((id) => id.toLowerCase()));
    const row = enabledRows.find((entry) => candidateIds.has(entry.id.toLowerCase()));
    return { row: row ?? null, inventoryItem: inventoryItems[0] ?? null };
  }

  function handleManageWorkshopContent(contentIds: string[]) {
    setBrowseKind("item");
    setManifestMode(false);
    const { row, inventoryItem } = resolveWorkshopContentSelection(contentIds);
    if (row) {
      setSelectedEnabledRowKey(row.key);
      setActiveDetailSource("enabled");
    } else {
      if (inventoryItem) {
        setSelectedInventoryPath(inventoryItem.path);
        setActiveDetailSource("inventory");
      }
    }
    setActiveWorkbenchView("config");
  }

  function handleManageWorkshopItem(item: SteamWorkshopLookupItem) {
    if (item.item_kind === "collection") {
      setBrowseKind("collection");
      setSelectedCollectionId(item.id);
      setManifestMode(false);
      setActiveWorkbenchView("config");
      return;
    }
    handleManageWorkshopContent(workshopItemContentIds(item));
  }

  function handleWorkshopStoreAction(item: SteamWorkshopLookupItem, action: WorkshopStoreAction) {
    if (action === "manage") {
      handleManageWorkshopItem(item);
      return;
    }
    handleInstallWorkshopItems([item.id]);
  }

  function handleSelectEnabledRow(rowKey: string) {
    setSelectedEnabledRowKey(rowKey);
    if (isArk) setSelectedArkModId(enabledRows.find((row) => row.key === rowKey)?.id ?? null);
    setActiveDetailSource("enabled");
  }

  function formatEnabledRowDisplayId(row: ModEnabledRow): string {
    return row.id || row.value;
  }
  function canSortWorkbenchRow(row: ModEnabledRow): boolean {
    return canReorderEnabledRow(row) && (!isArk || entryValueParser(row.entry)(settings[row.entry.fieldLabel]).includes(row.value));
  }

  function handleReorderEnabledRow(sourceRow: ModEnabledRow | null, targetRow: ModEnabledRow) {
    if (!sourceRow || sourceRow.entry.key !== targetRow.entry.key || !canReorderEnabledRow(sourceRow)) {
      return;
    }

    launchModMutation("enablement", async () => {
      const fieldLabel = sourceRow.entry.fieldLabel.trim();
      const latestSettings = moduleId === "arksurvivalascended" ? (await readWritableInstanceState()).settings : latestSettingsRef.current;
      if (moduleId === "arksurvivalascended") assertAsaMembershipBase(latestSettings, latestSettings);
      const latestValues = entryValueParser(sourceRow.entry)(latestSettings[fieldLabel]);
      const latestEntry = { ...sourceRow.entry, ids: latestValues, values: latestValues };
      const reorderedValues = reorderEnabledEntryValues(latestEntry, sourceRow.value, targetRow.value);
      if (!reorderedValues || !fieldLabel) {
        return;
      }

      const nextSettings: SettingsObject = { ...latestSettings, [fieldLabel]: reorderedValues.join("\n") };
      await persistSettings(nextSettings, moduleId === "arksurvivalascended" ? latestSettings : undefined, undefined,
        moduleId === "arksurvivalascended" ? (current) => assertAsaMembershipBase(latestSettings, current) : undefined);
      startTransition(() => {
        setApplyMessage(null);
        setSelectedEnabledRowKey(sourceRow.key);
      });
    });
  }

  function handleEnabledRowPointerDown(event: PointerEvent<HTMLButtonElement>, row: ModEnabledRow) {
    if (modMutationsDisabled || event.button !== 0 || !canSortWorkbenchRow(row)) {
      return;
    }
    setDraggedEnabledRow(row);
    setEnabledDropTargetKey(null);
  }

  function handleEnabledRowPointerEnter(row: ModEnabledRow) {
    if (!draggedEnabledRow || draggedEnabledRow.key === row.key || draggedEnabledRow.entry.key !== row.entry.key) {
      return;
    }
    if (enabledDropTargetKey !== row.key) {
      setEnabledDropTargetKey(row.key);
    }
  }

  function handleEnabledRowPointerUp(event: PointerEvent<HTMLButtonElement>, row: ModEnabledRow) {
    if (!draggedEnabledRow) {
      return;
    }
    const targetRow = enabledDropTargetKey
      ? enabledRows.find((candidate) => candidate.key === enabledDropTargetKey) ?? row
      : row;
    if (targetRow.key !== draggedEnabledRow.key) {
      event.preventDefault();
      handleReorderEnabledRow(draggedEnabledRow, targetRow);
    }
    setDraggedEnabledRow(null);
    setEnabledDropTargetKey(null);
  }

  function handleEnabledRowPointerCancel() {
    setDraggedEnabledRow(null);
    setEnabledDropTargetKey(null);
  }

  function renderModList() {
    const hasRows = enabledRows.length > 0 ||
      (showsManualInventory && unconfiguredInventoryItems.length > 0);

    if (!hasRows && showsManualInventory && !manualInventory) {
      return <div className="mw-empty" aria-busy={!manualInventoryError}>
        {manualInventoryError ? <button className="mw-ghost-btn" type="button" onClick={() => setScanNonce((value) => value + 1)}>
          {t("common.retry", undefined, "Retry")}
        </button> : <><ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" />
          <span>{t("servers.mods.inventoryLoading", undefined, "Reading installed Mods…")}</span></>}
      </div>;
    }
    if (!hasRows) {
      return (
        <div className="mw-empty">
          <ShellIcon name="inbox" className="mw-empty-icon" />
          <span>{t("servers.mods.configList.empty", undefined, "No Mods are configured for this instance.")}</span>
        </div>
      );
    }

    return (
      <div className="mw-entry-list">
        {enabledRows.map((row) => {
          const isActive = activeDetailSource === "enabled" && row.key === selectedEnabledRow?.key;
          const controlState = supportsWorkshopModEnablement(moduleId) && row.entry.kind === "steam-item"
            ? workshopControlStates.get(row.id) ?? null : null;
          const sharedArkMember = !isArk || (moduleId === "arksurvivalascended" ? sharedAsaMembership.owned.has(row.id)
            : uniqueEntries([...parseEvolvedModIds(settings.active_mod_ids), ...parseEvolvedModIds(settings.auto_managed_mod_ids)]).includes(row.id));
          const isDisabled = moduleId === "arksurvivalascended" ? !sharedAsaMembership.enabled.has(row.id)
            : controlState ? !controlState.enabled : row.entry.key === "dst-disabled";
          const id = formatEnabledRowDisplayId(row);
          const lookup = lookupMap[row.id];
          const title = lookup?.title ?? row.value ?? row.entry.label;
          const lifecycle = lookup ? resolveWorkshopStoreItemState(lookup).lifecycleState
            : moduleId === "dontstarve" ? "checking" : "enabled";
          const isSortable = !readOnly && (moduleId !== "arksurvivalascended" || !asaRawFlagsActive) && canSortWorkbenchRow(row);
          const isDragging = draggedEnabledRow?.key === row.key;
          const isDropTarget = enabledDropTargetKey === row.key;
          const rowClassName = [
            isActive ? "mw-entry-row mw-entry-row--active" : "mw-entry-row",
            isSortable ? "mw-entry-row--sortable" : "",
            isDragging ? "mw-entry-row--dragging" : "",
            isDropTarget ? "mw-entry-row--drop-target" : ""
          ].filter(Boolean).join(" ");
          return (
            <div key={row.key} className={rowClassName}>
              <button
                type="button"
                className="mw-entry-row-select"
                aria-pressed={isActive}
                onClick={() => handleSelectEnabledRow(row.key)}
                onPointerDown={(event) => handleEnabledRowPointerDown(event, row)}
                onPointerEnter={() => handleEnabledRowPointerEnter(row)}
                onPointerUp={(event) => handleEnabledRowPointerUp(event, row)}
                onPointerCancel={handleEnabledRowPointerCancel}
                aria-grabbed={isDragging ? true : undefined}
              >
                <span className="mw-entry-row-media">
                  <SteamWorkshopPreview url={lookup?.preview_url} loading={!readOnly} />
                </span>
                <span className="mw-entry-row-copy">
                  <span className="mw-entry-row-title mw-entry-row-title--singleline">{title}</span>
                  {lookup?.description_excerpt ? <span className="mw-entry-row-excerpt">{lookup.description_excerpt}</span> : null}
                  <span className="mw-entry-row-status">
                    {!readOnly ? <WorkshopStatus state={lifecycle} /> : null}
                    <span className="mw-entry-id">{row.entry.label} / {id}</span>
                    {isArk ? <><span className="ark-mod-default-label">{sharedArkMember ? locale === "zh-CN" ? "共用默认" : "Shared defaults"
                      : locale === "zh-CN" ? "仅地图配置" : "Map configuration"}</span><span data-ark-mod-map-count>{arkMapCounts.counts.get(row.id) ?? 0}/{arkMapCounts.total}{locale === "zh-CN" ? " 地图" : " maps"}</span></> : null}
                  </span>
                </span>
              </button>
              <div className="mw-entry-row-actions">
                {sharedArkMember && canToggleModEnabledRow(row, moduleId, isManualEnablementModule) ? <input
                  className="mw-entry-enabled-toggle"
                  type="checkbox"
                  checked={!isDisabled}
                  aria-checked={controlState?.partiallyEnabled ? "mixed" : !isDisabled}
                  ref={(input) => { if (input) input.indeterminate = Boolean(controlState?.partiallyEnabled); }}
                  disabled={modEnablementDisabled || (controlState !== null && !controlState.canToggle)}
                  aria-label={(isArk ? locale === "zh-CN" ? "共用默认：" : "Shared defaults: " : "") + (isDisabled
                    ? t("servers.mods.configList.enable", { name: title }, "Enable {name}")
                    : t("servers.mods.configList.disable", { name: title }, "Disable {name}"))}
                  onChange={() => {
                    handleSelectEnabledRow(row.key);
                    if (moduleId === "arksurvivalascended") handleAsaModAction(id, isDisabled ? "enable" : "disable");
                    else if (controlState) handleSetCollectionMembersEnabled([id], isDisabled);
                    else if (isDisabled) handleSetDstModsEnabled([id], true);
                    else handleDisableEnabledRow(row);
                  }}
                /> : null}
                <button
                  type="button"
                  className="mw-entry-remove-button"
                  disabled={modEnablementDisabled || (moduleId === "terraria" && parseLineOrSemicolonEntries(settings.tmodloader_enabled_mod_names).length > 0)}
                  aria-label={t("servers.mods.configList.remove", { name: title }, "Remove {name} from this instance")}
                  title={t("servers.mods.removeFromInstance", undefined, "Remove from instance")}
                  onClick={() => {
                    handleSelectEnabledRow(row.key);
                    handleDeleteEnabledRow(row);
                  }}
                >
                  <ShellIcon name="x" className="mw-entry-remove-icon" />
                </button>
              </div>
            </div>
          );
        })}

        {showsManualInventory ? <ManualModInventoryList
          items={unconfiguredInventoryItems}
          selectedPath={activeDetailSource === "inventory" ? selectedInventoryItem?.path ?? null : null}
          canEnable={isManualEnablementModule} disabled={modMutationsDisabled}
          onSelect={(path) => { setSelectedInventoryPath(path); setActiveDetailSource("inventory"); }}
          onEnable={handleEnableManualInventoryItem}
          fileManagement={usesFileModManagement}
          onManagedToggle={(item) => managedManualMods.change(item, item.management?.enabled ? "disable" : "enable")}
          onManagedDelete={managedManualMods.requestDelete}
          onAdopt={(item) => managedManualMods.change(item, "adopt")}
          canRemove={(item) => {
            if (moduleId === "arksurvivalascended") return Boolean(canonicalAsaModId(item.inferred_id));
            const ids = inventoryWorkshopIds(item, manualInventory);
            return (moduleId === "palworld" && ids.some((id) => workshopControlStates.get(id)?.owned)) ||
              (moduleId === "squad" && ids.length === 1 && collectionMemberIds.includes(ids[0]));
          }}
          onRemove={(item) => moduleId === "arksurvivalascended"
            ? handleAsaModAction(item.inferred_id ?? "", "remove")
            : handleRemoveManagedWorkshopMembers(inventoryWorkshopIds(item, manualInventory)
              .filter((id) => moduleId !== "palworld" || workshopControlStates.get(id)?.owned))}
        /> : null}


      </div>
    );
  }

  function resolveActiveDetailSource(): "enabled" | "inventory" | null {
    const hasEnabledSelection = Boolean(selectedEnabledRow);
    const hasDisabledSelection = showsManualInventory && selectedInventoryItem !== null;

    return (
      activeDetailSource === "enabled" && hasEnabledSelection ? "enabled"
      : activeDetailSource === "inventory" && hasDisabledSelection ? "inventory"
      : hasEnabledSelection ? "enabled"
      : hasDisabledSelection ? "inventory"
      : null
    );
  }

  function renderInfoRows(rows: Array<{ label: string; value: string | number | null | undefined }>) {
    const visibleRows = rows.filter((row) => row.value !== null && row.value !== undefined && String(row.value).trim().length > 0);
    if (visibleRows.length === 0) {
      return null;
    }
    return (
      <dl className="mw-entry-info-grid">
        {visibleRows.map((row) => (
          <div key={row.label} className="mw-entry-info-row">
            <dt>{row.label}</dt>
            <dd>{row.value}</dd>
          </div>
        ))}
      </dl>
    );
  }

  function renderDstConfiguration(modId: string) {
    const item = lookupMap[modId];
    if (item && isUnsupportedWorkshopItem(item, expectedAppId)) {
      return <div className="workshop-status-banner is-warning" role="alert">
        {t("servers.mods.configPanel.unsupportedItem", undefined, "This entry is a Steam guide or other non-installable content. Remove it from this instance's Mod list.")}
      </div>;
    }
    return <DstWorkshopConfiguration
      instanceId={props.details.summary.id}
      settings={settings}
      selectedModId={modId}
      disabled={modMutationsDisabled}
      readOnly={readOnly}
      scanNonce={scanNonce}
      canInstallMissingMod={canInstallWorkshopItems([modId])}
      installingMissingMod={installingWorkshopIds.has(modId)}
      onInstallMissingMod={(id) => handleInstallWorkshopItems([id], undefined, "prepare")}
      onSettingsChange={(nextSettings) => launchModMutation("configuration", async () => { await persistSettings(nextSettings); })}
    />;
  }

  function renderSelectedModConfigPanel(memberId?: string) {
    const selection = memberId ? resolveWorkshopContentSelection([memberId]) : null;
    const selectedRow = selection ? selection.row : selectedEnabledRow;
    const inventoryItem = selection ? selection.inventoryItem : selectedInventoryItem;
    const resolvedDetailSource = selection
      ? selectedRow ? "enabled" : inventoryItem ? "inventory" : null
      : resolveActiveDetailSource();

    if (!resolvedDetailSource) {
      return (
        <div className="mw-empty">
          <span>{t("servers.mods.emptyEntries", undefined, "No configured mod entries yet.")}</span>
        </div>
      );
    }

    const asaConfigId = canonicalAsaModId(resolvedDetailSource === "enabled"
      ? selectedRow?.id ?? "" : inventoryItem?.inferred_id ?? "");
    if (isArk && asaConfigId && !arkProjection.error) {
      return <div className="mw-selected-config-panel"><ArkModMapConfiguration
        key={props.details.summary.id} instanceId={props.details.summary.id} instanceName={props.details.summary.name} moduleId={moduleId} modId={asaConfigId} settings={settings}
        disabled={modMutationsDisabled} readOnly={readOnly}
        installed={manualInventoryItems.some((item) => item.inferred_id === asaConfigId && item.file_count > 0)}
        onChange={saveArkModChange}
        onReload={async () => {
          let fresh = settings;
          await runModMutation("configuration", async () => { fresh = (await readWritableInstanceState()).settings; });
          return fresh;
        }}
      /></div>;
    }
    if (resolvedDetailSource === "inventory") {
      return (
        <div className="mw-selected-config-panel">
          {inventoryItem ? renderInfoRows([
            { label: t("servers.mods.infoDialog.files", undefined, "Files"), value: inventoryItem.file_count },
            { label: t("servers.mods.storeDetail.fileSize", undefined, "File size"), value: formatWorkshopByteSize(inventoryItem.total_bytes) },
            { label: t("servers.mods.infoDialog.directory", undefined, "Directory"), value: inventoryItem.path }
          ]) : null}
        </div>
      );
    }

    if (!selectedRow) {
      return (
        <div className="mw-empty">
          <span>{t("servers.mods.emptyEntries", undefined, "No configured mod entries yet.")}</span>
        </div>
      );
    }

    const selectedEntry = selectedRow.entry;
    const selectedId = selectedRow.id;
    const emptyConfigMessage = selectedEntry.kind === "steam-item" || selectedEntry.kind === "steam-collection"
      ? t("servers.mods.configPanel.downloadOnly", undefined, "This entry controls downloads or collection membership. Select an enabled mod row to edit per-mod settings.")
      : t("servers.mods.configPanel.noEditor", undefined, "The manager does not yet provide a configuration panel for this mod. See the configuration instructions provided by the mod author.");

    return moduleId === "dontstarve" && selectedId && selectedEntry.kind !== "steam-collection"
      ? <div className="mw-selected-config-panel">{renderDstConfiguration(selectedId)}</div>
      : <div className="mw-empty"><span>{emptyConfigMessage}</span></div>;
  }

  function collectionMemberAdded(id: string): boolean {
    if (readOnly) return dstRowsById.has(id) || configuredSteamIdSet.has(id) || collectionMemberIds.includes(id);
    if (moduleId === "dontstarve") return dstRowsById.has(id);
    if (moduleId === "palworld" || supportsWorkshopModEnablement(moduleId)) return Boolean(workshopControlStates.get(id)?.owned);
    return configuredSteamIdSet.has(id);
  }

  function collectionRepairIds(collection: ManagedWorkshopCollection): string[] {
    return collection.member_ids.length ? collection.member_ids.filter((id) => !collectionMemberAdded(id)) : [collection.id];
  }

  function workshopControlError(error: unknown): Error {
    if (!(error instanceof WorkshopControlError) && !(error instanceof WorkshopInventoryControlError)) {
      return error instanceof Error ? error : new Error(describeError(error));
    }
    const key = error.code === "not-owned" ? "servers.mods.removeSummary.notConfigured"
      : error.code === "ownership-limit" ? "servers.mods.removeSummary.ownershipLimit"
      : error.code === "local-metadata-missing" ? "servers.mods.controls.localMetadata"
      : `servers.mods.collections.removalError.${error.code === "invalid-settings" ? "invalid-records" : error.code}`;
    return new Error(t(key, { ids: error.ids.join(", ") }));
  }

  function assertWorkshopControlBase(base: SettingsObject, current: SettingsObject) {
    const keys = ["steam_workshop_collections", "steam_workshop_disabled_mod_ids", "steam_workshop_removed_mod_ids",
      "workshop_items", "mods", "map_name", "active_mod_ids", "auto_managed_mod_ids", "mod_workshop_ids", "mod_package_names",
      "workshop_file_ids", "tmodloader_workshop_item_ids", "tmodloader_enabled_mod_names"];
    if (changedSettingKeys(base, current, keys).length) {
      throw new Error(t("servers.mods.collections.conflict", undefined, "The instance's collections changed elsewhere. Reload and retry."));
    }
  }

  function handleRemoveManagedWorkshopMembers(ids: string[], collectionId?: string) {
    if (moduleId !== "palworld" && moduleId !== "squad") { handleRemoveWorkshopItems(ids); return; }
    launchModMutation("enablement", async () => {
      const latest = await readWritableInstanceState();
      try {
        if (moduleId === "squad") {
          const collection = readManagedWorkshopCollections(latest.settings, moduleId)
            .find((entry) => (!collectionId || entry.id === collectionId) && ids.every((id) => entry.member_ids.includes(id)));
          if (!collection || ids.length !== 1) throw new WorkshopControlError("not-owned", ids);
          await persistSettings(latest.settings, latest.settings,
            { collectionId: collection.id, memberIds: ids, retainCollection: true });
        } else {
          const inventory = await readManualModInventory(props.details.summary.id);
          const plan = buildPalworldWorkshopPlan(latest.settings, inventory, ids, "remove");
          await persistSettings(plan.nextSettings, latest.settings, undefined,
            (current) => assertWorkshopControlBase(latest.settings, current));
        }
        setApplyMessage(null);
        setScanNonce((value) => value + 1);
      } catch (error) { throw workshopControlError(error); }
    });
  }

  function handleSetCollectionMembersEnabled(ids: string[], enabled: boolean) {
    if (moduleId === "dontstarve") {
      handleSetDstModsEnabled(ids, enabled);
      return;
    }
    if ((!supportsWorkshopModEnablement(moduleId) && moduleId !== "palworld") || !ids.length) return;
    launchModMutation("enablement", async () => {
      const latest = await readWritableInstanceState();
      try {
        const snapshot = moduleId === "projectzomboid" ? await readProjectZomboidWorkshopModsSnapshot(props.details.summary.id,
          uniqueEntries([...parseWorkshopIdList(latest.settings.workshop_items), ...ids])) : null;
        const plan = moduleId === "palworld"
          ? buildPalworldWorkshopPlan(latest.settings, await readManualModInventory(props.details.summary.id), ids, enabled ? "enable" : "disable")
          : buildWorkshopModEnablementPlan(moduleId, latest.settings, ids, enabled, snapshot);
        await persistSettings(plan.nextSettings, latest.settings, undefined,
          (current) => assertWorkshopControlBase(latest.settings, current));
        if (snapshot) setPzSnapshot(snapshot);
        setApplyMessage(plan.retainedLocalIds.length ? t("servers.mods.collections.retainedLocalIds", { ids: plan.retainedLocalIds.join(", ") }) : null);
        setScanNonce((current) => current + 1);
      } catch (error) { throw workshopControlError(error); }
    });
  }

  function collectionMemberState(id: string) {
    if (moduleId === "dontstarve") {
      const row = dstRowsById.get(id);
      return { added: Boolean(row), enabled: row?.entry.key === "dst-enabled", canToggle: Boolean(row), canRemove: Boolean(row) };
    }
    if (moduleId === "palworld" || supportsWorkshopModEnablement(moduleId)) {
      const state = workshopControlStates.get(id) ?? { owned: false, enabled: false, partiallyEnabled: false, canToggle: false };
      return { added: state.owned, enabled: state.enabled, partiallyEnabled: state.partiallyEnabled,
        canToggle: state.canToggle, canRemove: state.owned };
    }
    const added = collectionMemberAdded(id);
    return { added, enabled: false, canToggle: false,
      canRemove: added && (moduleId === "squad" || moduleId === "unturned" ||
        (moduleId === "terraria" && !parseLineOrSemicolonEntries(settings.tmodloader_enabled_mod_names).length)) };
  }

  function renderCollectionList(selectedMemberId: string | null) {
    return <WorkshopCollectionLibrary
      collections={managedCollections} selectedId={selectedManagedCollection?.id ?? null} selectedMemberId={selectedMemberId} lookup={lookupMap}
      busy={readOnly ? false : modMutationsDisabled || collectionRemoval.busy} readOnly={readOnly}
      loading={lookupLoading} error={lookupError ? formatDesktopError(t, lookupError) : null}
      getMemberState={collectionMemberState}
      enablementDisabled={modEnablementDisabled}
      onToggleMembers={handleSetCollectionMembersEnabled}
      onRemoveMember={(id, collectionId) => handleRemoveManagedWorkshopMembers([id], collectionId)}
      canRepair={(collection) => {
        const ids = collectionRepairIds(collection);
        return ids.length > 0 && ids.every((id) => lookupMap[id]?.status === "resolved" && steamItemBelongsToApp(lookupMap[id], expectedAppId))
          && canInstallWorkshopItems(ids);
      }}
      onSelect={(id) => { setSelectedCollectionId(id); setSelectedCollectionMember(null); }}
      onRetry={() => setLookupRevision((value) => value + 1)}
      onRepair={(collection) => { void handleInstallWorkshopItems(collectionRepairIds(collection)); }}
      onOpenMember={(id, collectionId) => {
        if (readOnly) {
          setSelectedCollectionId(collectionId);
          setSelectedCollectionMember({ collectionId, modId: id });
          return;
        }
        if (collectionMemberAdded(id)) {
          setSelectedCollectionId(collectionId);
          setSelectedCollectionMember({ collectionId, modId: id });
        } else {
          setBrowseKind("item");
          openWorkshopDetails(id);
          setActiveWorkbenchView("store");
        }
      }}
      onRemove={(id) => { void collectionRemoval.open(id); }}
    />;
  }

  function renderConfigurationPane() {
    const collectionsMode = !manifestMode && browseKind === "collection" && isSteamWorkshopModule;
    const selectedMemberId = selectedManagedCollection && selectedCollectionMember
      && selectedCollectionMember.collectionId === selectedManagedCollection.id
      && managedCollectionMemberIds(selectedManagedCollection, lookupMap[selectedManagedCollection.id]).includes(selectedCollectionMember.modId)
      && collectionMemberAdded(selectedCollectionMember.modId)
      ? selectedCollectionMember.modId : null;
    const disabledCount = showsManualInventory ? unconfiguredInventoryItems.length : 0;
    const modCount = configuredEntryCount + disabledCount;
    const empty = !manifestMode && (collectionsMode ? managedCollections.length === 0 : enabledRows.length === 0 && disabledCount === 0);
    const selectedName = collectionsMode ? selectedMemberId ? lookupMap[selectedMemberId]?.title ?? selectedMemberId : null : resolveActiveDetailSource() === "inventory"
      ? selectedInventoryItem?.name
      : selectedEnabledRow ? lookupMap[selectedEnabledRow.id]?.title ?? selectedEnabledRow.value : null;
    const settingsLabel = t("servers.mods.configPanel.label", undefined, "Mod settings");
    const acceptsDrop = Boolean(manualStaging) && !readOnly && !manifestMode;
    const allowReferences = !isSteamWorkshopModule && canUseManualReferences;
    const dropActive = acceptsDrop && manualDropActive && !modMutationsDisabled;

    return (
      <div className="mw-config-pane">
        <div className={`mw-detail-layout${manifestMode ? " mw-detail-layout--manifest" : empty ? " mw-detail-layout--empty" : ""}`}>
          <div className={`mw-detail-list-col${acceptsDrop ? " mw-dropzone" : ""}${dropActive ? " mw-dropzone--active" : ""}${acceptsDrop && manualStageState === "running" ? " mw-dropzone--running" : ""}`}
            {...(acceptsDrop ? manualListDropHandlers(allowReferences) : {})}
            role={acceptsDrop ? "region" : undefined}
            aria-label={acceptsDrop ? t("servers.mods.manualDropTitle", undefined, "Drop downloaded mods here") : undefined}
            aria-disabled={acceptsDrop ? modMutationsDisabled : undefined}
            aria-busy={acceptsDrop ? manualStageState === "running" || manualReferenceState === "running" : undefined}
          >
            {manifestMode ? null : <div className="mw-detail-col-title">
              <span>{collectionsMode ? t("servers.mods.browseKind.collection", undefined, "Collections") : t("servers.mods.detailTabs.mods", undefined, "Mods")}</span>
              <span className="mw-entry-count-chip">{collectionsMode ? managedCollections.length : modCount}</span>
              {!collectionsMode && showsManualInventory && manualInventory?.target_exists ? <button type="button" className="mw-ghost-btn"
                title={manualInventory.target_path}
                onClick={() => void openLocalPath(manualInventory.target_path).catch((error) => setMutationError(describeError(error)))}>
                <ShellIcon name="folder" className="mw-btn-icon" />{t("servers.mods.openFolder", undefined, "Open Mod folder")}
              </button> : null}
            </div>}
            <div className="mw-detail-panel">{manifestMode && moduleId === "arksurvivalascended" ? <AsaModManifestPanel
              key={props.details.summary.id} instanceId={props.details.summary.id}
              currentIds={sharedAsaMembership.active} initialText={manifestDraft} onDraftChange={setManifestDraft}
              readOnly={readOnly}
              blockedReason={asaRawFlagsActive ? t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING)
                : modChangesBlocked ? t("servers.mods.stopBeforeChanges", undefined, "Stop the instance before changing Mods.")
                  : arkProjection.error ? t("servers.mods.asaManifest.mapSettingsInvalid")
                    : mutationBusy || steamAccount?.busy ? t("servers.mods.mutationBusy", undefined, "Another Mod change is still in progress.")
                      : null}
              onSave={(ids, expected) => runModMutation("enablement", () => applyAsaActiveList(ids, expected, props.details.summary.id))}
            /> : manifestMode && expectedAppId ? <WorkshopManifestPanel
              key={props.details.summary.id} instanceId={props.details.summary.id} appId={expectedAppId}
              initialText={manifestDraft} onDraftChange={setManifestDraft}
              currentIds={moduleId === "arksurvivalevolved" ? parseEvolvedModIds(settings.active_mod_ids)
                : uniqueEntries([...workshopIds, ...(moduleId === "dontstarve" ? enabledRows.map((row) => row.id) : [])])} disabled={modMutationsDisabled}
              readOnly={readOnly}
              targetNote={t(`servers.mods.manifest.target.${moduleId}`, undefined, "")}
              allowCachedInstall={Boolean(manualStaging)}
              enablementNote={moduleId === "terraria" ? t("servers.mods.manifest.needsNames")
                : moduleId === "squad" ? t("servers.mods.manifest.downloadOnly") : undefined}
              onApply={(review, enable) => handleInstallWorkshopItems(review.contentIds, { review, enable })}
            /> : collectionsMode ? renderCollectionList(selectedMemberId) : renderModList()}</div>
            {dropActive ? <div className="mw-dropzone-feedback" aria-hidden="true">
                <ShellIcon name="package" className="mw-btn-icon" />
                <strong>{manualDropReferenceActive ? t("servers.mods.referenceDropTitle", { source: manualSourceLabel }, "Drop {source} links or IDs here")
                  : t("servers.mods.manualDropTitle", undefined, "Drop downloaded mods here")}</strong>
            </div> : null}
          </div>
          {!manifestMode && !empty ? <div className="mw-detail-config-col" role="region" aria-label={selectedName ? `${settingsLabel}: ${selectedName}` : settingsLabel}>
            <div className="mw-detail-config-panel">
              {!manifestMode && !empty ? collectionsMode ? selectedMemberId ? renderSelectedModConfigPanel(selectedMemberId) : <div className="mw-empty">
                <span>{t("servers.mods.collections.selectMember", undefined, "Select a Mod from a collection to view its settings.")}</span>
              </div> : renderSelectedModConfigPanel() : null}
            </div>
          </div> : null}
        </div>
        {renderManualImportFeedback()}
      </div>
    );
  }

  function renderSteamWorkshopToolbar() {
    return (
      <div className="mw-toolbar mw-workspace-toolbar">
        <label className="mw-search-field">
          <ShellIcon name="search" className="mw-search-icon" />
          <input
            className="mw-search-input"
            disabled={readOnly}
            value={browseQuery}
            aria-label={t("servers.mods.storeSearchPlaceholder", undefined, "Search by Workshop name or ID...")}
            placeholder={t("servers.mods.storeSearchPlaceholder", undefined, "Search by Workshop name or ID...")}
            onChange={(event) => {
              const next = event.target.value;
              if (!browseQuery.trim() && next.trim()) setBrowseSort("relevance");
              if (!next.trim() && browseSort === "relevance") setBrowseSort("trend");
              setBrowseQuery(next);
              setBrowsePage(1);
              setActiveWorkbenchView("store");
              openWorkshopDetails(null);
            }}
          />
        </label>
        <div className="mw-sort-pills" role="tablist" aria-label={t("servers.mods.mainTabs.aria", undefined, "Mod workspace")}>
          {([...(browseQuery.trim() ? ["relevance"] : []), "trend", "popular", "recent", ...(browseKind === "item" ? ["subscribers"] : [])] as SteamWorkshopBrowseSort[]).map((sort) => (
            <button
              key={sort}
              type="button"
              role="tab"
              aria-selected={activeWorkbenchView === "store" && browseSort === sort}
              disabled={readOnly}
              className={activeWorkbenchView === "store" && browseSort === sort ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
              onClick={() => {
                setActiveWorkbenchView("store");
                setBrowseSort(sort);
                setBrowsePage(1);
                openWorkshopDetails(null);
              }}
            >
              {t(`servers.mods.sort.${sort}`, undefined, formatBrowseSortLabel(sort))}
            </button>
          ))}
          <button
            type="button"
            role="tab"
            aria-selected={activeWorkbenchView === "config" && !manifestMode}
            className={activeWorkbenchView === "config" && !manifestMode ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
            onClick={() => {
              setActiveWorkbenchView("config");
              setManifestMode(false);
            }}
          >
            {browseKind === "collection" ? t("servers.mods.collections.my", undefined, "My collections") : t("servers.mods.mainTabs.config", undefined, "My Mods")}
          </button>
          {expectedAppId && workflow.steamDownloadMode === "steamcmd-cache" ? <button
            type="button"
            role="tab"
            aria-selected={activeWorkbenchView === "config" && manifestMode}
            className={activeWorkbenchView === "config" && manifestMode ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
            disabled={mutationBusy || readOnly}
            onClick={() => {
              setActiveWorkbenchView("config");
              setManifestMode(true);
            }}
          >
            {t("servers.mods.manifest.mode")}
          </button> : null}
        </div>
        <div className="mw-sort-pills mw-browse-kind" role="group" aria-label={t("servers.mods.browseKind.label", undefined, "Workshop content type")}>
          {(["item", "collection"] as const).map((kind) => (
            <button
              key={kind}
              type="button"
              aria-pressed={browseKind === kind}
              className={browseKind === kind ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
              onClick={() => {
                setBrowseKind(kind);
                if (kind === "collection" && browseSort === "subscribers") setBrowseSort("trend");
                setBrowsePage(1);
                openWorkshopDetails(null);
                if (activeWorkbenchView !== "config" || manifestMode) setActiveWorkbenchView("store");
                setManifestMode(false);
              }}
            >
              {t(`servers.mods.browseKind.${kind}`, undefined, kind === "item" ? "Mods" : "Collections")}
            </button>
          ))}
        </div>
      </div>
    );
  }

  function renderSteamWorkshopWorkspace() {
    const detailState = selectedBrowseWorkshopItem
      ? resolveWorkshopStoreItemState(selectedBrowseWorkshopItem)
      : null;
    const detailHasMismatch = selectedBrowseWorkshopItem
      ? !steamItemBelongsToApp(selectedBrowseWorkshopItem, expectedAppId)
      : false;

    return (
      <div className="mw-steam-workspace">
        {renderSteamWorkshopToolbar()}
        {downloadState === "running" || activeWorkshopJob ? <ActivityNotice>
          {(activeWorkshopJob?.detail || t("servers.mods.downloadPreparing", undefined, "Preparing Mod download…")) + (activeProgressPercent !== null ? ` ${activeProgressPercent}%` : "")}
        </ActivityNotice> : null}
        {activeWorkbenchView === "config" ? renderConfigurationPane() : (
          <div className={selectedBrowseWorkshopItem ? "mw-store-layout mw-store-layout--detail" : "mw-store-layout"}>
            <div className={catalogItems.length === 0 && browseResult && !browseLoading && !browseError ? "mw-browse-pane mw-browse-pane--empty" : "mw-browse-pane"} ref={browsePaneRef}>
          {browseLoading ? <ActivityNotice>{t("servers.mods.loadingPage", { page: browsePage }, "Loading page {page}…")}</ActivityNotice> : null}
          {browseResult && !browseError ? <div className="mw-browse-status" role="status">
            {t("servers.mods.resultSummary", { count: browseResult.total_count ?? browseResult.items.length, sort: t(`servers.mods.sort.${browseSort}`, undefined, formatBrowseSortLabel(browseSort)) }, "{count} results · {sort}")}
          </div> : null}
          {browseError ? <ActivityNotice tone="error" action={
            <button className="mw-ghost-btn" type="button" onClick={retryBrowse}>{t("common.retry", undefined, "Retry")}</button>
          }>{t("servers.mods.browseFailed", { error: formatDesktopError(t, browseError) }, "Could not load Workshop results: {error}")}</ActivityNotice> : null}
          {lookupError ? <ActivityNotice tone="error">{formatDesktopError(t, lookupError)}</ActivityNotice> : null}
          {catalogItems.length > 0 ? (
            <div className={catalogItems.length === browseResult?.page_size ? "mw-mod-grid mw-mod-grid--full-page" : "mw-mod-grid"}
              aria-busy={browseLoading || lookupLoading || workshopInstallationLoading}>
              {catalogItems.map((item) => {
                const id = item.id;
                const hasMismatch = !steamItemBelongsToApp(item, expectedAppId);
                const isOpen = selectedBrowseWorkshopId === id;
                const itemState = resolveWorkshopStoreItemState(item);
                const collectionSummary = isCollectionSummary(item);
                const quickActionLabel = collectionSummary
                  ? t("servers.mods.storeDetail.openDetails", { name: item.title ?? id }, "View details for {name}")
                  : itemState.action === "manage"
                    ? t("servers.mods.storeDetail.added", undefined, "Added to instance; manage")
                    : t("servers.mods.storeDetail.install", undefined, "Install");
                const actionDisabled = !collectionSummary && itemState.action !== "manage" && !canInstallWorkshopItems([id]);
                const cardClassName = [
                  "mw-mod-card",
                  hasMismatch ? "mw-mod-card--warn" : "",
                  isOpen ? "mw-mod-card--open" : "",
                  itemState.installationState === "installed" ? "mw-mod-card--installed" : "",
                  itemState.lifecycleState === "installing" ? "mw-mod-card--installing" : ""
                ].filter(Boolean).join(" ");

                return (
                  <article key={id} className={cardClassName}>
                    <button
                        type="button"
                        className={itemState.action === "manage" ? "mw-mod-quick-action mw-mod-quick-action--added" : "mw-mod-quick-action"}
                        aria-label={collectionSummary ? quickActionLabel : `${quickActionLabel}: ${item.title ?? id}`}
                        title={quickActionLabel}
                        disabled={hasMismatch || actionDisabled}
                        onClick={() => collectionSummary ? openWorkshopDetails(id) : handleWorkshopStoreAction(item, itemState.action)}
                      >
                        <ShellIcon
                          name={collectionSummary ? "chevron-right" : itemState.lifecycleState === "installing" ? "loader" : itemState.action === "manage" ? "check" : "plus"}
                          className={itemState.lifecycleState === "installing"
                            ? "mw-mod-quick-action-icon mw-btn-icon--spin"
                            : "mw-mod-quick-action-icon"}
                        />
                    </button>
                    <button
                      type="button"
                      className="mw-mod-thumb"
                      aria-label={t("servers.mods.storeDetail.openDetails", { name: item.title ?? id }, "View details for {name}")}
                      onClick={() => openWorkshopDetails(id)}
                    >
                      <SteamWorkshopPreview url={item.preview_url} loading />
                    </button>
                    {itemState.lifecycleState === "installing" ? (
                      <div className="mw-mod-card-progress" role="progressbar" aria-label={t("servers.mods.installRunning", undefined, "Installing…")} aria-valuenow={activeProgressPercent ?? undefined}>
                        <div
                          className={activeProgressPercent !== null ? "mw-mod-card-progress-bar mw-mod-card-progress-bar--determinate" : "mw-mod-card-progress-bar"}
                          style={activeProgressPercent !== null ? { width: `${Math.max(6, Math.min(100, activeProgressPercent))}%` } : undefined}
                        />
                      </div>
                    ) : null}
                    <div className="mw-mod-card-body">
                      <button type="button" className="mw-mod-card-title" onClick={() => openWorkshopDetails(id)}>
                        {item.title ?? t("servers.mods.workshopItemFallback", { id }, "Workshop item {id}")}
                      </button>
                      {item.description_excerpt ? <p className="mw-mod-card-excerpt">{item.description_excerpt}</p> : null}
                      {item.tags && item.tags.length > 0 ? (
                        <div className="mw-mod-card-tags">
                          {item.tags.slice(0, 4).map((tag) => <span key={tag}>{tag}</span>)}
                        </div>
                      ) : null}
                      <div className="mw-mod-chips">
                        {!collectionSummary ? <WorkshopStatus state={itemState.lifecycleState} progressPercent={activeProgressPercent} /> : null}
                        {item.child_count > 0 ? <span className="mw-chip">{t("servers.mods.includesCount", { count: item.child_count }, "Includes {count}")}</span> : null}
                        {hasMismatch ? <span className="mw-chip mw-chip--warn">{t("servers.mods.wrongGame", undefined, "Wrong game")}</span> : null}
                        {typeof item.subscriptions === "number" ? (
                          <span className="mw-mod-card-subscriptions">
                            {t("servers.mods.storeDetail.subscriberCount", { count: formatCompactCount(locale, item.subscriptions) }, "{count} subscribers")}
                          </span>
                        ) : <span className="mw-mod-card-id">#{id}</span>}
                      </div>
                    </div>
                  </article>
                );
              })}
            </div>
          ) : browseResult && !browseLoading && !browseError ? (
            <div className="mw-empty">
              <ShellIcon name="inbox" className="mw-empty-icon" />
              <span>
                {browsedWorkshopItems.length > 0
                  ? t("servers.mods.emptyServerBrowse")
                  : browseQuery.trim()
                  ? browseKind === "collection"
                    ? t("servers.mods.noCollectionSearchResults", undefined, "No matching collections. Try another name or paste a collection URL or ID.")
                    : t("servers.mods.noSearchResults", undefined, "No matching Mods. Try another name or paste a Workshop URL or ID.")
                  : t(`servers.mods.emptyBrowse.${browseKind}`, undefined, browseKind === "collection"
                    ? "No collections are available in this Workshop view."
                    : "No Mods are available in this Workshop view.")}
              </span>
              {!browseQuery.trim() ? <button className="mw-ghost-btn" type="button" onClick={retryBrowse}>{t("common.retry", undefined, "Retry")}</button> : null}
            </div>
          ) : null}

          {browseResult ? (
            <div className="mw-pager">
              <button type="button" className="mw-ghost-btn" disabled={browsePage <= 1 || browseLoading} onClick={() => { setBrowsePage(Math.max(1, browseResult.page - 1)); openWorkshopDetails(null); }}>
                {t("servers.mods.previousPage", undefined, "Previous")}
              </button>
              <span className="mw-pager-label" role="status">
                {browseLoading ? <><ShellIcon name="loader" className="mw-btn-icon mw-btn-icon--spin" />{t("servers.mods.loadingPage", { page: browsePage }, "Loading page {page}…")}</>
                  : t("servers.mods.pageLabel", { page: browseResult.page }, "Page {page}")}
              </span>
              <button type="button" className="mw-ghost-btn" disabled={browseLoading || Boolean(browseError) || !browseResult.has_more} onClick={() => { setBrowsePage(browseResult.page + 1); openWorkshopDetails(null); }}>
                {t("servers.mods.nextPage", undefined, "Next")}
              </button>
            </div>
          ) : null}

            </div>

            {selectedBrowseWorkshopItem && detailState ? (
              <SteamWorkshopStoreDetail
                item={selectedBrowseWorkshopItem}
                detailsReady={selectedBrowseDetailsReady}
                localizationWarningCovered={verificationWarning?.includesOriginalText}
                detailsError={detailError ? formatDesktopError(t, detailError) : null}
                onRetryDetails={() => {
                  if (selectedBrowseWorkshopId) detailsRequests.current.invalidate(selectedBrowseWorkshopId, locale);
                  setDetailFailure(null);
                  setDetailRevision((value) => value + 1);
                }}
                lifecycleState={!selectedBrowseDetailsReady ? (detailError ? "unknown" : "checking") : detailState.lifecycleState}
                action={detailState.action}
                actionBusy={detailState.lifecycleState === "installing"}
                progressPercent={activeProgressPercent}
                actionDisabled={detailHasMismatch || !selectedBrowseDetailsReady || (detailState.action === "manage"
                  ? false
                  : !canInstallWorkshopItems([selectedBrowseWorkshopItem.id]))}
                onAction={() => handleWorkshopStoreAction(selectedBrowseWorkshopItem, detailState.action)}
                onOpenChild={openWorkshopChild}
                onBack={detailHistory.length ? returnToWorkshopParent : undefined}
                onClose={() => openWorkshopDetails(null)}
                onOpenExternal={() => void openExternalUrl(selectedBrowseWorkshopItem.detail_url)}
              />
            ) : null}
          </div>
        )}
      </div>
    );
  }

  function renderCommunityWorkspace() {
    const versionPolicyLabel = t(thunderstoreVersionPolicy === "pinned"
      ? "servers.mods.thunderstore.pinned" : "servers.mods.thunderstore.followLatest");
    return (
      <div className={`mw-community-workspace${nexusSource ? " mw-community-workspace--nexus" : ""}`}>
        <div className="mw-workspace-toolbar mw-manual-toolbar">
          {canUseManualReferences ? <div className="mw-reference-row">
            <input className="mw-reference-input" value={manualReferenceInput} disabled={modEnablementDisabled || (nexusSource && !nexusLocal)}
              aria-label={t("servers.mods.referenceInputPlaceholder", { source: manualSourceLabel }, "Paste {source} link or mod ID...")}
              placeholder={t("servers.mods.referenceInputPlaceholder", { source: manualSourceLabel }, "Paste {source} link or mod ID...")}
              onChange={(event) => setManualReferenceInput(event.target.value)}
              onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); handleEnableManualReferenceInput(); } }} />
            <button type="button" className="mw-btn mw-btn--secondary" disabled={modEnablementDisabled || (nexusSource && !nexusLocal)} onClick={handleEnableManualReferenceInput}>
              {manualEnablement ? t("servers.mods.applyAction.enableLocal", undefined, "Enable") : t("servers.mods.applyAction.install", undefined, "Install")}
            </button>
          </div> : <span className="mw-manual-toolbar-title">{manualSourceLabel}</span>}
          <div className="mw-manual-toolbar-actions">
          {canInstallSiteReferences && manualSourceProvider === "thunderstore" ? <ConfigurationHelp
            description={t("servers.mods.thunderstore.versionPolicyHelp")}>
            {(help) => <button type="button" className="mw-btn mw-btn--secondary mw-version-policy-btn"
              ref={help.anchorRef} {...help.interactionProps} aria-describedby={help.descriptionId}
              aria-label={`${t("servers.mods.thunderstore.versionPolicy")}: ${versionPolicyLabel}`}
              data-version-policy={thunderstoreVersionPolicy} disabled={modEnablementDisabled}
              onClick={() => setThunderstoreVersionPolicy((current) => current === "pinned" ? "follow_latest" : "pinned")}>
              <ShellIcon name={thunderstoreVersionPolicy === "pinned" ? "lock" : "refresh"} className="mw-btn-icon" />
              <span>{versionPolicyLabel}</span>
            </button>}
          </ConfigurationHelp> : null}
          <div className="mw-sort-pills" role="tablist" aria-label={t("servers.mods.mainTabs.aria", undefined, "Mod workspace")}>
            {MOD_WORKBENCH_TABS.map((tab) => (
              <button
                key={tab.id}
                type="button"
                role="tab"
                disabled={readOnly && tab.id === "store"}
                aria-selected={activeWorkbenchView === tab.id && !manifestMode}
                className={activeWorkbenchView === tab.id && !manifestMode ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
                onClick={() => { setActiveWorkbenchView(tab.id); setManifestMode(false); }}
              >
                {t(tab.labelKey, undefined, tab.fallback)}
              </button>
            ))}
            {moduleId === "arksurvivalascended" ? <button
              type="button" role="tab"
              aria-selected={activeWorkbenchView === "config" && manifestMode}
              className={activeWorkbenchView === "config" && manifestMode ? "mw-sort-pill mw-sort-pill--active" : "mw-sort-pill"}
              disabled={mutationBusy || readOnly}
              onClick={() => { setActiveWorkbenchView("config"); setManifestMode(true); }}
            >{t("servers.mods.manifest.mode")}</button> : null}
          </div>
          {nexusSource && nexusLocal ? <button type="button" className="mw-btn mw-btn--secondary mw-nexus-auth-btn" data-nexus-auth-open
            data-nexus-authorized={nexusAccount ? String(nexusAccount.configured) : "unknown"}
            disabled={modMutationsDisabled || !nexusLocal} aria-label={nexusText(locale, "auth")}
            onClick={() => setNexusCredentialsOpen(true)}><ShellIcon name={nexusAccount?.configured ? "check-circle" : "lock"} className="mw-btn-icon" />{nexusText(locale, "key")}</button> : null}
          {manualSourceUrl ? <button type="button" className="mw-btn mw-btn--secondary mw-source-open-btn" onClick={() => void openExternalUrl(manualSourceUrl)}>
            <ShellIcon name="external-link" className="mw-btn-icon" />
            <span>{t("servers.mods.openSource", { source: manualSourceLabel }, "Open {source}")}</span>
          </button> : null}
          </div>
        </div>
        {activeWorkbenchView === "store" ? renderManualImportPane() : renderConfigurationPane()}
      </div>
    );
  }

  function manualListDropHandlers(allowReferences: boolean) {
    allowReferences = allowReferences && (!nexusSource || nexusLocal);
    const updateFeedback = (event: DragEvent<HTMLElement>) => {
      event.preventDefault();
      const effect = modMutationsDisabled ? "none" : modDropEffect(event.dataTransfer, allowReferences);
      event.dataTransfer.dropEffect = effect;
      setManualDropActive(event.type !== "drop" && effect !== "none");
      setManualDropReferenceActive(allowReferences && (effect === "link" || (!Array.from(event.dataTransfer.types).includes("Files")
        && !Array.from(event.dataTransfer.items).some((item) => item.kind === "file"))));
      return effect;
    };
    return {
      onDragEnter: updateFeedback,
      onDragOver: updateFeedback,
      onDragLeave: (event: DragEvent<HTMLElement>) => {
        if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) setManualDropActive(false);
      },
      onDrop: (event: DragEvent<HTMLElement>) => {
        const effect = updateFeedback(event);
        if (effect === "none") return;
        const references = allowReferences ? referencesFromDomDrop(event, nexusSource ? nexusReferencesFromText : referenceCandidatesFromText) : [];
        const files = Array.from(event.dataTransfer.files);
        const fileItems = Array.from(event.dataTransfer.items).filter((item) => item.kind === "file");
        const browserLinkFiles = files.length > 0 && files.length === fileItems.length
          && files.every((file) => /\.url$/i.test(file.name) || file.type.startsWith("image/"))
          && fileItems.every((item) => !item.webkitGetAsEntry?.()?.isDirectory);
        const webReferences = references.filter((reference) => /^https?:\/\//i.test(reference));
        if (effect === "link" || (browserLinkFiles && webReferences.length > 0)) {
          void handleEnableManualReferences(effect === "link" ? references : webReferences);
          return;
        }
        // Local Mod packages and folders retain precedence over incidental text.
        if (files.length || fileItems.length) { void handleManualFileDrop(event.dataTransfer); return; }
        if (references.length > 0 && allowReferences) { void handleEnableManualReferences(references); return; }
        if (!allowReferences) { void handleStageManualModPaths([]); return; }
        void handleEnableManualReferences([]);
      }
    };
  }

  function renderManualImportFeedback() {
    const nexusLocalNote = nexusSource && !nexusLocal && !readOnly ? <p className="mw-nexus-local-note">{nexusText(locale, "manualImportHelp")}</p> : null;
    if (!manualStageResult && !manualReferenceResult && !manualReferenceError && !manualStageError && manualStageState !== "running" && manualReferenceState !== "running") return nexusLocalNote;
    return <div className="mw-manual-feedback">
      {nexusLocalNote}
      {manualStageState === "running" ? <ActivityNotice>{t("servers.mods.manualDropRunning", undefined, "Installing...")}</ActivityNotice> : null}
      {manualReferenceState === "running" && manualStageState !== "running" ? <ActivityNotice>{t("servers.mods.referenceResolving", undefined, "Resolving...")}</ActivityNotice> : null}
      {manualStageResult ? <div className="mw-manual-success-bar"><span className="mw-chip mw-chip--on">
        {t("servers.mods.manualDropSuccess", { count: manualStageResult.copied_file_count, target: manualStageResult.target_label }, "{count} files installed to {target}")}
      </span></div> : null}
      {manualReferenceResult ? <span className="mw-chip mw-chip--on">
        {t("servers.mods.referenceResolved", { count: manualReferenceResult.resolved_ids.length }, "{count} ID(s) ready")}
      </span> : null}
      {manualReferenceError ? <ActivityNotice tone="error">{manualReferenceError}</ActivityNotice> : null}
      {manualStageError ? <ActivityNotice tone="error">{manualStageError}</ActivityNotice> : null}
    </div>;
  }

  function renderManualImportPane(allowReferences = canUseManualReferences) {
    const dropActive = manualDropActive && !modMutationsDisabled;
    return <div className="mw-manual-pane">
      {manualStaging ? <div
        className={`mw-dropzone${dropActive ? " mw-dropzone--active" : ""}${manualStageState === "running" ? " mw-dropzone--running" : ""}`}
        {...manualListDropHandlers(allowReferences)}
        role="region"
        aria-label={t("servers.mods.manualDropTitle", undefined, "Drop downloaded mods here")}
        aria-disabled={modMutationsDisabled}
        aria-busy={manualStageState === "running" || manualReferenceState === "running"}
      >
        <ShellIcon name="package" className="mw-dropzone-icon" />
        <div className="mw-dropzone-copy">
          <strong className="mw-dropzone-title">
            {manualStageState === "running" ? t("servers.mods.manualDropRunning", undefined, "Installing...")
              : manualReferenceState === "running" ? t("servers.mods.referenceResolving", undefined, "Resolving...")
              : t("servers.mods.manualDropTitle", undefined, "Drop downloaded mods here")}
          </strong>
          <span>{t("servers.mods.manualDropTarget", { target: manualStaging.target_label }, "Installs to {target}")}</span>
        </div>
        {dropActive ? <div className="mw-dropzone-feedback" aria-hidden="true">
          <ShellIcon name="package" className="mw-btn-icon" />
          <strong>{manualDropReferenceActive ? t("servers.mods.referenceDropTitle", { source: manualSourceLabel }, "Drop {source} links or IDs here")
            : t("servers.mods.manualDropTitle", undefined, "Drop downloaded mods here")}</strong>
        </div> : null}
      </div> : null}
      {renderManualImportFeedback()}
    </div>;
  }

  return (
    <section className={`mw-workbench${moduleId === "projectzomboid" ? " mw-workbench--pz" : ""}`}>
      {arkProjection.error ? <ActivityNotice tone="error">{locale === "zh-CN" ? "地图 MOD 配置无效，请在高级配置中检查 ark_mod_map_overrides。"
        : "Map Mod settings are invalid. Review ark_mod_map_overrides under Advanced."}</ActivityNotice> : null}
      {arkProjection.differs ? <ActivityNotice tone="warning">{locale === "zh-CN"
        ? "各地图的 MOD 列表、加载顺序或参数不同。跨服传送的物品、生物可能无法在目标地图加载；请核对 MOD 作者的集群兼容说明。"
        : "Maps use different Mod lists, load orders, or options. Transferred items or creatures may not load on the destination map. Check the Mod authors' cluster compatibility guidance."}</ActivityNotice> : null}
      {applyMessage ? <ActivityNotice tone="success" onDismiss={() => setApplyMessage(null)}>{applyMessage}</ActivityNotice> : null}
      {verificationWarning ? <ActivityNotice tone="warning" details={verificationWarning.details} action={
        <button className="mw-ghost-btn" type="button" disabled={lookupLoading || browseLoading}
          onClick={() => {
            setLookupRevision((current) => current + 1); retryBrowse();
            if (verificationWarning.includesOriginalText && selectedBrowseWorkshopId) {
              detailsRequests.current.invalidate(selectedBrowseWorkshopId, locale);
              setDetailFailure(null);
              setDetailRevision((value) => value + 1);
            }
          }}>{t("common.retry", undefined, "Retry")}</button>
      }>{verificationWarning.summary}</ActivityNotice> : null}
      <div className="mw-workbench-main">
            {selectedBrowseDetailsReady && !isCollectionSummary(selectedBrowseWorkshopItem) && isIncompleteWorkshopCollection(selectedBrowseWorkshopItem ?? undefined) ? <ActivityNotice tone="warning">
              {t("servers.mods.incompleteCollection", undefined, "This collection contains nested or unresolved entries. Add the individual Mods instead.")}
            </ActivityNotice> : null}
            {dstRawOverridesActive ? <ActivityNotice tone="warning">
              {t("dst.settings.modStatus.rawOverrideWarning", undefined, DST_RAW_MOD_WARNING)}
            </ActivityNotice> : null}
            {asaRawFlagsActive ? <ActivityNotice tone="warning">
              {t("servers.mods.asaRawFlagsWarning", undefined, ASA_RAW_MOD_WARNING)}
            </ActivityNotice> : null}
            {modChangesBlocked ? (
              <ActivityNotice tone="warning">
                {readOnly ? t("servers.archives.workspace.restoreForMods", undefined, "Restore this instance to manage Mods.")
                  : t("servers.mods.stopBeforeChanges", undefined, "Stop the instance before changing Mods.")}
              </ActivityNotice>
            ) : null}
            {mismatchedItems.length > 0 ? (
              <ActivityNotice tone="warning">
                {t("servers.mods.appMismatch", { count: mismatchedItems.length }, "{count} Workshop item(s) do not match this module's expected app id.")}
              </ActivityNotice>
            ) : null}
            {pzError ? <ActivityNotice tone="error">{t("servers.mods.projectZomboidScanFailed", { error: pzError }, "Project Zomboid local scan failed: {error}")}</ActivityNotice> : null}
            {downloadError ? <ActivityNotice tone="error">{t("servers.mods.installFailed", { error: formatDesktopError(t, downloadError) }, "Mod installation failed: {error}")}</ActivityNotice> : null}
            {manualInventoryError ? <ActivityNotice tone="error">{manualInventoryError}</ActivityNotice> : null}
            {mutationError ? <ActivityNotice tone="error">{mutationError}</ActivityNotice> : null}
            {managedManualMods.error && !managedManualMods.review ? <ActivityNotice tone="error">{managedManualMods.error}</ActivityNotice> : null}
            {managedManualMods.result ? <ActivityNotice
              tone={managedManualMods.result.value.warnings.length ? "warning" : "success"}
              action={managedManualMods.result.value.preserved_path ? <button type="button" className="mw-ghost-btn"
                onClick={() => {
                  const path = managedManualMods.result?.value.preserved_path;
                  if (path) void openLocalPath(path).catch((error) => setMutationError(describeError(error)));
                }}>{t("servers.mods.managed.openPreserved", undefined, "Open preserved files")}</button> : undefined}>
              {[
                managedManualMods.result.action === "delete"
                  ? t("servers.mods.managed.deleted", { count: managedManualMods.result.value.removed_file_count }, "Mod deleted; {count} program file(s) removed.")
                  : t("servers.mods.managed.updated", undefined, "Mod management updated."),
                managedManualMods.result.value.preserved_path
                  ? t("servers.mods.managed.preserved", { path: managedManualMods.result.value.preserved_path }, "Preserved files: {path}") : "",
                ...managedManualMods.result.value.warnings
              ].filter(Boolean).join("\n")}
            </ActivityNotice> : null}
            {collectionRemoval.error && !collectionRemoval.review ? <ActivityNotice tone="error">{collectionRemoval.error}</ActivityNotice> : null}

        <section className={`mw-workspace-section mw-workspace-section--${activeWorkbenchView}`} role="tabpanel">
          <div className="mw-workspace-panel">
            {modWorkflowUnsupported ? null : props.moduleDetails?.workshop?.provider === "steam" ? (
              renderSteamWorkshopWorkspace()
            ) : (
              renderCommunityWorkspace()
            )}
          </div>
        </section>
        {moduleId === "projectzomboid" ? <ProjectZomboidMapOrderEditor
          key={props.details.summary.id} value={settings.map_name} disabled={modMutationsDisabled}
          readOnly={readOnly}
          hidden={activeWorkbenchView !== "config" || manifestMode} onSave={saveProjectZomboidMapOrder}
          onReload={async () => {
            const latest = await readWritableInstanceState();
            latestSettingsRef.current = latest.settings;
            return typeof latest.settings.map_name === "string" ? latest.settings.map_name : undefined;
          }}
        /> : null}
      </div>

      {collectionRemoval.review ? <WorkshopCollectionRemovalDialog
        key={`${props.details.summary.id}:${collectionRemoval.review.collection.id}`}
        collection={collectionRemoval.review.collection}
        protectedIds={new Set(collectionRemoval.review.protectedMembers.map((member) => member.id))}
        unavailableReason={collectionRemoval.blockMessage}
        lookup={lookupMap} busy={collectionRemoval.busy} error={collectionRemoval.error}
        onClose={collectionRemoval.close} onRemove={(ids) => { void collectionRemoval.remove(ids); }}
      /> : null}
      {managedManualMods.review ? <ManagedModDeleteDialog
        key={`${props.details.summary.id}:${managedManualMods.review.management?.package_id}`}
        item={managedManualMods.review} busy={mutationBusy} blocked={modChangesBlocked}
        error={managedManualMods.error} onClose={managedManualMods.close}
        onDelete={() => {
          if (managedManualMods.review) managedManualMods.change(managedManualMods.review, "delete");
        }}
      /> : null}
      {nexusSource && nexusCredentialsOpen && nexusLocal && !readOnly ? <NexusCredentialsDialog
        disabled={modChangesBlocked || mutationBusy} onClose={() => setNexusCredentialsOpen(false)}
        onStatus={(status) => { nexusGeneration.current++; setNexusAccount(status); setNexusReview(null); }} /> : null}
      {nexusSource && nexusReview ? <NexusFileSelectionDialog reviews={nexusReview.reviews} busy={mutationBusy}
        disabled={modChangesBlocked || !nexusLocal} error={nexusInstallError} installNote={props.moduleDetails?.mods?.source?.install_note}
        onClose={() => { if (!mutationBusy) { setNexusReview(null); setNexusInstallError(null); } }}
        onInstall={(selected) => void handleInstallNexusFiles(selected)} /> : null}
    </section>
  );
}

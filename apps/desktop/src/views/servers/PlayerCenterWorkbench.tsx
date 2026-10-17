import { useEffect, useMemo, useState } from "react";
import {
  createPlayerCenterState,
  derivePlayerCenterViewModel,
  reducePlayerCenterState
} from "../../domain/live-player-state";
import { useI18n } from "../../i18n";
import type {
  ExecuteInstancePlayerActionInput,
  ExecuteInstancePlayerActionResult,
  InstanceDetails,
  InstancePlayerAccessMutationInput,
  InstancePlayerAccessMutationResult,
  InstanceRuntimeOverview,
  ModuleDetails,
  SaveInstanceSettingsOptions,
  UpdateInstanceInput
} from "../../types";
import {
  instanceHasRunningProcess,
  runtimeProcessKeyIsRunning
} from "../../runtime-action-state";
import { usePlayerAccess } from "./player-center/use-player-access";
import { ManualPlayerActions } from "./player-center/ManualPlayerActions";
import { actionNeedsManualTarget, manualPlayerActionTargetError, readManualPlayerActions } from "./player-center/manual-player-action-model";
import { readPlayerAccessRosterCapabilities } from "./player-center/player-access-roster-model";
import { OnlinePlayersView } from "./player-center/OnlinePlayersView";
import { useLivePlayers } from "./player-center/use-live-players";

import { MoriaNativeSettingsEditor } from "./MoriaNativeSettingsEditor";

interface PlayerCenterWorkbenchProps {
  details: InstanceDetails;
  moduleDetails: ModuleDetails | null;
  runtime: InstanceRuntimeOverview | null;
  readOnly?: boolean;
  onApplyPlayerAccessMutation?: (
    input: InstancePlayerAccessMutationInput
  ) => Promise<InstancePlayerAccessMutationResult>;
  onExecutePlayerAction?: (input: ExecuteInstancePlayerActionInput) => Promise<ExecuteInstancePlayerActionResult>;
  onPrepareValheimControl?: (instanceId: string) => Promise<InstanceDetails>;
  onOpenSettings?: () => void;
  onSaveSettings?: (input: UpdateInstanceInput, options?: SaveInstanceSettingsOptions) => Promise<InstanceDetails | undefined>;
}

export function PlayerCenterWorkbench(props: PlayerCenterWorkbenchProps) {
  return <PlayerCenterInstance key={props.details.summary.id} {...props} />;
}

function PlayerCenterInstance(props: PlayerCenterWorkbenchProps) {
  const { locale, t } = useI18n();
  const access = usePlayerAccess(props);
  const rosterCapabilities = useMemo(
    () => readPlayerAccessRosterCapabilities(props.moduleDetails),
    [props.moduleDetails]
  );
  const [state, setState] = useState(() => createPlayerCenterState({
    instanceId: props.details.summary.id
  }));
  const declaredActions = props.moduleDetails?.runtime?.player_actions ?? [];
  const playerListActionId = props.moduleDetails?.runtime?.player_list?.action_id;
  const playerListAction = declaredActions.find((action) => action.id === playerListActionId) ?? null;
  const instanceRunning = instanceHasRunningProcess(props.details.summary, props.details.active_run);
  const playerRuntimeAvailable = !props.readOnly && (playerListAction?.process_key
    ? runtimeProcessKeyIsRunning(props.details.active_run, playerListAction.process_key)
    : instanceRunning);
  const livePlayers = useLivePlayers(
    props.details.summary.id,
    !props.readOnly,
    playerRuntimeAvailable ? "running" : "stopped"
  );

  useEffect(() => {
    setState((current) => reducePlayerCenterState(current, {
      type: "instance-changed",
      instanceId: props.details.summary.id
    }));
  }, [props.details.summary.id, props.readOnly]);

  useEffect(() => {
    if (props.readOnly || !livePlayers.snapshot) {
      return;
    }
    setState((current) => reducePlayerCenterState(current, {
      type: "snapshot-received",
      snapshot: livePlayers.snapshot!
    }));
  }, [livePlayers.snapshot, props.readOnly]);

  const viewModel = derivePlayerCenterViewModel(props.readOnly
    ? createPlayerCenterState({ instanceId: props.details.summary.id }) : state, Date.now());
  const manualActions = useMemo(
    () => readManualPlayerActions(props.moduleDetails, rosterCapabilities),
    [props.moduleDetails, rosterCapabilities]
  );
  // 手动查询 accepts an ID when at least one manual action accepts it (rosters check their own codecs).
  const manualTargetFormat = useMemo(() => {
    const eligible = manualActions.filter(actionNeedsManualTarget);
    if (eligible.length === 0) return undefined;
    const moduleId = props.details.summary.module_id;
    const hints = Array.from(new Set(eligible.map((action) => manualPlayerActionTargetError(moduleId, action, "", locale)).filter(Boolean)));
    return {
      hint: hints.join(" ") || null,
      validate: (target: string) => {
        let first: string | null = null;
        for (const action of eligible) {
          const error = manualPlayerActionTargetError(moduleId, action, target, locale);
          if (!error) return null;
          first ??= error;
        }
        return first;
      }
    };
  }, [manualActions, props.details.summary.module_id, locale]);  return (
    <section className="player-access-workbench" aria-label={t("servers.playerCenter.workspaceLabel", undefined, "Player workspace")}>
      <OnlinePlayersView
        instanceDetails={props.details}
        onPrepareValheimControl={props.onPrepareValheimControl}
        readOnly={props.readOnly}
        moduleId={props.details.summary.module_id}
        declaredActions={declaredActions}
        playerActionIds={props.moduleDetails?.runtime?.player_management?.status === "pending_adapter"
          ? [] : props.moduleDetails?.runtime?.player_list?.player_action_ids ?? []}
        error={props.readOnly ? null : livePlayers.error}
        loading={!props.readOnly && livePlayers.loading}
        locale={locale}
        access={access}
        manualTargetFormat={manualTargetFormat}
        extraLists={props.details.summary.module_id === "returntomoria" ? [{
          key: "moria-permissions", title: t("servers.playerCenter.moria.permissionsTab", undefined, "Player permissions"),
          // The native permissions file is a category of the players list, not a separate card above it.
          content: <MoriaNativeSettingsEditor details={props.details} moduleDetails={props.moduleDetails} kind="permissions" embedded
            locale={locale} t={t} readOnly={props.readOnly} onSaveSettings={props.onSaveSettings} />
        }] : undefined}
        manualActions={manualActions.length > 0 ? (active, onActionCompleted, onActionFailed) => (
          <ManualPlayerActions
            key={`manual:${props.details.summary.id}`}
            actions={manualActions}
            moduleId={props.details.summary.module_id}
            disabled={Boolean(props.readOnly) || !instanceRunning}
            active={active}
            instanceId={props.details.summary.id}
            locale={locale}
            t={t}
            onActionCompleted={onActionCompleted}
            onActionFailed={onActionFailed}
          />
        ) : undefined}
        now={Date.now()}
        onOpenSettings={props.onOpenSettings}
        onRefresh={livePlayers.refresh}
        onExecutePlayerAction={props.onExecutePlayerAction}
        onSelectPlayer={(playerKey) => setState((current) => reducePlayerCenterState(current, {
          type: "player-selected",
          playerKey
        }))}
        presentation={viewModel.livePlayers}
        rosterCapabilities={rosterCapabilities}
        runtime={props.readOnly ? null : props.runtime}
        runtimeAvailable={playerRuntimeAvailable}
        snapshot={props.readOnly ? null : state.snapshot}
        t={t}
      />
    </section>
  );
}

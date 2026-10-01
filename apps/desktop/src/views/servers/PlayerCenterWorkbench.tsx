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
import { readManualPlayerActions } from "./player-center/manual-player-action-model";
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
  return (
    <section className="player-access-workbench" aria-label={t("servers.playerCenter.workspaceLabel", undefined, "Player workspace")}>
      {props.details.summary.module_id === "returntomoria" && <MoriaNativeSettingsEditor
        details={props.details} moduleDetails={props.moduleDetails} kind="permissions"
        locale={locale} t={t} readOnly={props.readOnly} onSaveSettings={props.onSaveSettings} />}
      <OnlinePlayersView
        readOnly={props.readOnly}
        moduleId={props.details.summary.module_id}
        declaredActions={declaredActions}
        playerActionIds={props.moduleDetails?.runtime?.player_management?.status === "pending_adapter"
          ? [] : props.moduleDetails?.runtime?.player_list?.player_action_ids ?? []}
        error={props.readOnly ? null : livePlayers.error}
        loading={!props.readOnly && livePlayers.loading}
        locale={locale}
        access={access}
        manualActions={manualActions.length > 0 ? (active) => (
          <ManualPlayerActions
            key={`manual:${props.details.summary.id}`}
            actions={manualActions}
            moduleId={props.details.summary.module_id}
            disabled={Boolean(props.readOnly) || !instanceRunning || !active}
            instanceId={props.details.summary.id}
            locale={locale}
            t={t}
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

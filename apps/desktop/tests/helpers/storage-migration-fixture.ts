import type { StorageMigrationModel } from "../../src/hooks/useStorageMigration";

// Unrelated component fixtures have no native file migration service.
export const unavailableStorageMigration: StorageMigrationModel = {
  available: false, status: null, draft: null, recovering: false, pending: null, unconfirmed: false,
  error: null, cancelRequested: false, refreshing: false, refreshError: null, dismissError: null, hasActivity: false, canDismiss: false,
  dismiss: () => {}, migrate: async () => {}, discard: async () => {}, cancel: async () => {}, refresh: async () => {}, recheck: () => {}
};

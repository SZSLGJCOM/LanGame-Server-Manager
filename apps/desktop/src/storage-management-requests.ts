import type { InstanceArchiveList } from "./storage-management-types";
import { selectLocaleText } from "./i18n-config";
import { readPreferredLocale } from "./locale-preference";

// Archive inventory reads can update storage metadata, so they must not overlap
// each other, an archive inspection, a catalog count read or a storage mutation.
// Keep one shared inventory read and one admitted write;
// ownership lasts until the native request settles, including across view unmounts.
export class StorageManagementRequests {
  private read: Promise<InstanceArchiveList> | null = null;
  private mutation: Promise<unknown> | null = null;
  private inspection: Promise<unknown> | null = null;
  private pendingInspections = 0;

  list(action: () => Promise<InstanceArchiveList>): Promise<InstanceArchiveList> {
    if (this.read) return this.read;
    const request = Promise.allSettled([this.mutation, this.inspection]).then(action).finally(() => {
      if (this.read === request) this.read = null;
    });
    this.read = request;
    return request;
  }

  inspect<T>(action: () => Promise<T>): Promise<T> {
    if (this.pendingInspections >= 128) return Promise.reject(new Error(selectLocaleText(
      readPreferredLocale(), "待处理的归档读取请求过多，请稍后重试。", "Too many pending archive reads. Try again shortly."
    )));
    this.pendingInspections++;
    const request = Promise.allSettled([this.read, this.mutation, this.inspection]).then(action).finally(() => {
      this.pendingInspections--;
      if (this.inspection === request) this.inspection = null;
    });
    this.inspection = request;
    return request;
  }

  mutate<T>(action: () => Promise<T>, options?: { immediate: boolean }): Promise<T> {
    if (this.mutation || (options?.immediate && (this.read || this.inspection))) return Promise.reject(new Error(selectLocaleText(
      readPreferredLocale(), "另一项存储操作正在进行，请等待操作完成。", "Another storage operation is in progress. Wait for it to finish."
    )));
    const precedingRead = this.read;
    const precedingInspection = this.inspection;
    // A read requested after this write must observe its outcome, never reuse
    // the inventory obtained before it. A failed read must not block the write.
    this.read = null;
    let succeed!: (value: T) => void;
    let fail!: (reason: unknown) => void;
    const outcome = new Promise<T>((resolve, reject) => { succeed = resolve; fail = reject; });
    const request = outcome.finally(() => {
      if (this.mutation === request) this.mutation = null;
    });
    this.mutation = request;
    const start = () => {
      try { void action().then(succeed, fail); }
      catch (error) { fail(error); }
    };
    if (precedingRead || precedingInspection) void Promise.allSettled([precedingRead, precedingInspection]).then(start);
    else start();
    return request;
  }
}

export const storageManagementRequests = new StorageManagementRequests();

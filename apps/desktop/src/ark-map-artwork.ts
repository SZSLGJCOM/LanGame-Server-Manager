export interface ArkImportedArtwork { dataUrl: string; source: string }
export class ArkArtworkInvalidError extends Error {}
const BYTE_LIMIT = 4 * 1024 * 1024;
const DIMENSION_LIMIT = 4096;
function validatePngHeader(header: Uint8Array) {
  if (header.length !== 33) throw new Error("The PNG header is incomplete.");
  const data = new DataView(header.buffer, header.byteOffset, header.byteLength);
  if (![137, 80, 78, 71, 13, 10, 26, 10].every((value, index) => header[index] === value)
    || data.getUint32(8) !== 13 || String.fromCharCode(...header.slice(12, 16)) !== "IHDR") throw new Error("Only PNG map images with a valid IHDR are supported.");
  const width = data.getUint32(16), height = data.getUint32(20);
  if (width < 128 || width > DIMENSION_LIMIT || height !== width) throw new Error("Use a square map image between 128 and 4096 pixels per side.");
  const depths: Record<number, number[]> = { 0: [1, 2, 4, 8, 16], 2: [8, 16], 3: [1, 2, 4, 8], 4: [8, 16], 6: [8, 16] };
  let crc = 0xffffffff;
  for (const byte of header.slice(12, 29)) { crc ^= byte; for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0); }
  if (!depths[header[25]]?.includes(header[24]) || header[26] !== 0 || header[27] !== 0 || header[28] > 1
    || ((crc ^ 0xffffffff) >>> 0) !== data.getUint32(29)) throw new Error("The PNG header is invalid.");
}
function key(edition: string, mapName: string) { return JSON.stringify([edition, mapName.toLowerCase()]); }
function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open("lsgm.ark-map-artwork", 1);
    request.onupgradeneeded = () => { request.result.createObjectStore("maps"); };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error ?? new Error("Could not open imported map storage."));
    request.onblocked = () => reject(new Error("Imported map storage is busy in another window."));
  });
}
async function entry<T>(mode: IDBTransactionMode, action: (store: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await database();
  try {
    return await new Promise<T>((resolve, reject) => {
      const transaction = db.transaction("maps", mode);
      const request = action(transaction.objectStore("maps"));
      transaction.oncomplete = () => resolve(request.result);
      transaction.onerror = transaction.onabort = () => reject(transaction.error ?? request.error ?? new Error("Could not save the imported map."));
    });
  } finally { db.close(); }
}
export async function readArkImportedArtwork(edition: string, mapName: string): Promise<ArkImportedArtwork | null> {
  const value: unknown = await entry("readonly", store => store.get(key(edition, mapName)));
  if (value === undefined) return null;
  if (!value || typeof value !== "object" || !("dataUrl" in value) || !("source" in value)
    || typeof value.dataUrl !== "string" || value.dataUrl.length > Math.ceil(BYTE_LIMIT * 4 / 3) + 100
    || !/^data:image\/png;base64,[A-Za-z0-9+/]+={0,2}$/.test(value.dataUrl)
    || typeof value.source !== "string" || value.source.length > 300) throw new ArkArtworkInvalidError("The saved map image is invalid. Import it again or remove it.");
  try { validatePngHeader(Uint8Array.from(atob(value.dataUrl.slice("data:image/png;base64,".length, "data:image/png;base64,".length + 44)), character => character.charCodeAt(0))); }
  catch { throw new ArkArtworkInvalidError("The saved map image is invalid. Import it again or remove it."); }
  return { dataUrl: value.dataUrl, source: value.source };
}
export async function importArkArtwork(edition: string, mapName: string, file: File): Promise<ArkImportedArtwork> {
  if (file.size < 33 || file.size > BYTE_LIMIT) throw new Error("Use a PNG no larger than 4 MiB.");
  // Bound allocation before asking the browser to decode any compressed pixels.
  validatePngHeader(new Uint8Array(await file.slice(0, 33).arrayBuffer()));
  const url = URL.createObjectURL(file);
  try {
    const image = new Image(); image.src = url; await image.decode();
    if (image.naturalWidth < 128 || image.naturalWidth > DIMENSION_LIMIT || image.naturalHeight !== image.naturalWidth)
      throw new Error("Use a square map image between 128 and 4096 pixels per side.");
  } finally { URL.revokeObjectURL(url); }
  const dataUrl = await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => typeof reader.result === "string" ? resolve(reader.result) : reject(new Error("Could not read the PNG image."));
    reader.onerror = () => reject(reader.error ?? new Error("Could not read the PNG image.")); reader.readAsDataURL(file.slice(0, file.size, "image/png"));
  });
  const artwork = { dataUrl, source: `Local PNG: ${file.name.replace(/[\x00-\x1f\x7f-\x9f]/g, "").slice(0, 256)}` };
  await entry("readwrite", store => store.put(artwork, key(edition, mapName)));
  return artwork;
}
export async function removeArkImportedArtwork(edition: string, mapName: string): Promise<void> {
  await entry("readwrite", store => store.delete(key(edition, mapName)));
}

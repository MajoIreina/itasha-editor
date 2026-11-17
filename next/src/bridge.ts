import { invoke, isTauri } from '@tauri-apps/api/core';
export interface SessionRef { projectId: string; sessionEpoch: string }
export interface Project { session: SessionRef; name: string; revision: string; contentId: string; dirty: boolean; stage: string; vehicleId: string | null; configuration: VehicleConfiguration | null; editor: EditorDocument }
export interface ImageAsset { id: string; name: string; width: number; height: number }
export type Shape = 'rectangle' | 'ellipse' | 'triangle' | 'line' | 'star' | 'hexagon';
export type LayerSource = { kind: 'shape'; shape: Shape; fill: string } | { kind: 'text'; text: string; fontFamily: string; fill: string } | { kind: 'image'; assetId: string };
export interface Placement { anchor?:{partId:string;regionId:string;sourceTriangleId:string;barycentric:number[];paintSurfaceFingerprint:string}; surfaceId: string; crossSurface?: boolean; flipX?:boolean; aspectRatioLocked?:boolean; profileVersion: number; xMm: number; yMm: number; widthMm: number; heightMm: number; rotationDeg: number }
export interface Layer { id: string; name: string; visible: boolean; locked: boolean; opacity: number; placement: Placement; source: LayerSource }
export interface EditorDocument { modelFingerprint: string | null; layers: Layer[]; assets: ImageAsset[] }
export type EditOperation = { type: 'insert'; layer: Layer; fingerprint: string } | { type: 'update'; layer: Layer } | { type: 'delete'; id: string } | { type: 'reorder'; id: string; offset: number };
export interface Summary { projectId: string; name: string; revision: string; savedAtUnixMs: string; vehicleId: string | null; pinned: boolean }
export const optionGroups = ['paint', 'wheels', 'interior', 'steering'] as const;
export type OptionGroup = typeof optionGroups[number];
export const optionLabels: Record<OptionGroup, string> = { paint: '车漆', wheels: '轮毂', interior: '内饰', steering: '方向盘' };
export interface VehicleConfiguration { catalogVersion: number; paint: string; wheels: string; interior: string; steering: string }
export interface VehicleOption { id: string; name: string; swatch: string | null; excludes: string[] }
export interface Vehicle extends Record<OptionGroup, VehicleOption[]> { id: string; name: string; family: string; bodyStyle: string; defaults: VehicleConfiguration }
export interface Settings {
  theme: 'dark' | 'light' | 'system';
  schemaVersion: number; layout: 'both' | 'edit' | '3d'; autosave: boolean;
  workspace: { direction: 'vertical' | 'horizontal'; splitPercent: number; panels: Record<string, { height: number; minimized: boolean }>; libraryWidth: number; libraryMinimized: boolean };
  performance: { targetFps: number; renderScale: number; textureLimit: number; cacheBudgetMb: number; reducedMotion: boolean; antialiasing: 'off' | 'fxaa' | 'taa' | 'taa2x' | 'ssaa' };
  scene: { skybox: boolean; timeMinutes: number };
  shortcuts: { save: string; undo: string; redo: string; settings: string };
  paths: { models: string; exports: string; cache: string };
}
export interface View { project: Project | null; projects: Summary[]; projectWarnings: string[]; settings: Settings; dataDirectory: string; vehicles: Vehicle[] }
export interface Mutation { requestId: string; session: SessionRef; expectedRevision: string }
export type Action =
  | { type: 'create-configured'; name: string; vehicleId: string; configuration: VehicleConfiguration }
  | { type: 'create'; name: string } | { type: 'open'; id: string }
  | { type: 'rename'; mutation: Mutation; name: string }
  | { type: 'delete-project' | 'duplicate-project'; id: string }
  | { type: 'pin-project'; id: string; pinned: boolean }
  | { type: 'edit'; mutation: Mutation; operation: EditOperation }
  | { type: 'save' | 'undo' | 'redo' | 'discard'; mutation: Mutation }
  | { type: 'settings'; value: Settings };
export const desktop = isTauri();
export function mutation(project: Project): Mutation {
  return { requestId: crypto.randomUUID(), session: project.session, expectedRevision: project.revision };
}
export const readView = () => invoke<View>('read_view');
export const execute = (action: Action) => invoke<void>('execute', { requestId: crypto.randomUUID(), action });
export const closeClean = () => invoke<void>('close_clean');
export const chooseDirectory = () => invoke<string | null>('choose_directory');

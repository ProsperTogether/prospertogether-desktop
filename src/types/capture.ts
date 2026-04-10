export type CaptureMode = 'screen' | 'monitor' | 'window' | 'region';

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type CaptureTarget =
  | { mode: 'screen' }
  | { mode: 'monitor'; x: number; y: number; width: number; height: number }
  | { mode: 'window'; title: string; hwnd: number; rect: Rect }
  | { mode: 'region'; x: number; y: number; width: number; height: number };

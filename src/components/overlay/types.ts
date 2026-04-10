export type DrawTool = 'pen' | 'highlighter' | 'arrow' | 'rectangle';

export type DrawColor = string;

export interface Point {
  x: number;
  y: number;
}

export interface StrokeAction {
  tool: DrawTool;
  color: string;
  width: number;
  opacity: number;
  points: Point[];
}

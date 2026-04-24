export interface ValveStartParams {
  push: number;
  release: number;
  count: number;
  interval: number;
  duty: number;
}

export interface ValveLedParams {
  mode: 0 | 1 | 2;
  r: number;
  g: number;
  b: number;
  speed: number;
}

export interface ValveStatus {
  running: boolean;
  count: number;
  state: number;
  stateLabel: string;
  raw: string;
}

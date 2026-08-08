import type { RuntimeIdentity, WeightDomain, WfstResource } from "@vinary-tree/interop";

export interface WfstArc {
  readonly input: string | null;
  readonly output: string | null;
  readonly target: bigint;
  readonly weight: number;
}
export interface WfstState {
  readonly valid: boolean;
  readonly final: boolean;
  readonly finalWeight: number;
  readonly arcs: readonly WfstArc[];
}
export interface Wfst extends WfstResource {
  readonly weightDomain: WeightDomain;
  start(): bigint;
  state(state: bigint): WfstState;
}
export interface WfstBuilder {
  addState(): number;
  setStart(state: number): void;
  setFinal(state: number, weight?: number): void;
  addArc(from: number, input: string | null, output: string | null, to: number, weight?: number): void;
  build(): Wfst;
  close(): void;
}
export interface LlingLlangNamespace {
  readonly runtimeIdentity: RuntimeIdentity;
  vectorWfst(): WfstBuilder;
  compose(first: WfstResource, second: WfstResource): Wfst;
}
export const runtimeIdentity: RuntimeIdentity;
export function vectorWfst(): WfstBuilder;
export function compose(first: WfstResource, second: WfstResource): Wfst;
declare const llingLlang: LlingLlangNamespace;
export default llingLlang;

// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { LogCoordinates } from "./LogCoordinates";

export type LogLine =
    | { type: "FromNode"; source: LogCoordinates; target: LogCoordinates; indirect: boolean }
    | { type: "ToNode"; source: LogCoordinates; target: LogCoordinates; indirect: boolean }
    | { type: "ToIntersection"; source: LogCoordinates; target: LogCoordinates; indirect: boolean }
    | { type: "ToMissing"; source: LogCoordinates; target: LogCoordinates; indirect: boolean };

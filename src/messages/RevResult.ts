// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { RevChange } from "./RevChange";
import type { RevConflict } from "./RevConflict";
import type { RevHeader } from "./RevHeader";
import type { RevId } from "./RevId";

export type RevResult =
    | { type: "NotFound"; id: RevId }
    | {
          type: "Detail";
          header: RevHeader;
          parents: Array<RevHeader>;
          changes: Array<RevChange>;
          conflicts: Array<RevConflict>;
      };

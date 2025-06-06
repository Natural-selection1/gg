// This file was generated by [ts-rs](https://github.com/Aleph-Alpha/ts-rs). Do not edit this file manually.
import type { MultilineString } from "./MultilineString";
import type { RepoStatus } from "./RepoStatus";
import type { RevHeader } from "./RevHeader";

export type MutationResult =
    | { type: "Unchanged" }
    | { type: "Updated"; new_status: RepoStatus }
    | { type: "UpdatedSelection"; new_status: RepoStatus; new_selection: RevHeader }
    | { type: "PreconditionError"; message: string }
    | { type: "InternalError"; message: MultilineString };

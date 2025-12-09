use std::sync::Arc;

use anyhow::{Result, anyhow};
use jj_lib::backend::{CopyId, FileId, TreeValue};
use jj_lib::commit::Commit;
use jj_lib::conflicts;
use jj_lib::conflicts::{ConflictMarkerStyle, ConflictMaterializeOptions, MaterializedTreeValue};
use jj_lib::files::FileMergeHunkLevel;
use jj_lib::git::REMOTE_NAME_FOR_LOCAL_GIT_REPO;
use jj_lib::merge::{Merge, SameChange};
use jj_lib::merged_tree::{MergedTree, MergedTreeBuilder};
use jj_lib::object_id::ObjectId as ObjectIdTrait;
use jj_lib::repo::Repo;
use jj_lib::repo_path::RepoPath;
use jj_lib::rewrite::{RebaseOptions, RebasedCommit};
use jj_lib::store::Store;
use jj_lib::str_util::StringPattern;
use jj_lib::tree_merge::MergeOptions;
use tokio::io::AsyncReadExt;

use super::Mutation;
use super::gui_util::WorkspaceSession;
use crate::messages::{
    AbandonRevisions, BackoutRevisions, CheckoutRevision, CopyChanges, CopyHunk, CreateRef,
    CreateRevision, CreateRevisionBetween, DeleteRef, DescribeRevision, DuplicateRevisions,
    GitFetch, GitPush, Id, InsertRevision, MoveChanges, MoveHunk, MoveRef, MoveRevision,
    MoveSource, MutationResult, RenameBranch, StoreRef, TrackBranch, UndoOperation, UntrackBranch,
};
use crate::worker::gui_util::run_jj;

macro_rules! precondition {
    ($($args:tt)*) => {
        return Ok(MutationResult::PreconditionError { message: format!($($args)*) })
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for AbandonRevisions {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["abandon"])
            .args(self.ids.iter().map(|id| id.multiple_of_four_prefix()))
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj abandon: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for BackoutRevisions {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["revert"])
            .args(
                self.ids
                    .iter()
                    .flat_map(|id| ["-r".into(), id.change.multiple_of_four_prefix()]),
            )
            .args(["--onto", "@"])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj revert: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CheckoutRevision {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["edit", &self.id.commit.multiple_of_four_prefix()])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    let working_copy = ws.get_commit(ws.wc_id())?;
                    let new_selection = ws.format_header(&working_copy, Some(false))?;
                    Ok(MutationResult::UpdatedSelection {
                        new_status: ws.format_status(),
                        new_selection,
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj edit: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CreateRevision {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["new"])
            .args(
                self.parent_ids
                    .iter()
                    .map(|id| id.change.multiple_of_four_prefix()),
            )
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    let working_copy = ws.get_commit(ws.wc_id())?;
                    let new_selection = ws.format_header(&working_copy, Some(false))?;
                    Ok(MutationResult::UpdatedSelection {
                        new_status: ws.format_status(),
                        new_selection,
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj new: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CreateRevisionBetween {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["new"])
            .args(["-A", &self.after_id.multiple_of_four_prefix()])
            .args(["-B", &self.before_id.change.multiple_of_four_prefix()])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    let working_copy = ws.get_commit(ws.wc_id())?;
                    let new_selection = ws.format_header(&working_copy, Some(false))?;
                    Ok(MutationResult::UpdatedSelection {
                        new_status: ws.format_status(),
                        new_selection,
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj new: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for DescribeRevision {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["describe", &self.id.change.multiple_of_four_prefix()])
            .args(["-m", &self.new_description])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj describe: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for DuplicateRevisions {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["duplicate"])
            .args(
                self.ids
                    .iter()
                    .map(|id| id.change.multiple_of_four_prefix()),
            )
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj duplicate: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for InsertRevision {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["rebase"])
            .args(["-r", &self.id.change.multiple_of_four_prefix()])
            .args(["--after", &self.after_id.change.multiple_of_four_prefix()])
            .args(["--before", &self.before_id.change.multiple_of_four_prefix()])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj rebase: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for MoveRevision {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["rebase"])
            .args(["-r", &self.id.change.multiple_of_four_prefix()])
            .args(
                self.parent_ids
                    .iter()
                    .flat_map(|id| ["-o".into(), id.change.multiple_of_four_prefix()]),
            )
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj rebase: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for MoveSource {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["rebase"])
            .args(["-r", &self.id.change.multiple_of_four_prefix()])
            .args(
                self.parent_ids
                    .iter()
                    .flat_map(|id| ["-o".into(), id.multiple_of_four_prefix()]),
            )
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj rebase: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for MoveChanges {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["squash"])
            .args(["--from", &self.from_id.change.multiple_of_four_prefix()])
            .args(["--into", &self.to_id.multiple_of_four_prefix()])
            .args(self.paths.iter().map(|path| path.repo_path.clone()))
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj squash: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CopyChanges {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["restore"])
            .args(["--from", &self.from_id.multiple_of_four_prefix()])
            .args(["--into", &self.to_id.change.multiple_of_four_prefix()])
            .args(self.paths.iter().map(|path| path.repo_path.clone()))
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj restore: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for TrackBranch {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        match self.r#ref {
            StoreRef::Tag { tag_name } => {
                precondition!("{} is a tag and cannot be tracked", tag_name);
            }
            StoreRef::LocalBookmark { branch_name, .. } => {
                precondition!("{} is a local bookmark and cannot be tracked", branch_name);
            }
            StoreRef::RemoteBookmark {
                branch_name,
                remote_name,
                ..
            } => {
                let result = run_jj(["bookmark", "track"])
                    .arg(format!("{}@{}", branch_name, remote_name))
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark track: {e}")),
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for UntrackBranch {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        match self.r#ref {
            StoreRef::Tag { tag_name } => {
                precondition!("{} is a tag and cannot be untracked", tag_name);
            }
            StoreRef::LocalBookmark { branch_name, .. } => {
                for (remote_ref_symbol, remote_ref) in ws.view().remote_bookmarks_matching(
                    &StringPattern::exact(branch_name).to_matcher(),
                    &StringPattern::all().to_matcher(),
                ) {
                    if remote_ref_symbol.remote == REMOTE_NAME_FOR_LOCAL_GIT_REPO
                        || !remote_ref.is_tracked()
                    {
                        continue;
                    }

                    let result = run_jj(["bookmark", "untrack"])
                        .arg(format!(
                            "{}@{}",
                            remote_ref_symbol.name.as_str(),
                            remote_ref_symbol.remote.as_str()
                        ))
                        .current_dir(ws.workspace.workspace_root())
                        .output();

                    match result {
                        Ok(output) => {
                            if !output.status.success() {
                                return Ok(MutationResult::PreconditionError {
                                    message: String::from_utf8_lossy(&output.stderr).trim().into(),
                                });
                            }
                        }
                        Err(e) => {
                            return Err(anyhow!("Failed to execute jj bookmark untrack: {e}"));
                        }
                    }
                }

                ws.load_at_head()?;
                Ok(MutationResult::Updated {
                    new_status: ws.format_status(),
                })
            }
            StoreRef::RemoteBookmark {
                branch_name,
                remote_name,
                ..
            } => {
                let result = run_jj(["bookmark", "untrack"])
                    .arg(format!("{}@{}", branch_name, remote_name))
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark untrack: {e}")),
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for RenameBranch {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj([
            "bookmark",
            "rename",
            self.r#ref.as_branch()?,
            &self.new_name,
        ])
        .current_dir(ws.workspace.workspace_root())
        .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj bookmark rename: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CreateRef {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let revision_arg = self.id.change.multiple_of_four_prefix();

        match self.r#ref {
            StoreRef::RemoteBookmark {
                branch_name,
                remote_name,
                ..
            } => {
                precondition!(
                    "{}@{} is a remote bookmark and cannot be created",
                    branch_name,
                    remote_name
                );
            }
            StoreRef::LocalBookmark { branch_name, .. } => {
                let result = run_jj(["bookmark", "create"])
                    .args(["-r", &revision_arg])
                    .arg(&branch_name)
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark create: {e}")),
                }
            }
            StoreRef::Tag { tag_name, .. } => {
                let result = run_jj(["tag", "set"])
                    .args(["-r", &revision_arg])
                    .arg(&tag_name)
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj tag set: {e}")),
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for DeleteRef {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        match self.r#ref {
            StoreRef::RemoteBookmark { branch_name, .. } => {
                let result = run_jj(["bookmark", "forget", &branch_name])
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark forget: {e}")),
                }
            }
            StoreRef::LocalBookmark { branch_name, .. } => {
                let result = run_jj(["bookmark", "forget", &branch_name])
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark forget: {e}")),
                }
            }
            StoreRef::Tag { tag_name } => {
                let result = run_jj(["tag", "delete", &tag_name])
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj tag delete: {e}")),
                }
            }
        }
    }
}

// does not currently enforce fast-forwards
#[async_trait::async_trait(?Send)]
impl Mutation for MoveRef {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let change_id_prefix = self.to_id.change.multiple_of_four_prefix();

        match self.r#ref {
            StoreRef::RemoteBookmark {
                branch_name,
                remote_name,
                ..
            } => {
                precondition!("Bookmark is remote: {branch_name}@{remote_name}")
            }
            StoreRef::LocalBookmark { branch_name, .. } => {
                let result = run_jj(["bookmark"])
                    .args(["move", &branch_name])
                    .args(["--to", &change_id_prefix])
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj bookmark move: {e}")),
                }
            }
            StoreRef::Tag { tag_name } => {
                let result = run_jj(["tag", "set"])
                    .args(["-r", &change_id_prefix])
                    .arg(&tag_name)
                    .arg("--allow-move")
                    .current_dir(ws.workspace.workspace_root())
                    .output();

                match result {
                    Ok(output) => {
                        if output.status.success() {
                            ws.load_at_head()?;
                            Ok(MutationResult::Updated {
                                new_status: ws.format_status(),
                            })
                        } else {
                            Ok(MutationResult::PreconditionError {
                                message: String::from_utf8_lossy(&output.stderr).trim().into(),
                            })
                        }
                    }
                    Err(e) => Err(anyhow!("Failed to execute jj tag set: {e}")),
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for MoveHunk {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let from = ws.resolve_single_change(&self.from_id)?;
        let mut to = ws.resolve_single_commit(&self.to_id)?;

        if ws.check_immutable(vec![from.id().clone(), to.id().clone()])? {
            precondition!("Revisions are immutable");
        }

        // Split-rebase-squash algorithm:
        // - sibling_tree represents a virtual commit with just the hunk (like jj split)
        // - from_tree is modified by extracting the hunk, and its descendants updated (like jj rebase)
        // - to_tree is given the added hunk by doing a 3-way merge (like jj squash)
        let mut tx: jj_lib::transaction::Transaction = ws.start_transaction().await?;
        let repo_path = RepoPath::from_internal_string(&self.path.repo_path)?;

        // Get the base tree (from's parent) - this is the tree the hunk was computed against
        let from_tree = from.tree();
        let from_parents: Result<Vec<_>, _> = from.parents().collect();
        let from_parents = from_parents?;
        if from_parents.len() != 1 {
            precondition!("Cannot move hunk from a merge commit");
        }
        let base_tree = from_parents[0].tree();

        // Construct the "sibling tree": base_tree with just this hunk applied.
        // This represents a virtual sibling commit containing only the hunk.
        let store = tx.repo().store();
        let base_content = read_file_content(store, &base_tree, repo_path).await?;
        let sibling_content = apply_hunk_to_base(&base_content, &self.hunk)?;
        let sibling_blob_id = store
            .write_file(repo_path, &mut sibling_content.as_slice())
            .await?;
        let sibling_executable = match from_tree.path_value(repo_path)?.into_resolved() {
            Ok(Some(TreeValue::File { executable, .. })) => executable,
            Ok(_) => false,
            Err(_) => false,
        };
        let sibling_tree = update_tree_entry(
            store,
            &base_tree,
            repo_path,
            sibling_blob_id,
            sibling_executable,
        )?;

        // Remove hunk from source: backout the base→sibling diff from from_tree
        let remainder_tree = from_tree
            .clone()
            .merge(sibling_tree.clone(), base_tree.clone())
            .await?;

        // Apply hunk to destination: merge the base→sibling diff into to_tree
        // (may be recomputed after rebase in the from_is_ancestor case)
        let to_tree = to.tree();
        let mut new_to_tree = to_tree
            .merge(base_tree.clone(), sibling_tree.clone())
            .await?;

        let abandon_source = remainder_tree.tree_ids() == base_tree.tree_ids();
        let description = combine_messages(&from, &to, abandon_source);

        // Check ancestry to determine rebase strategy. The hunk must be applied to the destination's
        // tree AFTER any ancestry-related rebasing, so we do it early if moving from an ancestor.
        let from_is_ancestor = tx.repo().index().is_ancestor(from.id(), to.id())?;
        let to_is_ancestor = tx.repo().index().is_ancestor(to.id(), from.id())?;

        if to_is_ancestor {
            // Child→Parent: apply hunk to ancestor, then handle source
            tx.repo_mut()
                .rewrite_commit(&to)
                .set_tree(new_to_tree)
                .set_description(description)
                .write()?;

            if abandon_source {
                tx.repo_mut().record_abandoned_commit(&from);
            } else {
                tx.repo_mut()
                    .rewrite_commit(&from)
                    .set_tree(remainder_tree)
                    .write()?;
            }

            // Rebase all descendants, which includes rebasing source's descendants onto modified ancestor
            tx.repo_mut().rebase_descendants()?;
        } else {
            // Parent→Child or Unrelated: modify source first
            if abandon_source {
                tx.repo_mut().record_abandoned_commit(&from);
            } else {
                tx.repo_mut()
                    .rewrite_commit(&from)
                    .set_tree(remainder_tree)
                    .write()?;
            }

            if from_is_ancestor {
                // Parent→Child: rebase descendants first, then apply hunk to the rebased destination
                let mut rebase_map = std::collections::HashMap::new();
                tx.repo_mut().rebase_descendants_with_options(
                    &RebaseOptions::default(),
                    |old_commit, rebased_commit| {
                        rebase_map.insert(
                            old_commit.id().clone(),
                            match rebased_commit {
                                RebasedCommit::Rewritten(new_commit) => new_commit.id().clone(),
                                RebasedCommit::Abandoned { parent_id } => parent_id,
                            },
                        );
                    },
                )?;

                // The destination was rebased onto the modified source, so its tree changed.
                // Recompute the hunk application against the rebased tree.
                let rebased_to_id = rebase_map
                    .get(to.id())
                    .ok_or_else(|| anyhow!("descendant to_commit not found in rebase map"))?
                    .clone();
                to = tx.repo().store().get_commit(&rebased_to_id)?;
                new_to_tree = to
                    .tree()
                    .merge(base_tree.clone(), sibling_tree.clone())
                    .await?;
            }

            // Apply hunk to destination
            tx.repo_mut()
                .rewrite_commit(&to)
                .set_tree(new_to_tree)
                .set_description(description)
                .write()?;

            // Rebase all descendants as usual
            tx.repo_mut().rebase_descendants()?;
        }

        match ws.finish_transaction(
            tx,
            format!(
                "move hunk in {} from {} to {}",
                self.path.repo_path,
                from.id().hex(),
                to.id().hex()
            ),
        )? {
            Some(new_status) => Ok(MutationResult::Updated { new_status }),
            None => Ok(MutationResult::Unchanged),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for CopyHunk {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let mut tx = ws.start_transaction().await?;

        let from = ws.resolve_single_commit(&self.from_id)?;
        let to = ws.resolve_single_change(&self.to_id)?;
        let repo_path = RepoPath::from_internal_string(&self.path.repo_path)?;

        if ws.check_immutable(vec![to.id().clone()])? {
            precondition!("Revision is immutable");
        }

        let store = tx.repo().store();
        let to_tree = to.tree();

        // vheck for conflicts in destination
        let to_path_value = to_tree.path_value(repo_path)?;
        if to_path_value.into_resolved().is_err() {
            precondition!("Cannot restore hunk: destination file has conflicts");
        }

        // read destination content
        let to_content = read_file_content(store, &to_tree, repo_path).await?;
        let to_text = String::from_utf8_lossy(&to_content);
        let to_lines: Vec<&str> = to_text.lines().collect();

        // validate destination bounds
        let to_start_0based = self.hunk.location.to_file.start.saturating_sub(1);
        let to_end_0based = to_start_0based + self.hunk.location.to_file.len;
        if to_end_0based > to_lines.len() {
            precondition!(
                "Hunk location out of bounds: file has {} lines, hunk requires lines {}-{}",
                to_lines.len(),
                self.hunk.location.to_file.start,
                to_end_0based
            );
        }

        // validate destination content
        let expected_to_lines: Vec<&str> = self
            .hunk
            .lines
            .lines
            .iter()
            .filter(|line| line.starts_with(' ') || line.starts_with('+'))
            .map(|line| line[1..].trim_end())
            .collect();
        let actual_to_lines: Vec<&str> = to_lines[to_start_0based..to_end_0based]
            .iter()
            .map(|line| line.trim_end())
            .collect();

        if expected_to_lines.len() != actual_to_lines.len() {
            return Err(anyhow!(
                "Hunk validation failed: expected {} lines, found {} lines at destination",
                expected_to_lines.len(),
                actual_to_lines.len()
            ));
        }

        for (i, (expected, actual)) in expected_to_lines
            .iter()
            .zip(actual_to_lines.iter())
            .enumerate()
        {
            if expected != actual {
                return Err(anyhow!(
                    "Hunk validation failed at line {}: expected '{}', found '{}'",
                    to_start_0based + i + 1,
                    expected,
                    actual
                ));
            }
        }

        // read source content
        let from_tree = from.tree();
        let from_content = read_file_content(store, &from_tree, repo_path).await?;
        let from_text = String::from_utf8_lossy(&from_content);
        let from_lines: Vec<&str> = from_text.lines().collect();

        // validate source bounds
        let from_start_0based = self.hunk.location.from_file.start.saturating_sub(1);
        let from_end_0based = from_start_0based + self.hunk.location.from_file.len;
        if from_end_0based > from_lines.len() {
            precondition!(
                "Source hunk location out of bounds: file has {} lines, hunk requires lines {}-{}",
                from_lines.len(),
                self.hunk.location.from_file.start,
                from_end_0based
            );
        }

        // extract source region
        let source_region_lines = &from_lines[from_start_0based..from_end_0based];

        // construct destination content and check whether anything changed
        let mut new_to_lines = Vec::new();
        new_to_lines.extend(to_lines[..to_start_0based].iter().map(|s| s.to_string()));
        new_to_lines.extend(source_region_lines.iter().map(|s| s.to_string()));
        new_to_lines.extend(to_lines[to_end_0based..].iter().map(|s| s.to_string()));

        let ends_with_newline = to_content.ends_with(b"\n");
        let mut new_to_content = Vec::new();
        let num_lines = new_to_lines.len();
        for (i, line) in new_to_lines.iter().enumerate() {
            new_to_content.extend_from_slice(line.as_bytes());
            if i < num_lines - 1 {
                new_to_content.push(b'\n');
            }
        }
        if ends_with_newline && !new_to_content.is_empty() && !new_to_content.ends_with(b"\n") {
            new_to_content.push(b'\n');
        }

        if new_to_content == to_content {
            return Ok(MutationResult::Unchanged);
        }

        // create new destination tree with preserved executable bit
        let new_to_blob_id = store
            .write_file(repo_path, &mut new_to_content.as_slice())
            .await?;

        let to_executable = match to_tree.path_value(repo_path)?.into_resolved() {
            Ok(Some(TreeValue::File { executable, .. })) => executable,
            _ => false,
        };

        let new_to_tree =
            update_tree_entry(store, &to_tree, repo_path, new_to_blob_id, to_executable)?;

        // rewrite destination
        tx.repo_mut()
            .rewrite_commit(&to)
            .set_tree(new_to_tree)
            .write()?;

        tx.repo_mut().rebase_descendants()?;

        match ws.finish_transaction(
            tx,
            format!(
                "restore hunk in {} from {} into {}",
                self.path.repo_path, self.from_id.hex, self.to_id.commit.hex
            ),
        )? {
            Some(new_status) => Ok(MutationResult::Updated { new_status }),
            None => Ok(MutationResult::Unchanged),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for GitPush {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = match self.as_ref() {
            GitPush::AllBookmarks { remote_name } => {
                run_jj(["git", "push", "--remote", remote_name])
                    .current_dir(ws.workspace.workspace_root())
                    .output()
            }
            GitPush::AllRemotes { branch_ref } => {
                run_jj(["git", "push", "--bookmark", branch_ref.as_branch()?])
                    .current_dir(ws.workspace.workspace_root())
                    .output()
            }
            GitPush::RemoteBookmark {
                remote_name,
                branch_ref,
            } => run_jj(["git", "push"])
                .args(["--bookmark", branch_ref.as_branch()?])
                .args(["--remote", remote_name])
                .current_dir(ws.workspace.workspace_root())
                .output(),
        };

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj git push: {e}")),
        }
    }
}

#[async_trait::async_trait(?Send)]
impl Mutation for GitFetch {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = match self.as_ref() {
            GitFetch::AllBookmarks { remote_name } => run_jj(["git", "fetch"])
                .args(["--remote", remote_name])
                .current_dir(ws.workspace.workspace_root())
                .output(),
            GitFetch::AllRemotes { branch_ref } => run_jj(["git", "fetch"])
                .args(["--branch", branch_ref.as_branch()?])
                .current_dir(ws.workspace.workspace_root())
                .output(),
            GitFetch::RemoteBookmark {
                remote_name,
                branch_ref,
            } => run_jj(["git", "fetch"])
                .args(["--branch", branch_ref.as_branch()?])
                .args(["--remote", remote_name])
                .current_dir(ws.workspace.workspace_root())
                .output(),
        };

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    Ok(MutationResult::Updated {
                        new_status: ws.format_status(),
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).trim().into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj git fetch: {e}")),
        }
    }
}

// this is another case where it would be nice if we could reuse jj-cli's error messages
#[async_trait::async_trait(?Send)]
impl Mutation for UndoOperation {
    async fn execute(self: Box<Self>, ws: &mut WorkspaceSession) -> Result<MutationResult> {
        let result = run_jj(["undo"])
            .current_dir(ws.workspace.workspace_root())
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    ws.load_at_head()?;
                    let working_copy = ws.get_commit(ws.wc_id())?;
                    let new_selection = ws.format_header(&working_copy, None)?;

                    Ok(MutationResult::UpdatedSelection {
                        new_status: ws.format_status(),
                        new_selection,
                    })
                } else {
                    Ok(MutationResult::PreconditionError {
                        message: String::from_utf8_lossy(&output.stderr).into(),
                    })
                }
            }
            Err(e) => Err(anyhow!("Failed to execute jj undo: {e}")),
        }
    }
}

fn combine_messages(source: &Commit, destination: &Commit, abandon_source: bool) -> String {
    if abandon_source {
        if source.description().is_empty() {
            destination.description().to_owned()
        } else if destination.description().is_empty() {
            source.description().to_owned()
        } else {
            destination.description().to_owned() + "\n" + source.description()
        }
    } else {
        destination.description().to_owned()
    }
}

async fn read_file_content(
    store: &Arc<Store>,
    tree: &MergedTree,
    path: &RepoPath,
) -> Result<Vec<u8>> {
    let entry = tree.path_value(path)?;
    match entry.into_resolved() {
        Ok(Some(TreeValue::File { id, .. })) => {
            let mut reader = store.read_file(path, &id).await?;
            let mut content = Vec::new();
            reader.read_to_end(&mut content).await?;
            Ok(content)
        }
        Ok(Some(_)) => Ok(Vec::new()),
        Ok(None) => Ok(Vec::new()),
        Err(_) => {
            // handle conflicts by materializing them
            match conflicts::materialize_tree_value(store, path, tree.path_value(path)?).await? {
                MaterializedTreeValue::FileConflict(file) => {
                    let mut content = Vec::new();
                    conflicts::materialize_merge_result(
                        &file.contents,
                        &mut content,
                        &ConflictMaterializeOptions {
                            marker_style: ConflictMarkerStyle::Git,
                            marker_len: None,
                            merge: MergeOptions {
                                hunk_level: FileMergeHunkLevel::Line,
                                same_change: SameChange::Accept,
                            },
                        },
                    )?;
                    Ok(content)
                }
                _ => Ok(Vec::new()),
            }
        }
    }
}

/// Construct the sibling tree's file content by applying a hunk to its base.
///
/// The hunk was computed as a diff between `base` (the source commit's parent) and the
/// source commit. This function applies that diff to reconstruct the file content that
/// would exist in a virtual "sibling" commit containing only this hunk.
///
/// Line numbers must match exactly since the hunk was computed against this base.
fn apply_hunk_to_base(base_content: &[u8], hunk: &crate::messages::ChangeHunk) -> Result<Vec<u8>> {
    let base_text = String::from_utf8_lossy(base_content);
    let base_lines: Vec<&str> = base_text.lines().collect();
    let ends_with_newline = base_content.ends_with(b"\n");

    let mut result_lines: Vec<String> = Vec::new();
    let hunk_lines = hunk.lines.lines.iter().peekable();

    // Convert 1-indexed line number to 0-indexed
    let hunk_start = hunk.location.from_file.start.saturating_sub(1);

    // Copy lines before the hunk unchanged
    result_lines.extend(base_lines[..hunk_start].iter().map(|s| s.to_string()));
    let mut base_idx = hunk_start;

    for diff_line in hunk_lines {
        if diff_line.starts_with(' ') || diff_line.starts_with('-') {
            // Context or deletion: verify the base content matches
            let expected = &diff_line[1..];
            if base_idx < base_lines.len() && base_lines[base_idx].trim_end() == expected.trim_end()
            {
                if diff_line.starts_with(' ') {
                    result_lines.push(base_lines[base_idx].to_string());
                }
                // Deletions are consumed but not added to result
                base_idx += 1;
            } else {
                anyhow::bail!(
                    "Hunk mismatch at line {}: expected '{}', found '{}'",
                    base_idx + 1,
                    expected.trim_end(),
                    base_lines.get(base_idx).map_or("<EOF>", |l| l.trim_end())
                );
            }
        } else if let Some(added) = diff_line.strip_prefix('+') {
            // Addition: include in result
            let added = added.trim_end_matches('\n');
            result_lines.push(added.to_string());
        } else {
            anyhow::bail!("Malformed diff line: {}", diff_line);
        }
    }

    // Copy remaining lines after the hunk unchanged
    result_lines.extend(base_lines[base_idx..].iter().map(|s| s.to_string()));

    let mut result_bytes = Vec::new();
    let num_lines = result_lines.len();
    for (i, line) in result_lines.iter().enumerate() {
        result_bytes.extend_from_slice(line.as_bytes());
        if i < num_lines - 1 {
            result_bytes.push(b'\n');
        }
    }

    if ends_with_newline && !result_bytes.is_empty() && !result_bytes.ends_with(b"\n") {
        result_bytes.push(b'\n');
    }

    Ok(result_bytes)
}

fn update_tree_entry(
    _store: &Arc<jj_lib::store::Store>,
    original_tree: &MergedTree,
    path: &RepoPath,
    new_blob: FileId,
    executable: bool,
) -> Result<MergedTree, anyhow::Error> {
    let mut builder = MergedTreeBuilder::new(original_tree.clone());
    builder.set_or_remove(
        path.to_owned(),
        Merge::normal(TreeValue::File {
            id: new_blob,
            executable,
            copy_id: CopyId::placeholder(),
        }),
    );
    let new_tree = builder.write_tree()?;
    Ok(new_tree)
}

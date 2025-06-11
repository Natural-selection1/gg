// 版本控制查询模块
// 该模块负责处理日志查询、修订版本查询和远程仓库查询等功能
// 主要用于git版本控制系统的数据展示和交互

use std::{
    borrow::Borrow,
    io::Write,
    iter::{Peekable, Skip},
    mem,
    ops::Range,
};

use anyhow::{Result, anyhow};

use futures_util::{StreamExt, try_join};
use gix::bstr::ByteVec;
use itertools::Itertools;
use jj_cli::diff_util::{LineCompareMode, LineDiffOptions};
use jj_lib::{
    backend::CommitId,
    conflicts::{self, ConflictMarkerStyle, MaterializedFileValue, MaterializedTreeValue},
    diff::{
        CompareBytesExactly, CompareBytesIgnoreAllWhitespace, CompareBytesIgnoreWhitespaceAmount,
        Diff, DiffHunk, DiffHunkKind, find_line_ranges,
    },
    graph::{GraphEdge, GraphEdgeType, TopoGroupedGraphIterator},
    matchers::EverythingMatcher,
    merged_tree::{TreeDiffEntry, TreeDiffStream},
    ref_name::{RefNameBuf, RemoteNameBuf, RemoteRefSymbol},
    repo::Repo,
    repo_path::RepoPath,
    revset::{Revset, RevsetEvaluationError},
    rewrite,
};
use pollster::FutureExt;

use crate::messages::{
    ChangeHunk, ChangeKind, FileRange, HunkLocation, LogCoordinates, LogLine, LogPage, LogRow,
    MultilineString, RevChange, RevConflict, RevId, RevResult,
};

use super::WorkspaceSession;

/// 日志主干结构体
/// 用于跟踪日志图中的垂直线，表示提交之间的连接关系
struct LogStem {
    /// 源坐标位置
    source: LogCoordinates,
    /// 目标提交ID
    target: CommitId,
    /// 是否为间接连接（合并等）
    indirect: bool,
    /// 是否为插入的连接
    was_inserted: bool,
    /// 是否已知为不可变提交
    known_immutable: bool,
}

/// 查询状态结构体
/// 用于初始化或重启查询时的状态管理
pub struct QueryState {
    /// 每页最大行数
    page_size: usize,
    /// 已产生的行数
    next_row: usize,
    /// 正在进行的垂直线；节点将被放置在这些线上或周围
    stems: Vec<Option<LogStem>>,
}

impl QueryState {
    /// 创建新的查询状态实例
    ///
    /// # 参数
    /// * `page_size` - 每页显示的行数
    pub fn new(page_size: usize) -> QueryState {
        QueryState {
            page_size,
            next_row: 0,
            stems: Vec::new(),
        }
    }
}

/// 查询会话结构体
/// 表示一个活跃的查询实例，包含查询状态和迭代器
pub struct QuerySession<'q, 'w: 'q> {
    /// 工作空间会话引用
    pub ws: &'q WorkspaceSession<'w>,
    /// 查询状态
    pub state: QueryState,
    /// 拓扑分组的图迭代器，用于遍历提交历史
    #[allow(clippy::type_complexity)]
    iter: Peekable<
        Skip<
            TopoGroupedGraphIterator<
                CommitId,
                Box<
                    dyn Iterator<
                            Item = Result<
                                (CommitId, Vec<GraphEdge<CommitId>>),
                                RevsetEvaluationError,
                            >,
                        > + 'q,
                >,
            >,
        >,
    >,
    /// 判断提交是否不可变的函数
    #[allow(clippy::type_complexity)]
    is_immutable: Box<dyn Fn(&CommitId) -> Result<bool, RevsetEvaluationError> + 'q>,
}

impl<'q, 'w> QuerySession<'q, 'w> {
    /// 创建新的查询会话
    ///
    /// # 参数
    /// * `ws` - 工作空间会话引用
    /// * `revset` - 修订集合
    /// * `state` - 查询状态
    pub fn new(
        ws: &'q WorkspaceSession<'w>,
        revset: &'q dyn Revset,
        state: QueryState,
    ) -> QuerySession<'q, 'w> {
        // 创建拓扑分组的图迭代器，跳过已处理的行
        let iter = TopoGroupedGraphIterator::new(revset.iter_graph())
            .skip(state.next_row)
            .peekable();

        // 获取不可变修订集合用于判断提交是否可变
        let immutable_revset = ws.evaluate_immutable().unwrap();
        let is_immutable = immutable_revset.containing_fn();

        QuerySession {
            ws,
            iter,
            state,
            is_immutable,
        }
    }

    /// 获取一页日志数据
    ///
    /// # 返回值
    /// 返回包含日志行和是否有更多数据的日志页面
    pub fn get_page(&mut self) -> Result<LogPage> {
        // 输出要绘制的行
        let mut rows: Vec<LogRow> = Vec::with_capacity(self.state.page_size);
        let mut row = self.state.next_row;
        let max = row + self.state.page_size;

        // 获取根提交ID
        let root_id = self.ws.repo().store().root_commit_id().clone();

        // 遍历提交历史
        while let Some(Ok((commit_id, commit_edges))) = self.iter.next() {
            // 当前行要绘制的输出线
            let mut lines: Vec<LogLine> = Vec::new();

            // 查找指向当前节点的现有主干
            let mut column = self.state.stems.len();
            let mut stem_known_immutable = false;
            let mut padding = 0; // 用于偏移提交摘要，越过一些边

            // 查找指向当前提交的主干
            if let Some(slot) = self.find_stem_for_commit(&commit_id) {
                column = slot;
                padding = self.state.stems.len() - column - 1;
            }

            // 终止任何现有主干，从末尾移除或留下空隙
            if column < self.state.stems.len() {
                if let Some(terminated_stem) = &self.state.stems[column] {
                    stem_known_immutable = terminated_stem.known_immutable;
                    // 根据主干是否被插入来决定连线类型
                    lines.push(if terminated_stem.was_inserted {
                        LogLine::FromNode {
                            indirect: terminated_stem.indirect,
                            source: terminated_stem.source,
                            target: LogCoordinates(column, row),
                        }
                    } else {
                        LogLine::ToNode {
                            indirect: terminated_stem.indirect,
                            source: terminated_stem.source,
                            target: LogCoordinates(column, row),
                        }
                    });
                }
                self.state.stems[column] = None;
            }
            // 否则，填充任何可能存在的空隙
            else {
                for (slot, stem) in self.state.stems.iter().enumerate() {
                    if stem.is_none() {
                        column = slot;
                        padding = self.state.stems.len() - slot - 1;
                        break;
                    }
                }
            }

            // 确定提交的不可变性
            let known_immutable = if stem_known_immutable {
                Some(true)
            } else {
                Some((self.is_immutable)(&commit_id)?)
            };

            // 格式化提交头部信息
            let header = self
                .ws
                .format_header(&self.ws.get_commit(&commit_id)?, known_immutable)?;

            // 移除右边缘的空主干
            let empty_stems = self
                .state
                .stems
                .iter()
                .rev()
                .take_while(|stem| stem.is_none())
                .count();
            self.state
                .stems
                .truncate(self.state.stems.len() - empty_stems);

            // 将边合并到现有主干中或在右侧添加新主干
            let mut next_missing: Option<CommitId> = None;
            'edges: for edge in commit_edges.iter() {
                // 处理缺失的边（通常是合并点）
                if edge.edge_type == GraphEdgeType::Missing {
                    if edge.target == root_id {
                        continue;
                    } else {
                        next_missing = Some(edge.target.clone());
                    }
                }

                let indirect = edge.edge_type != GraphEdgeType::Direct;

                // 检查是否已有主干指向此目标
                for (slot, stem) in self.state.stems.iter().enumerate() {
                    if let Some(stem) = stem {
                        if stem.target == edge.target {
                            lines.push(LogLine::ToIntersection {
                                indirect,
                                source: LogCoordinates(column, row),
                                target: LogCoordinates(slot, row + 1),
                            });
                            continue 'edges;
                        }
                    }
                }

                // 在空主干中插入新的边
                for stem in self.state.stems.iter_mut() {
                    if stem.is_none() {
                        *stem = Some(LogStem {
                            source: LogCoordinates(column, row),
                            target: edge.target.clone(),
                            indirect,
                            was_inserted: true,
                            known_immutable: header.is_immutable,
                        });
                        continue 'edges;
                    }
                }

                // 在末尾添加新主干
                self.state.stems.push(Some(LogStem {
                    source: LogCoordinates(column, row),
                    target: edge.target.clone(),
                    indirect,
                    was_inserted: false,
                    known_immutable: header.is_immutable,
                }));
            }

            // 添加当前行到结果中
            rows.push(LogRow {
                revision: header,
                location: LogCoordinates(column, row),
                padding,
                lines,
            });
            row += 1;

            // 终止为缺失边创建的任何临时主干
            if let Some(slot) = next_missing
                .take()
                .and_then(|id| self.find_stem_for_commit(&id))
            {
                if let Some(terminated_stem) = &self.state.stems[slot] {
                    rows.last_mut().unwrap().lines.push(LogLine::ToMissing {
                        indirect: terminated_stem.indirect,
                        source: LogCoordinates(column, row - 1),
                        target: LogCoordinates(slot, row),
                    });
                }
                self.state.stems[slot] = None;
                row += 1;
            };

            // 检查是否达到页面大小限制
            if row == max {
                break;
            }
        }

        // 更新下一行位置
        self.state.next_row = row;
        Ok(LogPage {
            rows,
            has_more: self.iter.peek().is_some(),
        })
    }

    /// 查找指向指定提交的主干
    ///
    /// # 参数
    /// * `id` - 要查找的提交ID
    ///
    /// # 返回值
    /// 返回主干的索引位置，如果未找到则返回None
    fn find_stem_for_commit(&self, id: &CommitId) -> Option<usize> {
        for (slot, stem) in self.state.stems.iter().enumerate() {
            if let Some(LogStem { target, .. }) = stem {
                if target == id {
                    return Some(slot);
                }
            }
        }

        None
    }
}

/// 测试用的日志查询函数
///
/// # 参数
/// * `ws` - 工作空间会话
/// * `revset_str` - 修订集合字符串
/// * `max_results` - 最大结果数
#[cfg(test)]
pub fn query_log(ws: &WorkspaceSession, revset_str: &str, max_results: usize) -> Result<LogPage> {
    let state = QueryState::new(max_results);
    let revset = ws.evaluate_revset_str(revset_str)?;
    let mut session = QuerySession::new(ws, &*revset, state);
    session.get_page()
}

/// 查询指定修订版本的详细信息
///
/// # 参数
/// * `ws` - 工作空间会话
/// * `id` - 修订版本ID
///
/// # 返回值
/// 返回修订版本的详细信息，包括头部、父提交、变更和冲突
// XXX 这里重新加载了头部信息，而客户端已经有了
pub fn query_revision(ws: &WorkspaceSession, id: RevId) -> Result<RevResult> {
    // 解析提交ID
    let commit = match ws.resolve_optional_id(&id)? {
        Some(commit) => commit,
        None => return Ok(RevResult::NotFound { id }),
    };

    // 获取父提交并合并其树
    let commit_parents: Result<Vec<_>, _> = commit.parents().collect();
    let parent_tree = rewrite::merge_commit_trees(ws.repo(), &commit_parents?)?;
    let tree = commit.tree()?;

    // 收集冲突信息
    let mut conflicts = Vec::new();
    for (path, entry) in parent_tree.entries() {
        if let Ok(entry) = entry {
            if !entry.is_resolved() {
                // 物化树值以获取冲突内容
                match conflicts::materialize_tree_value(ws.repo().store(), &path, entry)
                    .block_on()?
                {
                    MaterializedTreeValue::FileConflict(file) => {
                        let mut hunk_content = vec![];
                        // 物化合并结果，生成冲突标记
                        conflicts::materialize_merge_result(
                            &file.contents,
                            ConflictMarkerStyle::default(),
                            &mut hunk_content,
                        )?;
                        let mut hunks = get_unified_hunks(3, &hunk_content, &[])?;
                        if let Some(hunk) = hunks.pop() {
                            conflicts.push(RevConflict {
                                path: ws.format_path(path)?,
                                hunk,
                            });
                        }
                    }
                    _ => {
                        log::warn!("nonresolved tree entry did not materialise as conflict");
                    }
                }
            }
        }
    }

    // 收集变更信息
    let mut changes = Vec::new();
    let tree_diff = parent_tree.diff_stream(&tree, &EverythingMatcher);
    format_tree_changes(ws, &mut changes, tree_diff).block_on()?;

    // 格式化头部信息
    let header = ws.format_header(&commit, None)?;

    // 格式化父提交信息
    let parents = commit
        .parents()
        .map_ok(|p| {
            ws.format_header(
                &p,
                if header.is_immutable {
                    Some(true)
                } else {
                    None
                },
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    Ok(RevResult::Detail {
        header,
        parents,
        changes,
        conflicts,
    })
}

/// 查询远程仓库列表
///
/// # 参数
/// * `ws` - 工作空间会话
/// * `tracking_branch` - 可选的跟踪分支名称
///
/// # 返回值
/// 返回匹配的远程仓库名称列表
pub fn query_remotes(
    ws: &WorkspaceSession,
    tracking_branch: Option<String>,
) -> Result<Vec<String>> {
    // 获取git仓库实例
    let git_repo = match ws.git_repo()? {
        Some(git_repo) => git_repo,
        None => return Err(anyhow!("No git backend")),
    };

    // 获取所有远程仓库
    let all_remotes: Vec<String> = git_repo
        .remotes()?
        .into_iter()
        .filter_map(|remote| remote.map(|remote| remote.to_owned()))
        .collect();

    // 根据跟踪分支过滤远程仓库
    let matching_remotes = match tracking_branch {
        Some(branch_name) => all_remotes
            .into_iter()
            .filter(|remote_name| {
                let remote_name_ref = RemoteNameBuf::from(remote_name);
                let branch_name_ref = RefNameBuf::from(branch_name.clone());
                let remote_ref_symbol = RemoteRefSymbol {
                    name: &branch_name_ref,
                    remote: &remote_name_ref,
                };
                let remote_ref = ws.view().get_remote_bookmark(remote_ref_symbol);
                !remote_ref.is_absent() && remote_ref.is_tracked()
            })
            .collect(),
        None => all_remotes,
    };

    Ok(matching_remotes)
}

/// 异步格式化树变更
///
/// # 参数
/// * `ws` - 工作空间会话
/// * `changes` - 变更列表（可变引用）
/// * `tree_diff` - 树差异流
async fn format_tree_changes(
    ws: &WorkspaceSession<'_>,
    changes: &mut Vec<RevChange>,
    mut tree_diff: TreeDiffStream<'_>,
) -> Result<()> {
    let store = ws.repo().store();

    // 遍历树差异条目
    while let Some(TreeDiffEntry { path, values }) = tree_diff.next().await {
        let (before, after) = values?;

        // 确定变更类型
        let kind = if before.is_present() && after.is_present() {
            ChangeKind::Modified
        } else if before.is_absent() {
            ChangeKind::Added
        } else {
            ChangeKind::Deleted
        };

        // 检查是否有冲突
        let has_conflict = !after.is_resolved();

        // 异步物化变更前后的树值
        let before_future = conflicts::materialize_tree_value(store, &path, before);
        let after_future = conflicts::materialize_tree_value(store, &path, after);
        let (before_value, after_value) = try_join!(before_future, after_future)?;

        // 获取变更块
        let hunks = get_value_hunks(3, &path, before_value, after_value)?;

        changes.push(RevChange {
            path: ws.format_path(path)?,
            kind,
            has_conflict,
            hunks,
        });
    }
    Ok(())
}

/// 获取值变更块
///
/// # 参数
/// * `num_context_lines` - 上下文行数
/// * `path` - 仓库路径
/// * `left_value` - 左侧（旧）值
/// * `right_value` - 右侧（新）值
///
/// # 返回值
/// 返回变更块列表
fn get_value_hunks(
    num_context_lines: usize,
    path: &RepoPath,
    left_value: MaterializedTreeValue,
    right_value: MaterializedTreeValue,
) -> Result<Vec<ChangeHunk>> {
    if left_value.is_absent() {
        // 仅有右侧值（新增文件）
        let right_part = get_value_contents(path, right_value)?;
        get_unified_hunks(num_context_lines, &[], &right_part)
    } else if right_value.is_present() {
        // 两侧都有值（修改文件）
        let left_part = get_value_contents(path, left_value)?;
        let right_part = get_value_contents(path, right_value)?;
        get_unified_hunks(num_context_lines, &left_part, &right_part)
    } else {
        // 仅有左侧值（删除文件）
        let left_part = get_value_contents(path, left_value)?;
        get_unified_hunks(num_context_lines, &left_part, &[])
    }
}

/// 获取值内容
///
/// # 参数
/// * `path` - 仓库路径
/// * `value` - 物化的树值
///
/// # 返回值
/// 返回值的字节内容
fn get_value_contents(path: &RepoPath, value: MaterializedTreeValue) -> Result<Vec<u8>> {
    match value {
        MaterializedTreeValue::Absent => Err(anyhow!(
            "Absent path {path:?} in diff should have been handled by caller"
        )),
        MaterializedTreeValue::File(MaterializedFileValue { mut reader, .. }) => {
            let mut contents = vec![];
            reader.read_to_end(&mut contents)?;

            // 使用与git相同的启发式方法检测二进制文件
            let start = &contents[..8000.min(contents.len())];
            let is_binary = start.contains(&b'\0');
            if is_binary {
                contents.clear();
                contents.push_str("(binary)");
            }
            Ok(contents)
        }
        MaterializedTreeValue::Symlink { target, .. } => Ok(target.into_bytes()),
        MaterializedTreeValue::GitSubmodule(_) => Ok("(submodule)".to_owned().into_bytes()),
        MaterializedTreeValue::FileConflict(file) => {
            // 处理文件冲突，生成冲突标记
            let mut hunk_content = vec![];
            conflicts::materialize_merge_result(
                &file.contents,
                ConflictMarkerStyle::default(),
                &mut hunk_content,
            )?;
            Ok(hunk_content)
        }
        MaterializedTreeValue::OtherConflict { id } => Ok(id.describe().into_bytes()),
        MaterializedTreeValue::Tree(_) => Err(anyhow!("Unexpected tree in diff at path {path:?}")),
        MaterializedTreeValue::AccessDenied(error) => Err(anyhow!(error)),
    }
}

/// 获取统一格式的差异块
///
/// # 参数
/// * `num_context_lines` - 上下文行数
/// * `left_content` - 左侧（旧）内容
/// * `right_content` - 右侧（新）内容
///
/// # 返回值
/// 返回变更块列表
fn get_unified_hunks(
    num_context_lines: usize,
    left_content: &[u8],
    right_content: &[u8],
) -> Result<Vec<ChangeHunk>> {
    let mut hunks = Vec::new();

    // 生成统一格式差异块
    for hunk in unified_diff_hunks(
        left_content,
        right_content,
        &UnifiedDiffOptions {
            context: num_context_lines,
            line_diff: LineDiffOptions {
                compare_mode: LineCompareMode::Exact,
            },
        },
    ) {
        // 创建块位置信息
        let location = HunkLocation {
            from_file: FileRange {
                start: hunk.left_line_range.start,
                len: hunk.left_line_range.len(),
            },
            to_file: FileRange {
                start: hunk.right_line_range.start,
                len: hunk.right_line_range.len(),
            },
        };

        // 格式化差异行
        let mut lines = Vec::new();
        for (line_type, tokens) in hunk.lines {
            let mut formatter: Vec<u8> = vec![];
            // 添加行类型标记
            match line_type {
                DiffLineType::Context => {
                    write!(formatter, " ")?;
                }
                DiffLineType::Removed => {
                    write!(formatter, "-")?;
                }
                DiffLineType::Added => {
                    write!(formatter, "+")?;
                }
            }

            // 添加标记内容
            for (token_type, content) in tokens {
                match token_type {
                    DiffTokenType::Matching => formatter.write_all(content)?,
                    DiffTokenType::Different => formatter.write_all(content)?, // XXX 为GUI显示标记此处
                }
            }

            lines.push(std::str::from_utf8(&formatter)?.into());
        }

        hunks.push(ChangeHunk {
            location,
            lines: MultilineString { lines },
        });
    }

    Ok(hunks)
}

/**************************/
/* 从 jj_cli::diff_util 复制的代码 */
/**************************/

/// 统一差异选项
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnifiedDiffOptions {
    /// 要显示的上下文行数
    pub context: usize,
    /// 行的标记化和比较方式
    pub line_diff: LineDiffOptions,
}

/// 差异行类型枚举
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLineType {
    /// 上下文行（未变更）
    Context,
    /// 删除的行
    Removed,
    /// 添加的行
    Added,
}

/// 差异标记类型枚举
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffTokenType {
    /// 匹配的标记
    Matching,
    /// 不同的标记
    Different,
}

/// 差异标记向量类型别名
type DiffTokenVec<'content> = Vec<(DiffTokenType, &'content [u8])>;

/// 统一差异块结构体
struct UnifiedDiffHunk<'content> {
    /// 左侧行范围
    left_line_range: Range<usize>,
    /// 右侧行范围
    right_line_range: Range<usize>,
    /// 行内容和类型
    lines: Vec<(DiffLineType, DiffTokenVec<'content>)>,
}

impl<'content> UnifiedDiffHunk<'content> {
    /// 扩展上下文行
    fn extend_context_lines(&mut self, lines: impl IntoIterator<Item = &'content [u8]>) {
        let old_len = self.lines.len();
        self.lines.extend(lines.into_iter().map(|line| {
            let tokens = vec![(DiffTokenType::Matching, line)];
            (DiffLineType::Context, tokens)
        }));
        self.left_line_range.end += self.lines.len() - old_len;
        self.right_line_range.end += self.lines.len() - old_len;
    }

    /// 扩展删除的行
    fn extend_removed_lines(&mut self, lines: impl IntoIterator<Item = DiffTokenVec<'content>>) {
        let old_len = self.lines.len();
        self.lines
            .extend(lines.into_iter().map(|line| (DiffLineType::Removed, line)));
        self.left_line_range.end += self.lines.len() - old_len;
    }

    /// 扩展添加的行
    fn extend_added_lines(&mut self, lines: impl IntoIterator<Item = DiffTokenVec<'content>>) {
        let old_len = self.lines.len();
        self.lines
            .extend(lines.into_iter().map(|line| (DiffLineType::Added, line)));
        self.right_line_range.end += self.lines.len() - old_len;
    }
}

/// 生成统一格式差异块
///
/// # 参数
/// * `left_content` - 左侧内容
/// * `right_content` - 右侧内容
/// * `options` - 差异选项
///
/// # 返回值
/// 返回统一格式差异块列表
fn unified_diff_hunks<'content>(
    left_content: &'content [u8],
    right_content: &'content [u8],
    options: &UnifiedDiffOptions,
) -> Vec<UnifiedDiffHunk<'content>> {
    let mut hunks = vec![];
    let mut current_hunk = UnifiedDiffHunk {
        left_line_range: 1..1,
        right_line_range: 1..1,
        lines: vec![],
    };

    // 按行进行差异分析
    let diff = diff_by_line([left_content, right_content], &options.line_diff);
    let mut diff_hunks = diff.hunks().peekable();

    while let Some(hunk) = diff_hunks.next() {
        match hunk.kind {
            DiffHunkKind::Matching => {
                // 仅使用右侧（即新）内容。我们可以单独计算跳过的行数，
                // 但上下文行的数量应该与显示的内容匹配。
                let [_, right] = hunk.contents[..].try_into().unwrap();
                let mut lines = right.split_inclusive(|b| *b == b'\n').fuse();

                if !current_hunk.lines.is_empty() {
                    // 前一个块行应该是删除/添加的。
                    current_hunk.extend_context_lines(lines.by_ref().take(options.context));
                }

                let before_lines = if diff_hunks.peek().is_some() {
                    lines.by_ref().rev().take(options.context).collect()
                } else {
                    vec![] // 没有更多块
                };

                let num_skip_lines = lines.count();
                if num_skip_lines > 0 {
                    let left_start = current_hunk.left_line_range.end + num_skip_lines;
                    let right_start = current_hunk.right_line_range.end + num_skip_lines;
                    if !current_hunk.lines.is_empty() {
                        hunks.push(current_hunk);
                    }
                    current_hunk = UnifiedDiffHunk {
                        left_line_range: left_start..left_start,
                        right_line_range: right_start..right_start,
                        lines: vec![],
                    };
                }

                // 如果有的话，下一个块应该是DiffHunk::Different类型。
                current_hunk.extend_context_lines(before_lines.into_iter().rev());
            }
            DiffHunkKind::Different => {
                // 将差异块按词分解成行
                let (left_lines, right_lines) =
                    unzip_diff_hunks_to_lines(Diff::by_word(hunk.contents).hunks());
                current_hunk.extend_removed_lines(left_lines);
                current_hunk.extend_added_lines(right_lines);
            }
        }
    }

    if !current_hunk.lines.is_empty() {
        hunks.push(current_hunk);
    }
    hunks
}

/// 将`(left, right)`块对拆分为`(left_lines, right_lines)`
///
/// # 参数
/// * `diff_hunks` - 差异块迭代器
///
/// # 返回值
/// 返回左右两侧的行标记向量
fn unzip_diff_hunks_to_lines<'content, I>(
    diff_hunks: I,
) -> (Vec<DiffTokenVec<'content>>, Vec<DiffTokenVec<'content>>)
where
    I: IntoIterator,
    I::Item: Borrow<DiffHunk<'content>>,
{
    let mut left_lines: Vec<DiffTokenVec<'content>> = vec![];
    let mut right_lines: Vec<DiffTokenVec<'content>> = vec![];
    let mut left_tokens: DiffTokenVec<'content> = vec![];
    let mut right_tokens: DiffTokenVec<'content> = vec![];

    for hunk in diff_hunks {
        let hunk = hunk.borrow();
        match hunk.kind {
            DiffHunkKind::Matching => {
                // TODO: 添加对不匹配上下文的支持
                debug_assert!(hunk.contents.iter().all_equal());
                for token in hunk.contents[0].split_inclusive(|b| *b == b'\n') {
                    left_tokens.push((DiffTokenType::Matching, token));
                    right_tokens.push((DiffTokenType::Matching, token));
                    if token.ends_with(b"\n") {
                        left_lines.push(mem::take(&mut left_tokens));
                        right_lines.push(mem::take(&mut right_tokens));
                    }
                }
            }
            DiffHunkKind::Different => {
                let [left, right] = hunk.contents[..]
                    .try_into()
                    .expect("hunk should have exactly two inputs");

                // 处理左侧（删除的）内容
                for token in left.split_inclusive(|b| *b == b'\n') {
                    left_tokens.push((DiffTokenType::Different, token));
                    if token.ends_with(b"\n") {
                        left_lines.push(mem::take(&mut left_tokens));
                    }
                }

                // 处理右侧（添加的）内容
                for token in right.split_inclusive(|b| *b == b'\n') {
                    right_tokens.push((DiffTokenType::Different, token));
                    if token.ends_with(b"\n") {
                        right_lines.push(mem::take(&mut right_tokens));
                    }
                }
            }
        }
    }

    // 处理未完成的行
    if !left_tokens.is_empty() {
        left_lines.push(left_tokens);
    }
    if !right_tokens.is_empty() {
        right_lines.push(right_tokens);
    }
    (left_lines, right_lines)
}

/// 按行进行差异分析
///
/// # 参数
/// * `inputs` - 输入内容迭代器
/// * `options` - 行差异选项
///
/// # 返回值
/// 返回差异对象
fn diff_by_line<'input, T: AsRef<[u8]> + ?Sized + 'input>(
    inputs: impl IntoIterator<Item = &'input T>,
    options: &LineDiffOptions,
) -> Diff<'input> {
    // TODO: 如果我们添加--ignore-blank-lines，其标记器将必须将空行附加到前面的范围。
    // 也许它也可以作为后处理（类似于refine_changed_regions()）来实现，
    // 该处理在空行之间扩展未更改的区域。
    match options.compare_mode {
        LineCompareMode::Exact => {
            Diff::for_tokenizer(inputs, find_line_ranges, CompareBytesExactly)
        }
        LineCompareMode::IgnoreAllSpace => {
            Diff::for_tokenizer(inputs, find_line_ranges, CompareBytesIgnoreAllWhitespace)
        }
        LineCompareMode::IgnoreSpaceChange => {
            Diff::for_tokenizer(inputs, find_line_ranges, CompareBytesIgnoreWhitespaceAmount)
        }
    }
}

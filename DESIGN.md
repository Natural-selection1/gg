## 设计原则 | Design Principles

The primary metaphor is _direct manipulation_.
GG aims to present a view of the repository's conceptual contents - revisions, changes to files, synced refs and maybe more -
which can be modified, using right-click and drag-drop, to 'edit' the repo as a whole.
主要的设计理念是*直接操作*。
GG旨在展示仓库的概念性内容视图 - 包括修订版本、文件更改、同步引用等 -
这些内容可以通过右键点击和拖放来"编辑"整个仓库。

Jujutsu CLI commands sometimes have a lot of options (`rebase`) or are partially redundant for convenience (`move`, `squash`).
This is good for scripting, but some use cases demand interactivity - reordering multiple commits, for example.
Hopefully, `gg` can complement `jj` by providing decomposed means to achieve some of the same tasks, with immediate visual feedback.
Jujutsu CLI命令有时会有很多选项(`rebase`)或为了便利性而部分冗余(`move`, `squash`)。
这对脚本编写很有用,但某些用例需要交互性 - 例如重新排序多个提交。
希望`gg`能够通过提供分解的方式来实现一些相同的任务,并提供即时的视觉反馈,从而补充`jj`的功能。

The UI uses a couple of key conventions for discoverability:
UI使用了一些关键约定来提高可发现性:

- An _actionable object_ is represented by an icon followed by a line of text.
- These are drag sources, drop targets and context menu hosts.
- 一个*可操作对象*由一个图标后跟一行文本表示。
- 这些是拖拽源、放置目标和上下文菜单宿主。

- Chrome and labels are greyscale;
- anything interactable uses specific colours to indicate categories of widget or object states.
- 界面元素和标签使用灰度;
- 任何可交互元素都使用特定颜色来指示小部件或对象状态的类别。

## 架构选择 | Architectural Choices

In order to create a quality desktop app, a pure webapp is out of scope.
However, significant portions of the code could be reused in a client server app, and we won't introduce _needless_ coupling.
`mod worker` and `ipc.ts` are key abstraction boundaries which keep Tauri-specific code in its own glue layers.
为了创建一个高质量的桌面应用程序,纯web应用不在考虑范围内。
然而,代码的很大一部分可以在客户端-服务器应用中重用,我们不会引入*不必要的*耦合。
`mod worker`和`ipc.ts`是关键抽象边界,它们将Tauri特定的代码保持在各自的粘合层中。

Each window has a worker thread which owns `Session` data.
A session can be in multiple states, including:
每个窗口都有一个拥有`Session`数据的工作线程。
会话可以有多种状态,包括:

- `WorkerSession` - 打开/重新打开工作区
- `WorkerSession` - Opening/reopening a workspace

- `WorkspaceSession` - 工作区已打开,可以执行变更
- `WorkspaceSession` - Workspace open, able to execute mutations

- `QuerySession` - 分页查询进行中,可以高效获取数据
- `QuerySession` - Paged query in progress, able to fetch efficiently

IPC分为四类,这可能有点多:

IPC is divided into four categories, which is probably one too many:

- 客户端->服务器**触发器**导致后端执行原生UI操作。
- Client->Server **triggers** cause the backend to perform native UI actions.

- 客户端->服务器**查询**从会话请求信息而不影响状态。
- Client->Server **queries** request information from the session without affecting state.

- 客户端->服务器**变更**以结构化方式修改会话状态。
- Client->Server **mutations** modify session state in a structured fashion.

- 服务器->客户端和客户端->客户端**事件**广播以将信息推送到UI。
- Server->Client and Client->Client **events** are broadcast to push information to the UI.

Drag & drop capabilities are implemented by `objects/Object.svelte`, a draggable item, and `objects/Zone.svelte`, a droppable region.
Policy is centralised in `mutators/BinaryMutator.ts`.
拖放功能由`objects/Object.svelte`(可拖动项)和`objects/Zone.svelte`(可放置区域)实现。
策略集中在`mutators/BinaryMutator.ts`中。

## 分支对象 | Branch Objects

The representation of branches, in JJ and GG, is a bit complicated; there are multiple state axes.
A repository can have zero or more **remotes**.
A **local branch** can track zero or more of the remotes. (Technically, remote _branches_.)
A **remote branch** can be any of
_tracked_ (a flag on the ref),
_synced_ (if it points to the same commit as a local branch of the same name),
_absent_ (if there's a local branch with _no_ ref, in which case it will be deleted by the CLI on push.)
在JJ和GG中,分支的表示有点复杂;有多个状态轴。
一个仓库可以有零个或多个**远程仓库**。
一个**本地分支**可以跟踪零个或多个远程仓库。(技术上来说是远程*分支*。)
一个**远程分支**可以是
_tracked_(引用上的标志)、
_synced_(如果它指向与同名本地分支相同的提交)
_absent_(如果有一个没有引用的本地分支, 在这种情况下它将在推送时被CLI删除)。

GG attempts to simplify the display of branches by combining refs in the UI.
Taking advantage of Jujutsu's model, which guarantees that a branch name identifies the same branch across remotes,
a local branch and the tracked remote branches with which it is currently synced are be combined into a single UI object.
Remote branches are displayed separately if they're unsynced, untracked or absent.
GG试图通过在UI中组合引用来简化分支的显示。
利用Jujutsu的模型(它保证分支名称在远程仓库中标识相同的分支),
本地分支和当前同步的跟踪远程分支被组合成单个UI对象。
如果远程分支不同步、未跟踪或不存在,则单独显示。

Consequently, the commands available for a branch as displayed in the UI have polymorphic effect:
因此,UI中显示的分支可用的命令具有多态效果:

1. "Track": 适用于任何尚未跟踪的远程分支。
2. "Track": Applies to any remote branch that is not already tracked.

3. "Untrack":

   - 对于*跟踪本地/组合分支*,取消跟踪所有远程仓库。
   - For a _tracking local/combined branch_, untracks all remotes.
   - 对于*不同步的远程分支*,取消跟踪一个远程仓库。
   - For an _unsynced remote branch_, untracks one remote.

4. "Push": 适用于跟踪任何远程仓库的本地分支。
5. "Push": Applies to local branches tracking any remotes.

6. "Push to remote...": 当存在任何远程仓库时适用于本地分支。
7. "Push to remote...": Applies to local branches when any remotes exist.

8. "Fetch": 仅下载特定分支。
9. "Fetch": Downloads for a specific branch only.

   - 对于*跟踪本地/组合分支*,从所有远程仓库获取。
   - For a _tracking local/combined branch_, fetches from all remotes.
   - 对于*远程分支*,从其远程仓库获取。
   - For a _remote branch_, fetches from its remote.

10. "Fetch from remote...": 当存在任何可跟踪的远程仓库时适用于本地分支。
11. "Fetch from remote...": Applies to local branches when any trackable remotes exist.

12. "Rename...": 重命名本地分支,不影响远程分支。
13. "Rename...": Renames a local branch, without affecting remote branches.

- 对于*非跟踪本地分支*,仅重命名。
- For a _nontracking local branch_, just renames.
- 对于*跟踪/组合分支*,先取消跟踪。
- For a _tracking/combined branch_, untracks first.

14. "Delete": 适用于用户可见对象,而不是组合对象。
15. "Delete": Applies to a user-visible object, not combined objects.

- 对于*本地/组合分支*,删除本地引用。
- For a _local/combined branch_, deletes the local ref.
- 对于*远程分支*,忘记远程引用(这也清除待删除项)。
- For a _remote branch_, forgets the remote ref (which also clears pending deletes.)

Multiple-dispatch commands:
多重分派命令:

1. "Move": 将本地分支拖放到修订版本上。设置引用到提交,可能取消或重新同步。
2. "Move": Drop local branch onto revision. Sets the ref to a commit, potentially de- or re-syncing it.

3. "Track": 将远程分支拖放到同名本地分支上。
4. "Track": Drop remote branch onto local of the same name.

5. "Delete": 将几乎任何分支拖出,具有多态效果(见上文)。
6. "Delete": Drag almost any branch out, with polymorphic effect (see above).

Displaying the branch state is a bit fuzzy.
The idea is to convey the most useful bits of information at a glance, and leave the rest to tooltips or context menus.
Most branches display in the "modify" state;
"add" and "remove" are used only for _unsynced_ branches, with unsynced locals being "add" and unsynced or absent remotes "remove".
显示分支状态有点模糊。
其目的是让用户一眼就能看到最有用的信息,其余的细节则通过工具提示或上下文菜单查看。
大多数分支显示为"修改"状态;
"添加"和"删除"仅用于*不同步*的分支,其中不同步的本地分支显示为"添加",不同步或缺失的远程分支显示为"删除"。

This is vaguely analogous to the more straightforward use of modify/add/remove for file changes,
adapted to the fact that many branch states are "normal";
the mental shorthand is that add/green means that pushing will cause a remote to set this ref,
and remove/red means the remote will no longer contain this ref (at this pointer).
这种显示方式与文件变更中更直观的修改/添加/删除用法有些类似,
但适应了分支状态大多为"正常"的事实;
简单来说,添加/绿色意味着推送将导致远程设置此引用,
而删除/红色意味着远程将不再包含此引用(在此指针处)。

Additionally, a dashed border (like the dashed lines used for elided commits) has a special meaning,
also fuzzy: this ref is "disconnected", either local-only or remote-only.
Disconnected local branches are ones which have no remotes (in a repo that does have remotes);
disconnected remote branches are ones which will be deleted on push (with an absent local ref).
此外,虚线边框(类似于用于省略提交的虚线)有特殊含义,
同样有些模糊:这个引用是"断开的",要么是仅本地要么是仅远程。
断开的本地分支是指没有远程的分支(在具有远程的仓库中);
断开的远程分支是指将在推送时被删除的分支(没有对应的本地引用)。

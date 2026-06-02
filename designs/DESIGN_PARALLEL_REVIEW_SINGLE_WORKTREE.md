# Design Document: Parallel Review on a Single Worktree

## 1. Introduction and Goal

Currently, when Sashiko reviews a patchset with multiple patches in parallel, each parallel worker process spawns a new, separate Git worktree. Checking out the entire Linux kernel repository multiple times is extremely slow and consumes significant disk space and I/O bandwidth.

With the newly implemented **Virtualized HEAD** feature, the AI agent's Git tools no longer rely on the physical checkout state of the worktree. Instead, they query the Git object database directly using the virtualized HEAD commit SHA.

This enables a major optimization: **we can use strictly one worktree per patchset and review all patches in parallel using this single worktree**, completely eliminating the overhead of creating and destroying multiple worktrees.

## 2. Proposed Architecture

### 2.1. Single Worktree Lifetime

The lifecycle of the worktree will be managed entirely by the orchestrator (`src/reviewer.rs`):

1.  **Preparation Phase**:
    - The orchestrator creates a single baseline worktree.
    - It applies ALL patches in the patchset to this worktree in sequence (the validation pass). This populates the worktree's Git database with the commit SHAs for all patches in the series.
2.  **Review Phase (Parallel)**:
    - The orchestrator spawns parallel worker subprocesses (running the `review` binary).
    - **Every subprocess is passed the path of the SAME single worktree** using the `--reuse-worktree` argument.
    - Each subprocess is also passed its target `--review-patch-index` and `--review-commit` SHA.
3.  **Cleanup Phase**:
    - Once all parallel reviews are completed, the orchestrator destroys the single worktree.

### 2.2. Eliminating Physical Resets in `review` Binary

Since the AI tools use the virtual HEAD, the `review` subprocesses no longer need to physically align the worktree's files to the patch being reviewed.

We will modify `src/bin/review.rs` to **never perform physical checkouts or resets** during the review phase:
- Skip `worktree.reset_hard(commit_hash)` when `--review-commit` is specified.
- Skip resetting to baseline and re-applying subsets of patches when `--review-patch-index` is specified.

This ensures that the physical state of the single worktree remains completely static during the concurrent review phase, allowing multiple processes to read from its Git database simultaneously without I/O conflicts or state corruption.

### 2.3. Parallel Orchestration in `reviewer.rs`

We will update the concurrent spawning logic in `src/reviewer.rs`:
- Always pass `Some(&worktree.path)` to `process_patch_review` for both sequential and parallel tasks (previously, parallel tasks were passed `None`, forcing them to spawn their own worktrees).

```rust
                                match Self::process_patch_review(
                                    &ctx_clone,
                                    patchset_id,
                                    job.patch_id,
                                    job.index,
                                    &baseline_ref_clone,
                                    Some(baseline_id_clone),
                                    &input_payload_clone,
                                    job.commit_sha,
                                    prompts_hash_clone.as_deref(),
                                    Some(&worktree_path_clone), // <-- Always reuse!
                                    &job.diff,
                                    embargo_until_clone,
                                )
```

## 3. Verification Plan

### 3.1. Unit & Integration Tests
- Ensure all existing tests in `reviewer.rs` and `review.rs` pass.
- Verify that parallel reviews still produce correct findings.
- Run `make check-all` to run the complete test suite including integration tests.

### 3.2. Performance & Resource Verification
- Verify that parallel reviews no longer create multiple `sashiko-worktree-` directories in the temporary review folder.
- Measure the execution time of a multi-patch review to confirm the speedup from avoiding redundant worktree checkouts.

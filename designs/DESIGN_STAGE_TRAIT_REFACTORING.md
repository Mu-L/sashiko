# Design Document: Review Stages Refactoring

## Problem Statement
Currently, the review stages in Sashiko are represented as `u8` integers. The logic for executing, validating, and formatting feedback for these stages is scattered across `src/worker/prompts.rs` and `src/bin/review.rs` using `match` statements and conditional logic. This makes it difficult to maintain, extend, or understand the nuances of each stage.

## Proposed Solution
We propose to introduce a `ReviewStage` trait that encapsulates all stage-specific behavior, including:
- Stage number and name.
- Context requirements (e.g., whether to include logs).
- Validation logic for LLM responses.
- Validation feedback formatting.
- Specific error handling (e.g., recitation handling for Stage 11).

We will implement this trait for each of the 11 stages. `ReviewStageSession` will then hold a `Box<dyn ReviewStage>` and delegate stage-specific behavior to it.

## Detailed Design

### 1. The `ReviewStage` Trait
We will define the `ReviewStage` trait in a new file `src/worker/stage.rs`:

```rust
use crate::ai::{AiResponse, ValidationError, ErrorAction};
use serde_json::Value;

pub trait ReviewStage: Send + Sync {
    /// Returns the stage number (1..=11).
    fn number(&self) -> u8;

    /// Returns the stage name.
    fn name(&self) -> &'static str;

    /// Returns true if this stage should include the full log in its context.
    /// Stages 3-6 return false to optimize context size.
    fn use_log_in_context(&self) -> bool {
        true
    }

    /// Validates the LLM response for this stage.
    fn validate(&mut self, response: &AiResponse) -> Result<Value, ValidationError>;

    /// Formats the feedback message for the LLM when validation fails.
    fn format_validation_feedback(&self, violation: &str) -> String {
        format!(
            "\n\nPrevious attempt was rejected: {}. Please correct your output format.",
            violation
        )
    }

    /// Handles provider errors, specifically recitation errors.
    /// Returns `Some(ErrorAction)` if it handled the error, or `None` to fallback to default handling.
    fn handle_recitation_error(&mut self) -> Option<ErrorAction> {
        None
    }
}
```

### 2. Stage Implementations
We will implement `ReviewStage` for:
- `Stage1` to `Stage7` (sharing validation logic but having different names/numbers).
- `Stage8` (Deduplication).
- `Stage9` (Conflict Resolution).
- `Stage10` (Verification).
- `Stage11` (Report Generation, with custom recitation handling and `free_form_mode` state).

### 3. Factory Function
We will provide a factory function to create stage instances:
```rust
pub fn create_stage(stage: u8) -> Box<dyn ReviewStage> {
    match stage {
        1 => Box::new(Stage1),
        2 => Box::new(Stage2),
        // ...
        11 => Box::new(Stage11::new()),
        _ => panic!("Unsupported stage: {}", stage),
    }
}
```

### 4. Integration with `ReviewStageSession`
`ReviewStageSession` will be updated to use `Box<dyn ReviewStage>`:
```rust
struct ReviewStageSession {
    stage: Box<dyn ReviewStage>,
    // ... (other fields remain, except free_form_mode which moves to Stage11)
}
```
And its `LlmSession` implementation will delegate to `self.stage`.

### 5. Integration with `Worker`
`Worker::run` will be updated to use the stage trait to determine whether to use `shared_context` or `shared_context_no_log` based on `stage.use_log_in_context()`.

## Verification Plan
We will run `make check-pr` to ensure the refactoring does not break compilation, linting, or tests.
Since this is a pure refactoring, all existing tests must pass without modifications.

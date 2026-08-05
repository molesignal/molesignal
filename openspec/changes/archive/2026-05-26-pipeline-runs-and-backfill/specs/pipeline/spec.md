## ADDED Requirements

### Requirement: Scheduled Pipeline Runs Recorded

The system SHALL record every scheduled-pipeline execution attempt (success, failure, or cancellation) into the new `pipeline_runs` table defined by the `pipeline-runs` capability. The recording SHALL happen at the boundary of `PipelineService::run_one` (or equivalent existing scheduler entry point) and SHALL NOT change the existing pipeline scheduling, retry, or dispatch semantics.

#### Scenario: Tick records a run row

- **WHEN** the scheduler executes a tick for pipeline `p1`
- **THEN** a row is inserted into `pipeline_runs` with `pipeline_id = 'p1'` and `state = 'running'` before the work begins
- **AND** the row is updated to `state = 'succeeded' | 'failed' | 'cancelled'` when the tick ends
- **AND** the existing pipeline outputs (downstream stream writes, alert dispatches, etc.) are unchanged

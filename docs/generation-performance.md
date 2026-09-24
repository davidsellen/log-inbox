# Daily generation performance

Daily exposes the current attempt, elapsed time and the configured total timeout. Server logs record model request time and prompt byte counts without recording prompts, note contents or credentials. A fast HTTP acceptance response is not a completed draft.

Daily batches up to four existing evidence groups per request, with a 16 KiB prompt target. A larger single group remains subject to the existing hard input limit. Groups and server-owned evidence IDs are never merged by batching. Invalid multi-group output falls back once to individual requests for that batch; transport failures are not retried. All work shares the existing attempt deadline. Progress counts validated groups, not generated tokens, and discloses fallback.

Reference snapshots are validated before inference. A mapped note outside the selected usable collections fails explicitly; Daily offers a one-attempt retry without reference notes rather than reading unselected files. This does not change reference settings.

## Repeatable local-model measurement

The normal test suite never contacts a real model. The ignored benchmark accepts a private JSON array containing one to three representative days, ordered small, medium and large. Each item has `events` (the existing `StoredLogEvent` JSON shape) and `vault_context` (the bounded context projection actually supplied to generation). Keep this file outside source control and exclude secrets. The benchmark produces no revisions or Markdown and prints only counts, durations and validation outcomes.

Run with an explicitly configured local model:

```sh
LOG_INBOX_BENCHMARK_INPUT=/tmp/private-daily-benchmark.json \
LOG_INBOX_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
LOG_INBOX_LLM_MODEL=granite3.3:2b \
cargo test -p log-inbox-mcp-server benchmark_representative_days -- --ignored --nocapture
```

For each day it compares activity-only and supplied context, with two consecutive passes. Every pass validates the structured output and has the configured total timeout. A failed or timed-out result remains a result, not a fast successful sample. A first pass is **not automatically cold**: note the model's actual load state separately. To measure cold loading, use an isolated provider started without a loaded model; do not unload a model serving active Daily work.

Empty context fixtures skip the duplicate context run. Set `LOG_INBOX_BENCHMARK_VARIANT=baseline` to measure the prior single-group request strategy on the same fixture and model. Omit it for batching. Benchmark progress prints completed/total groups and whether fallback was used. Keep the model idle from other workloads during comparisons and do not run both variants concurrently.

Compare output usefulness as well as elapsed time. Context is worthwhile only if it improves factual usefulness or reduces editing. Do not lower evidence coverage or claim a speed improvement from a faster invalid output. Keep the existing timeout until measured representative results justify a change.

Real-day quality evaluation and the ten-active-day pilot require actual use. Synthetic workflow tests establish reliability, not summary quality or real-model speed.
